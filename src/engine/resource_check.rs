//! Post-merge resource-integrity check (Requirement 15).
//!
//! After the `Split_Merge_Engine` (Subsystem A, `pdf_split_merge`) produces a
//! merged output, this module verifies - for every page of that output - that
//! each resource the page references (`/Resources`, `/Font`, `/XObject`,
//! `/ExtGState`) is actually present in the document before the output is
//! emitted (Req 15.1).
//!
//! It is a pure-Rust, `lopdf`-only module. It deliberately contains no
//! reference to the Pro editor stack or its FFI bridge: structural resource
//! checking never needs the Pro library. (A render probe via `pdfium-render`
//! is intentionally avoided here - see the warn-vs-fatal heuristic below -
//! keeping the check cheap and deterministic.)
//!
//! ## Warn-vs-fatal heuristic
//!
//! A full "does this page render?" answer needs a rasterizer, which is
//! expensive and can produce false-fatals. To stay tractable and avoid
//! discarding output that would actually render fine, this module classifies
//! a missing resource as follows:
//!
//! * **Warning (Req 15.2 - record + emit):** a referenced entry inside a
//!   `/Font`, `/XObject`, or `/ExtGState` sub-dictionary (or the sub-dictionary
//!   itself) does not resolve. The page very likely still renders, just
//!   degraded (a missing glyph program, image, or graphics-state). The caller
//!   records the affected Global_Page + category and emits the output.
//!
//! * **Fatal (Req 15.3 - error + retain original):** the page cannot render at
//!   all, which we treat structurally as either of:
//!     1. the page's `/Resources` entry is a *dangling reference* (the object
//!        id it points at is absent), so the resource dictionary itself cannot
//!        be resolved; or
//!     2. the page *declares* content (`/Contents`) but **none** of the
//!        referenced content streams resolve, so there is no drawable content
//!        stream at all.
//!        A page that simply has no `/Contents` entry is a legal blank page and is
//!        therefore renderable (not fatal).
//!
//! This is a deliberate, documented approximation. The caller (Fidelity_Guard,
//! wired in task 12.1) may layer an optional `pdfium-render` probe on top, but
//! the structural heuristic here is sufficient and preferred for default use.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId};

/// The sub-resource categories a page may reference, checked in addition to the
/// top-level `/Resources` dictionary (Req 15.1).
const RESOURCE_CATEGORIES: [&[u8]; 3] = [b"Font", b"XObject", b"ExtGState"];

/// A single resource-integrity finding for one merged page.
///
/// `category` is the human-readable resource category the issue concerns
/// (`"Resources"`, `"Font"`, `"XObject"`, `"ExtGState"`, or `"Contents"`),
/// and `global_page` is the 0-based document-wide page index in the merged
/// output (the space the UI works in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceIssue {
    /// A referenced resource was not carried over but the page still renders
    /// (degraded). Maps to Req 15.2: record a warning and emit the output.
    Warning {
        global_page: usize,
        category: String,
    },
    /// A missing resource makes the page unrenderable. Maps to Req 15.3:
    /// return an error and retain the original document instead of emitting.
    Fatal {
        global_page: usize,
        category: String,
    },
}

impl ResourceIssue {
    /// Render this issue as a stable, human-readable warning/diagnostic string.
    fn message(&self) -> String {
        match self {
            ResourceIssue::Warning { global_page, category } => format!(
                "global page {global_page}: missing /{category} resource not carried over during merge (page still renders, degraded)"
            ),
            ResourceIssue::Fatal { global_page, category } => format!(
                "global page {global_page}: missing /{category} makes the page unrenderable"
            ),
        }
    }
}

/// Errors returned by [`check_merged_resources`].
#[derive(Debug, thiserror::Error)]
pub enum ResourceCheckError {
    /// `lopdf` failed to load the merged document for inspection.
    #[error("resource check failed to load {path}: {source}")]
    Load { path: PathBuf, source: lopdf::Error },
    /// A missing resource makes a page unrenderable (Req 15.3). Carries the
    /// offending 0-based Global_Page so the caller can identify it, retain the
    /// original document, and avoid emitting a visually broken PDF.
    #[error("global page {global_page} of the merged output is unrenderable: missing /{category}")]
    UnrenderablePage {
        global_page: usize,
        category: String,
    },
    /// The merged document has malformed structure that prevents the check
    /// from inspecting a page (e.g. a page object that is not a dictionary).
    #[error("malformed merged PDF structure: {0}")]
    Structure(String),
}

/// Verify resource integrity of a merged PDF before it is emitted (Req 15).
///
/// For every page of `merged_path` (iterated in ascending 0-based Global_Page
/// order), this resolves the page's `/Resources` dictionary and confirms that
/// every object referenced by it and by its `/Font`, `/XObject`, and
/// `/ExtGState` sub-dictionaries resolves in the document.
///
/// * On success, returns the list of **warning** messages for resources that
///   were not carried over but whose pages still render (Req 15.2). An empty
///   vector means full resource integrity.
/// * Returns [`ResourceCheckError::UnrenderablePage`] - carrying the offending
///   Global_Page - when a missing resource makes a page unrenderable, so the
///   caller can retain the original document instead of emitting (Req 15.3).
///
/// The check stops at the first fatal (unrenderable) page; warnings on pages
/// inspected before it are discarded because no output will be emitted anyway.
pub fn check_merged_resources(merged_path: &Path) -> Result<Vec<String>, ResourceCheckError> {
    let mut doc = Document::load(merged_path).map_err(|e| ResourceCheckError::Load {
        path: merged_path.to_path_buf(),
        source: e,
    })?;
    // Decompress so object streams are expanded and every referenced id is
    // directly resolvable, matching how the merge step inspects documents.
    doc.decompress();

    let mut warnings: Vec<String> = Vec::new();

    // `get_pages` yields a 1-based page-number -> page object-id map in page
    // order; the 0-based Global_Page is therefore `page_num - 1`.
    for (page_num, page_id) in doc.get_pages() {
        let global_page = page_num.saturating_sub(1) as usize;

        // `check_page` returns this page's issues (warnings and/or a fatal).
        // A `Fatal` means the page is unrenderable: stop and report it as an
        // error so the caller retains the original document instead of emitting
        // (Req 15.3). Otherwise accumulate the page's warnings (Req 15.2).
        for issue in check_page(&doc, global_page, page_id)? {
            match issue {
                ResourceIssue::Warning { .. } => warnings.push(issue.message()),
                ResourceIssue::Fatal {
                    global_page,
                    category,
                } => {
                    return Err(ResourceCheckError::UnrenderablePage {
                        global_page,
                        category,
                    });
                }
            }
        }
    }

    Ok(warnings)
}

/// Check one merged page and return its resource issues (warnings and/or a
/// single fatal). Pure structural inspection - no rendering.
///
/// At most one [`ResourceIssue::Fatal`] is produced per page; when present it
/// is the last (and only) issue in the returned vector. The only [`Err`]
/// returned is for malformed structure that blocks inspection.
fn check_page(
    doc: &Document,
    global_page: usize,
    page_id: ObjectId,
) -> Result<Vec<ResourceIssue>, ResourceCheckError> {
    let mut issues: Vec<ResourceIssue> = Vec::new();

    let page_dict = doc.get_dictionary(page_id).map_err(|e| {
        ResourceCheckError::Structure(format!(
            "global page {global_page} object {page_id:?} is not a dictionary: {e}"
        ))
    })?;

    // --- Fatal check 1: a page that declares content must have at least one
    //     resolvable content stream, otherwise there is nothing to draw. ---
    let declared_contents = page_declares_contents(page_dict);
    if declared_contents {
        let content_ids = doc.get_page_contents(page_id);
        let any_resolvable = content_ids.iter().any(|id| doc.has_object(*id));
        if !any_resolvable {
            issues.push(ResourceIssue::Fatal {
                global_page,
                category: "Contents".to_string(),
            });
            return Ok(issues);
        }
    }

    // --- Resolve the page's /Resources, distinguishing a dangling reference
    //     (fatal) from "absent / inline / inherited" (handled below). ---
    let resources = match resolve_resources(doc, page_dict) {
        ResourcesResolution::Dict(dict) => Some(dict),
        ResourcesResolution::None => None,
        // Fatal check 2: the /Resources entry points at an object id that is
        // not present. The resource dictionary itself cannot be resolved, so
        // the page cannot render.
        ResourcesResolution::Dangling => {
            issues.push(ResourceIssue::Fatal {
                global_page,
                category: "Resources".to_string(),
            });
            return Ok(issues);
        }
    };

    // --- Warnings: missing entries within /Font, /XObject, /ExtGState. A
    //     missing sub-resource degrades the page but does not stop it from
    //     rendering, so it is recorded and the output is still emitted. ---
    if let Some(resources) = resources {
        for category in RESOURCE_CATEGORIES {
            if category_has_missing_resource(doc, &resources, category) {
                issues.push(ResourceIssue::Warning {
                    global_page,
                    category: String::from_utf8_lossy(category).into_owned(),
                });
            }
        }
    }

    Ok(issues)
}

/// Whether a page declares any content via a `/Contents` entry. An array form
/// with zero elements counts as "no declared content" (legal blank page).
fn page_declares_contents(page_dict: &Dictionary) -> bool {
    match page_dict.get(b"Contents") {
        Ok(Object::Reference(_)) => true,
        Ok(Object::Array(arr)) => !arr.is_empty(),
        _ => false,
    }
}

/// Outcome of resolving a page's `/Resources` entry.
enum ResourcesResolution<'a> {
    /// A resolvable resource dictionary (inline, resolved reference, or
    /// inherited from an ancestor `/Pages` node).
    Dict(std::borrow::Cow<'a, Dictionary>),
    /// The `/Resources` entry is a reference to an absent object (fatal).
    Dangling,
    /// The page (and its ancestors) declare no `/Resources` at all.
    None,
}

/// Resolve a page's effective `/Resources` dictionary.
///
/// Order of resolution: the leaf's own `/Resources` (inline dict or reference),
/// then - if absent - the nearest ancestor `/Pages` node via the `/Parent`
/// chain (resources are an inheritable attribute per the PDF spec). A
/// `/Resources` reference whose target object is missing is reported as
/// [`ResourcesResolution::Dangling`] so the caller can treat it as fatal.
fn resolve_resources<'a>(doc: &'a Document, page_dict: &'a Dictionary) -> ResourcesResolution<'a> {
    match resolve_resources_on(doc, page_dict) {
        Some(Ok(dict)) => return ResourcesResolution::Dict(dict),
        Some(Err(())) => return ResourcesResolution::Dangling,
        None => {}
    }

    // Walk the `/Parent` chain looking for an inherited `/Resources`, guarding
    // against cycles in malformed trees.
    let mut parent_ref = page_dict.get(b"Parent").and_then(Object::as_reference).ok();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    while let Some(parent_id) = parent_ref {
        if !seen.insert(parent_id) {
            break;
        }
        let node = match doc.get_dictionary(parent_id) {
            Ok(node) => node,
            Err(_) => break,
        };
        match resolve_resources_on(doc, node) {
            Some(Ok(dict)) => return ResourcesResolution::Dict(dict),
            Some(Err(())) => return ResourcesResolution::Dangling,
            None => {}
        }
        parent_ref = node.get(b"Parent").and_then(Object::as_reference).ok();
    }

    ResourcesResolution::None
}

/// Resolve the `/Resources` entry present directly on one dictionary.
/// Returns `None` when the dictionary has no `/Resources`, `Some(Ok(dict))`
/// when it resolves, and `Some(Err(()))` when it is a dangling reference.
fn resolve_resources_on<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
) -> Option<Result<std::borrow::Cow<'a, Dictionary>, ()>> {
    match dict.get(b"Resources") {
        Ok(Object::Dictionary(d)) => Some(Ok(std::borrow::Cow::Borrowed(d))),
        Ok(Object::Reference(id)) => match doc.get_dictionary(*id) {
            Ok(d) => Some(Ok(std::borrow::Cow::Borrowed(d))),
            Err(_) => Some(Err(())),
        },
        _ => None,
    }
}

/// Whether a resource sub-category (`/Font`, `/XObject`, `/ExtGState`) contains
/// a reference that does not resolve in the document - either the category
/// sub-dictionary itself is a dangling reference, or one of its entries is.
fn category_has_missing_resource(doc: &Document, resources: &Dictionary, category: &[u8]) -> bool {
    let category_dict = match resources.get(category) {
        Ok(Object::Dictionary(dict)) => dict,
        Ok(Object::Reference(id)) => match doc.get_dictionary(*id) {
            Ok(dict) => dict,
            // Dangling category sub-dictionary: the category cannot be
            // enumerated, so its resources are effectively missing.
            Err(_) => return true,
        },
        // Category absent or not a dictionary: nothing referenced, nothing
        // missing.
        _ => return false,
    };

    category_dict.iter().any(|(_name, value)| {
        if let Object::Reference(id) = value {
            !doc.has_object(*id)
        } else {
            false
        }
    })
}
