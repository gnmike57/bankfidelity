use crate::engine::verification::{VerificationGate, VerificationGateStatus};
use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone)]
struct PageSignature {
    media_box: [f32; 4],
    crop_box: [f32; 4],
    rotation: i64,
    content_nonempty: bool,
    text_nonempty: bool,
    text_anchors: BTreeSet<String>,
    fonts: BTreeSet<String>,
}

fn inherited_object<'a>(
    document: &'a Document,
    mut object_id: ObjectId,
    key: &[u8],
) -> Result<Option<&'a Object>, String> {
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(object_id) {
            return Err(format!(
                "page tree contains a parent cycle at {object_id:?}"
            ));
        }
        let dictionary = document
            .get_dictionary(object_id)
            .map_err(|error| format!("page node {object_id:?} is invalid: {error}"))?;
        if let Ok(object) = dictionary.get(key) {
            let (_, object) = document.dereference(object).map_err(|error| {
                format!(
                    "page node {object_id:?} {} cannot be dereferenced: {error}",
                    String::from_utf8_lossy(key)
                )
            })?;
            return Ok(Some(object));
        }
        object_id = match dictionary.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => parent,
            Err(_) => return Ok(None),
        };
    }
}

fn page_box(
    document: &Document,
    page_id: ObjectId,
    key: &[u8],
) -> Result<Option<[f32; 4]>, String> {
    let Some(object) = inherited_object(document, page_id, key)? else {
        return Ok(None);
    };
    let values = object.as_array().map_err(|error| {
        format!(
            "page {page_id:?} {} is not an array: {error}",
            String::from_utf8_lossy(key)
        )
    })?;
    if values.len() != 4 {
        return Err(format!(
            "page {page_id:?} {} has {} values, expected four",
            String::from_utf8_lossy(key),
            values.len()
        ));
    }
    let mut result = [0.0_f32; 4];
    for (index, value) in values.iter().enumerate() {
        result[index] = value.as_float().map_err(|error| {
            format!(
                "page {page_id:?} {} value {index} is not numeric: {error}",
                String::from_utf8_lossy(key)
            )
        })?;
    }
    Ok(Some(result))
}

fn page_rotation(document: &Document, page_id: ObjectId) -> Result<i64, String> {
    let Some(object) = inherited_object(document, page_id, b"Rotate")? else {
        return Ok(0);
    };
    object
        .as_i64()
        .map(|value| value.rem_euclid(360))
        .map_err(|error| format!("page {page_id:?} Rotate is not an integer: {error}"))
}

fn normalize_font_name(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name)
        .trim_start_matches('/')
        .to_string();
    if name.len() > 7
        && name.as_bytes()[6] == b'+'
        && name.as_bytes()[..6]
            .iter()
            .all(|byte| byte.is_ascii_uppercase())
    {
        name[7..].to_string()
    } else {
        name
    }
}

fn page_fonts(document: &Document, page_id: ObjectId) -> Result<BTreeSet<String>, String> {
    let fonts = document
        .get_page_fonts(page_id)
        .map_err(|error| format!("cannot resolve page {page_id:?} fonts: {error}"))?;
    Ok(fonts
        .values()
        .filter_map(|font| font.get(b"BaseFont").ok())
        .filter_map(|object| object.as_name().ok())
        .map(normalize_font_name)
        .collect())
}

fn text_anchors(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() >= 4)
        .filter(|token| token.chars().any(char::is_alphabetic))
        .map(|token| token.to_lowercase())
        .collect()
}

fn page_signatures(document: &Document) -> Result<Vec<PageSignature>, String> {
    document
        .get_pages()
        .into_iter()
        .map(|(page_number, page_id)| {
            let media_box = page_box(document, page_id, b"MediaBox")?
                .ok_or_else(|| format!("page {page_number} has no inherited MediaBox"))?;
            let crop_box = page_box(document, page_id, b"CropBox")?.unwrap_or(media_box);
            let content = document
                .get_page_content(page_id)
                .map_err(|error| format!("cannot read page {page_number} content: {error}"))?;
            let text = document
                .extract_text(&[page_number])
                .map_err(|error| format!("cannot extract page {page_number} text: {error}"))?;
            Ok(PageSignature {
                media_box,
                crop_box,
                rotation: page_rotation(document, page_id)?,
                content_nonempty: content.iter().any(|byte| !byte.is_ascii_whitespace()),
                text_nonempty: !text.trim().is_empty(),
                text_anchors: text_anchors(&text),
                fonts: page_fonts(document, page_id)?,
            })
        })
        .collect()
}

fn boxes_equal(left: [f32; 4], right: [f32; 4]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() <= 0.01)
}

fn object_text(document: &Document, object: &Object) -> Option<String> {
    let (_, object) = document.dereference(object).ok()?;
    match object {
        Object::String(bytes, _) | Object::Name(bytes) => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        Object::Integer(value) => Some(value.to_string()),
        Object::Real(value) => Some(value.to_string()),
        Object::Boolean(value) => Some(value.to_string()),
        _ => None,
    }
}

fn info_value(document: &Document, key: &[u8]) -> Option<String> {
    let info = document.trailer.get(b"Info").ok()?;
    let (_, info) = document.dereference(info).ok()?;
    let dictionary = info.as_dict().ok()?;
    object_text(document, dictionary.get(key).ok()?)
}

fn catalog_value(document: &Document, key: &[u8]) -> Option<String> {
    let root = document.trailer.get(b"Root").ok()?;
    let (_, root) = document.dereference(root).ok()?;
    let dictionary = root.as_dict().ok()?;
    object_text(document, dictionary.get(key).ok()?)
}

fn gate(id: &str, passed: bool, message: String) -> VerificationGate {
    VerificationGate::mandatory(
        id,
        if passed {
            VerificationGateStatus::Passed
        } else {
            VerificationGateStatus::Failed
        },
        message,
    )
}

/// Validates cross-reference and structural compliance of a PDF file according to ISO 32000-1:
/// - %PDF- header presence within first 1024 bytes
/// - %%EOF marker presence within trailing window
/// - startxref keyword and valid numeric byte offset within file length
/// - target at offset starts with 'xref' table or cross-reference stream object
/// - trailer /Root catalog dictionary resolution
/// - catalog /Pages reference and dictionary resolution with /Kids or /Count
/// - page tree resolvable pages nonempty
pub fn verify_xref_compliance(path: &Path, doc: Option<&Document>) -> (bool, String) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return (false, format!("cannot read PDF file: {e}")),
    };

    if bytes.len() < 32 {
        return (false, "PDF file is too small (< 32 bytes)".into());
    }

    // 1. Header check: %PDF- within the first 1024 bytes
    let header_len = bytes.len().min(1024);
    if !bytes[..header_len].windows(5).any(|w| w == b"%PDF-") {
        return (false, "missing %PDF- header in first 1024 bytes".into());
    }

    // 2. Trailing window: %%EOF within trailing 4096 bytes
    let trailer_search_len = bytes.len().min(4096);
    let trailer_start = bytes.len() - trailer_search_len;
    let Some(eof_rel_pos) = bytes[trailer_start..]
        .windows(5)
        .rposition(|w| w == b"%%EOF")
    else {
        return (false, "missing %%EOF marker in trailing 4096 bytes".into());
    };
    let eof_abs_pos = trailer_start + eof_rel_pos;

    // 3. startxref keyword preceding %%EOF
    let Some(startxref_pos) = bytes[..eof_abs_pos]
        .windows(9)
        .rposition(|w| w == b"startxref")
    else {
        return (false, "missing startxref keyword before %%EOF".into());
    };

    let after_startxref = &bytes[startxref_pos + 9..eof_abs_pos];
    let offset_digits: String = after_startxref
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();

    if offset_digits.is_empty() {
        return (
            false,
            "startxref keyword not followed by numeric byte offset".into(),
        );
    }

    let Ok(startxref_offset) = offset_digits.parse::<usize>() else {
        return (
            false,
            format!("invalid startxref byte offset '{offset_digits}'"),
        );
    };

    if startxref_offset >= bytes.len() {
        return (
            false,
            format!(
                "startxref offset {startxref_offset} exceeds file length {}",
                bytes.len()
            ),
        );
    }

    // 4. Verify target at startxref_offset points to xref table or stream object
    let target = &bytes[startxref_offset..];
    let skip_ws = target
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(0);
    let trimmed_target = &target[skip_ws..];

    let is_xref_table = trimmed_target.starts_with(b"xref");
    let is_xref_stream = if !is_xref_table {
        let prefix_len = trimmed_target.len().min(64);
        let prefix = &trimmed_target[..prefix_len];
        if let Ok(prefix_str) = std::str::from_utf8(prefix) {
            let mut tokens = prefix_str.split_whitespace();
            let num = tokens.next().and_then(|s| s.parse::<u32>().ok());
            let gen = tokens.next().and_then(|s| s.parse::<u16>().ok());
            let obj = tokens.next();
            num.is_some() && gen.is_some() && obj == Some("obj")
        } else {
            false
        }
    } else {
        false
    };

    if !is_xref_table && !is_xref_stream {
        return (
            false,
            format!(
                "startxref offset {startxref_offset} does not point to 'xref' table or cross-reference stream object"
            ),
        );
    }

    // 5. Document-level structural validation
    let loaded_doc;
    let document = match doc {
        Some(d) => d,
        None => match Document::load_from(&mut std::io::Cursor::new(&bytes)) {
            Ok(d) => {
                loaded_doc = d;
                &loaded_doc
            }
            Err(e) => {
                return (
                    false,
                    format!("PDF cross-reference or structural parse error: {e}"),
                )
            }
        },
    };

    // Trailer /Root check
    let root_obj = match document.trailer.get(b"Root") {
        Ok(obj) => obj,
        Err(_) => return (false, "trailer is missing /Root catalog reference".into()),
    };

    let (_, catalog_obj) = match document.dereference(root_obj) {
        Ok(pair) => pair,
        Err(e) => return (false, format!("cannot dereference /Root catalog: {e}")),
    };

    let catalog_dict = match catalog_obj.as_dict() {
        Ok(dict) => dict,
        Err(_) => return (false, "/Root catalog object is not a dictionary".into()),
    };

    if let Ok(type_obj) = catalog_dict.get(b"Type") {
        if let Ok(type_name) = type_obj.as_name() {
            if type_name != b"Catalog" {
                return (
                    false,
                    format!(
                        "catalog /Type is not /Catalog: /{}",
                        String::from_utf8_lossy(type_name)
                    ),
                );
            }
        }
    }

    let pages_ref = match catalog_dict.get(b"Pages") {
        Ok(obj) => obj,
        Err(_) => {
            return (
                false,
                "catalog dictionary is missing /Pages reference".into(),
            )
        }
    };

    let (_, pages_obj) = match document.dereference(pages_ref) {
        Ok(pair) => pair,
        Err(e) => return (false, format!("cannot dereference /Pages object: {e}")),
    };

    let pages_dict = match pages_obj.as_dict() {
        Ok(dict) => dict,
        Err(_) => return (false, "/Pages object is not a dictionary".into()),
    };

    if !pages_dict.has(b"Kids") && !pages_dict.has(b"Count") {
        return (
            false,
            "/Pages dictionary is missing /Kids and /Count".into(),
        );
    }

    let page_map = document.get_pages();
    if page_map.is_empty() {
        return (
            false,
            "PDF contains no resolvable pages in page tree".into(),
        );
    }

    (
        true,
        "XRef table/stream, header, and catalog structure comply with PDF specification".into(),
    )
}

pub fn verify_structural_invariants(
    original_path: &Path,
    edited_path: &Path,
) -> Result<Vec<VerificationGate>, String> {
    let original = Document::load(original_path)
        .map_err(|error| format!("cannot load original PDF structure: {error}"))?;

    let edited = match Document::load(edited_path) {
        Ok(doc) => doc,
        Err(load_error) => {
            let (_, compliance_msg) = verify_xref_compliance(edited_path, None);
            let message = if compliance_msg.contains("comply") {
                format!("PDF loading failed: {load_error}")
            } else {
                compliance_msg
            };
            return Ok(vec![
                gate(
                    "structure.page_count",
                    false,
                    "cannot load edited PDF structure".into(),
                ),
                gate(
                    "structure.page_geometry",
                    false,
                    "cannot load edited PDF structure".into(),
                ),
                gate(
                    "structure.content_presence",
                    false,
                    "cannot load edited PDF structure".into(),
                ),
                gate(
                    "structure.page_identity",
                    false,
                    "cannot load edited PDF structure".into(),
                ),
                gate(
                    "structure.font_resources",
                    false,
                    "cannot load edited PDF structure".into(),
                ),
                gate(
                    "structure.metadata_policy",
                    false,
                    "cannot load edited PDF structure".into(),
                ),
                gate("structure.xref_compliance", false, message),
            ]);
        }
    };

    let original_pages = page_signatures(&original)?;
    let edited_pages = match page_signatures(&edited) {
        Ok(pages) => pages,
        Err(sig_error) => {
            let (_, compliance_msg) = verify_xref_compliance(edited_path, Some(&edited));
            return Ok(vec![
                gate(
                    "structure.page_count",
                    false,
                    format!("corrupted edited pages: {sig_error}"),
                ),
                gate(
                    "structure.page_geometry",
                    false,
                    format!("corrupted edited pages: {sig_error}"),
                ),
                gate(
                    "structure.content_presence",
                    false,
                    format!("corrupted edited pages: {sig_error}"),
                ),
                gate(
                    "structure.page_identity",
                    false,
                    format!("corrupted edited pages: {sig_error}"),
                ),
                gate(
                    "structure.font_resources",
                    false,
                    format!("corrupted edited pages: {sig_error}"),
                ),
                gate(
                    "structure.metadata_policy",
                    false,
                    format!("corrupted edited pages: {sig_error}"),
                ),
                gate("structure.xref_compliance", false, compliance_msg),
            ]);
        }
    };

    let page_counts_match = original_pages.len() == edited_pages.len();
    let mut gates = vec![gate(
        "structure.page_count",
        page_counts_match,
        format!(
            "original pages={}, edited pages={}",
            original_pages.len(),
            edited_pages.len()
        ),
    )];

    let comparable = original_pages.len().min(edited_pages.len());
    let geometry_failures: Vec<usize> = (0..comparable)
        .filter(|index| {
            let source = &original_pages[*index];
            let candidate = &edited_pages[*index];
            !boxes_equal(source.media_box, candidate.media_box)
                || !boxes_equal(source.crop_box, candidate.crop_box)
                || source.rotation != candidate.rotation
        })
        .map(|index| index + 1)
        .collect();
    gates.push(gate(
        "structure.page_geometry",
        page_counts_match && geometry_failures.is_empty(),
        if geometry_failures.is_empty() {
            "MediaBox, CropBox, and rotation match on every page".into()
        } else {
            format!("page geometry differs on pages {geometry_failures:?}")
        },
    ));

    let presence_failures: Vec<usize> = (0..comparable)
        .filter(|index| {
            let source = &original_pages[*index];
            let candidate = &edited_pages[*index];
            (source.content_nonempty && !candidate.content_nonempty)
                || (source.text_nonempty && !candidate.text_nonempty)
        })
        .map(|index| index + 1)
        .collect();
    gates.push(gate(
        "structure.content_presence",
        page_counts_match && presence_failures.is_empty(),
        if presence_failures.is_empty() {
            "no source page became empty or textless".into()
        } else {
            format!("content is missing on pages {presence_failures:?}")
        },
    ));

    let mut identity_failures = Vec::new();
    let mut worst_anchor_recall = 1.0_f64;
    for index in 0..comparable {
        let source = &original_pages[index].text_anchors;
        if source.len() < 3 {
            continue;
        }
        let candidate = &edited_pages[index].text_anchors;
        let retained = source.intersection(candidate).count();
        let recall = retained as f64 / source.len() as f64;
        worst_anchor_recall = worst_anchor_recall.min(recall);
        if recall < 0.60 {
            identity_failures.push(index + 1);
        }
    }
    gates.push(gate(
        "structure.page_identity",
        page_counts_match && identity_failures.is_empty(),
        if identity_failures.is_empty() {
            format!("per-page stable-text anchor recall >= {worst_anchor_recall:.3}")
        } else {
            format!(
                "page identity/order anchor recall fell below 0.60 on pages {identity_failures:?}"
            )
        },
    ));

    let font_failures: Vec<usize> = (0..comparable)
        .filter(|index| {
            let source = &original_pages[*index].fonts;
            let candidate = &edited_pages[*index].fonts;
            !source.is_subset(candidate)
        })
        .map(|index| index + 1)
        .collect();
    gates.push(gate(
        "structure.font_resources",
        page_counts_match && font_failures.is_empty(),
        if font_failures.is_empty() {
            "every source page font family remains available".into()
        } else {
            format!("source font resources are missing on pages {font_failures:?}")
        },
    ));

    let mut metadata_mismatches = Vec::new();
    for key in [b"Title".as_slice(), b"Author", b"Subject", b"Keywords"] {
        if info_value(&original, key) != info_value(&edited, key) {
            metadata_mismatches.push(format!("Info.{}", String::from_utf8_lossy(key)));
        }
    }
    for key in [b"Lang".as_slice(), b"PageMode", b"PageLayout"] {
        if catalog_value(&original, key) != catalog_value(&edited, key) {
            metadata_mismatches.push(format!("Catalog.{}", String::from_utf8_lossy(key)));
        }
    }
    gates.push(gate(
        "structure.metadata_policy",
        metadata_mismatches.is_empty(),
        if metadata_mismatches.is_empty() {
            "stable Info and catalog metadata match policy".into()
        } else {
            format!(
                "stable metadata differs: {}",
                metadata_mismatches.join(", ")
            )
        },
    ));

    let (xref_passed, xref_message) = verify_xref_compliance(edited_path, Some(&edited));
    gates.push(gate("structure.xref_compliance", xref_passed, xref_message));

    Ok(gates)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn subset_prefix_is_normalized() {
        assert_eq!(normalize_font_name(b"ABCDEF+Helvetica"), "Helvetica");
        assert_eq!(normalize_font_name(b"Helvetica"), "Helvetica");
    }

    #[test]
    fn text_anchor_selection_ignores_numeric_mutations() {
        let anchors = text_anchors("01/02/2026 Coffee Shop 123.45 900.00");
        assert_eq!(anchors, BTreeSet::from(["coffee".into(), "shop".into()]));
    }
}
