use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Metadata returned for one produced segment.
#[derive(Debug, Clone)]
pub struct SplitSegment {
    pub path: PathBuf,
    pub page_offset: usize,
    pub page_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SplitMergeError {
    #[error("lopdf failed to load {path}: {source}")]
    Load { path: PathBuf, source: lopdf::Error },
    #[error("lopdf failed to save {path}: {source}")]
    Save { path: PathBuf, source: lopdf::Error },
    #[error("malformed PDF structure: {0}")]
    Structure(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Split `src_path` into ordered <=`max_pages` segment files in `out_dir`,
/// using lopdf only. Returns ordered segment metadata.
pub fn split_pdf(
    src_path: &Path,
    out_dir: &Path,
    max_pages: usize,
) -> Result<Vec<SplitSegment>, SplitMergeError> {
    if max_pages == 0 {
        return Err(SplitMergeError::Structure("max_pages must be > 0".into()));
    }

    let mut doc = Document::load(src_path).map_err(|e| SplitMergeError::Load {
        path: src_path.to_path_buf(),
        source: e,
    })?;

    doc.decompress();

    let page_map = doc.get_pages();
    let total_pages = page_map.len();
    // Ordered (1-based page number -> object id) so we can window contiguously.
    let mut ordered_pages: Vec<(u32, ObjectId)> = page_map.into_iter().collect();
    ordered_pages.sort_by_key(|(num, _)| *num);

    let mut segments = Vec::new();

    if !out_dir.exists() {
        std::fs::create_dir_all(out_dir)?;
    }

    for (idx, start) in (0..total_pages).step_by(max_pages).enumerate() {
        let end = (start + max_pages).min(total_pages);
        let count = end - start;

        // Build each segment as a FRESH document, importing only this window's
        // pages and the objects they transitively reference. This avoids the
        // clone -> delete_pages -> prune_objects -> save approach, which on
        // real-world PDFs (xref-stream / PDF 1.6+) can emit a file lopdf then
        // refuses to re-load ("Invalid file trailer"). Building fresh and
        // remapping object ids is the same proven path `merge_pdfs` uses.
        let window: Vec<ObjectId> = ordered_pages[start..end]
            .iter()
            .map(|(_, id)| *id)
            .collect();

        let mut segment_doc = build_segment_document(&doc, &window)?;

        // Resolve inherited page-tree attributes onto each retained leaf and
        // confirm its referenced resources/content survived the import.
        normalize_pages(&mut segment_doc)?;

        let out_path = out_dir.join(format!("segment_{idx:03}.pdf"));
        segment_doc
            .save(&out_path)
            .map_err(|e| SplitMergeError::Save {
                path: out_path.clone(),
                source: lopdf::Error::IO(e),
            })?;

        segments.push(SplitSegment {
            path: out_path,
            page_offset: start,
            page_count: count,
        });
    }

    Ok(segments)
}

/// Build a fresh, self-contained `lopdf::Document` containing exactly the pages
/// in `window` (object ids in the source `doc`), in order, with a fresh
/// `Catalog` + flat `Pages` tree. All objects each page transitively
/// references are deep-copied and their ids remapped, so the result is a
/// standalone PDF that round-trips through `lopdf` cleanly.
fn build_segment_document(
    doc: &Document,
    window: &[ObjectId],
) -> Result<Document, SplitMergeError> {
    use std::collections::BTreeSet;

    let mut out = Document::with_version("1.7");

    let pages_id = out.new_object_id();
    let catalog_id = out.new_object_id();

    // Collect the transitive closure of objects reachable from each page in the
    // window (the page dict, its /Resources, /Contents, fonts, xobjects, ...),
    // plus document metadata that remains valid for every standalone segment.
    let mut needed: BTreeSet<ObjectId> = BTreeSet::new();
    for &page_id in window {
        collect_referenced(doc, page_id, &mut needed);
    }
    if let Ok(info) = doc.trailer.get(b"Info") {
        collect_from_object(doc, info, &mut needed);
    }
    if let Ok(catalog) = doc.catalog() {
        for key in CATALOG_METADATA_KEYS {
            if let Ok(value) = catalog.get(key) {
                collect_from_object(doc, value, &mut needed);
            }
        }
    }

    // Assign a fresh id in `out` for every needed source object.
    let mut id_map: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
    for &src_id in &needed {
        id_map.insert(src_id, out.new_object_id());
    }

    // Deep-copy each needed object with references remapped into `out`.
    for &src_id in &needed {
        if let Ok(obj) = doc.get_object(src_id) {
            let mut cloned = obj.clone();
            remap_references(&mut cloned, &id_map);
            let new_id = id_map[&src_id];
            out.objects.insert(new_id, cloned);
        }
    }

    // Build the flat Kids list and fix each page's /Parent.
    let mut kids: Vec<Object> = Vec::with_capacity(window.len());
    for &page_id in window {
        if let Some(&new_page_id) = id_map.get(&page_id) {
            if let Ok(page_dict) = out
                .get_object_mut(new_page_id)
                .and_then(|o| o.as_dict_mut())
            {
                page_dict.set("Parent", pages_id);
            }
            kids.push(Object::Reference(new_page_id));
        }
    }

    let kids_len = kids.len() as i64;
    out.objects.insert(
        pages_id,
        Object::Dictionary(dictionary!(
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => kids_len,
        )),
    );
    out.objects.insert(
        catalog_id,
        Object::Dictionary(dictionary!(
            "Type" => "Catalog",
            "Pages" => pages_id,
        )),
    );

    out.trailer.set("Root", catalog_id);
    if let Ok(info) = doc.trailer.get(b"Info") {
        let mut mapped = info.clone();
        remap_references(&mut mapped, &id_map);
        if !matches!(mapped, Object::Null) {
            out.trailer.set("Info", mapped);
        }
    }
    if let Ok(source_catalog) = doc.catalog() {
        let output_catalog = out.get_dictionary_mut(catalog_id).map_err(|error| {
            SplitMergeError::Structure(format!("new segment catalog is not a dictionary: {error}"))
        })?;
        for key in CATALOG_METADATA_KEYS {
            if let Ok(value) = source_catalog.get(key) {
                let mut mapped = value.clone();
                remap_references(&mut mapped, &id_map);
                if !matches!(mapped, Object::Null) {
                    output_catalog.set(key, mapped);
                }
            }
        }
    }
    out.max_id = out.objects.keys().map(|(n, _)| *n).max().unwrap_or(0);

    Ok(out)
}

/// Transitively collect every object id reachable from `start` (excluding
/// `/Parent` back-references, which would drag in the whole original tree).
fn collect_referenced(
    doc: &Document,
    start: ObjectId,
    acc: &mut std::collections::BTreeSet<ObjectId>,
) {
    if !acc.insert(start) {
        return; // already visited
    }
    if let Ok(obj) = doc.get_object(start) {
        collect_from_object(doc, obj, acc);
    }
}

fn collect_from_object(
    doc: &Document,
    obj: &Object,
    acc: &mut std::collections::BTreeSet<ObjectId>,
) {
    match obj {
        Object::Reference(id) => collect_referenced(doc, *id, acc),
        Object::Array(arr) => {
            for v in arr {
                collect_from_object(doc, v, acc);
            }
        }
        Object::Dictionary(dict) => {
            for (key, v) in dict.iter() {
                // Skip /Parent so we don't pull in ancestor /Pages nodes and
                // the rest of the document tree.
                if key == b"Parent" {
                    continue;
                }
                collect_from_object(doc, v, acc);
            }
        }
        Object::Stream(stream) => {
            for (key, v) in stream.dict.iter() {
                if key == b"Parent" {
                    continue;
                }
                collect_from_object(doc, v, acc);
            }
        }
        _ => {}
    }
}

/// Remap every `Reference` inside `obj` according to `id_map`. References to
/// objects not in the map (e.g. a skipped /Parent) are dropped to null so the
/// output never dangles.
fn remap_references(obj: &mut Object, id_map: &BTreeMap<ObjectId, ObjectId>) {
    match obj {
        Object::Reference(id) => {
            if let Some(&new_id) = id_map.get(id) {
                *id = new_id;
            } else {
                *obj = Object::Null;
            }
        }
        Object::Array(arr) => {
            for v in arr.iter_mut() {
                remap_references(v, id_map);
            }
        }
        Object::Dictionary(dict) => {
            for (_k, v) in dict.iter_mut() {
                remap_references(v, id_map);
            }
        }
        Object::Stream(stream) => {
            for (_k, v) in stream.dict.iter_mut() {
                remap_references(v, id_map);
            }
        }
        _ => {}
    }
}

/// Page-tree attributes that a leaf `/Page` may inherit from an ancestor
/// `/Pages` node per the PDF spec. After a split the surviving page tree may
/// keep these on an intermediate node, but the merge step rebuilds a flat
/// tree, so each value is materialized directly onto the leaf here.
const INHERITABLE_ATTRS: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Rotate", b"Resources"];

/// Document-level catalog entries that are safe and meaningful on every
/// standalone segment and merged result. Page-targeting actions, outlines, and
/// destinations are intentionally excluded because their page references must
/// be rebuilt rather than copied blindly.
const CATALOG_METADATA_KEYS: [&[u8]; 5] = [
    b"Metadata",
    b"Lang",
    b"ViewerPreferences",
    b"PageMode",
    b"PageLayout",
];

/// Attributes whose presence on every retained leaf is mandatory for faithful
/// rendering of a standalone segment. `/MediaBox` is required by the spec;
/// `/Rotate` defaults to 0 when absent and `/CropBox` defaults to `/MediaBox`,
/// so those are materialized explicitly only when an inherited value exists.
fn normalize_pages(doc: &mut Document) -> Result<(), SplitMergeError> {
    // Snapshot the leaf page ids so we can mutate the document while iterating.
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();

    for page_id in page_ids {
        // 1) Resolve inherited `/Pages`-node attributes by walking the
        //    `/Parent` chain, then set any that the leaf lacks directly on it.
        let resolved = resolve_inherited_attrs(doc, page_id)?;
        let page_dict = doc.get_dictionary_mut(page_id).map_err(|e| {
            SplitMergeError::Structure(format!("page {page_id:?} is not a dictionary: {e}"))
        })?;

        for (key, value) in resolved {
            if !page_dict.has(&key) {
                page_dict.set(key, value);
            }
        }

        // 2) Explicitly materialize `/MediaBox`, `/CropBox`, and `/Rotate`.
        //    `/MediaBox` must exist on a standalone page; if neither the leaf
        //    nor any ancestor provided one, fall back to US Letter so the
        //    segment still renders rather than failing to open.
        let page_dict = doc.get_dictionary_mut(page_id).map_err(|e| {
            SplitMergeError::Structure(format!("page {page_id:?} is not a dictionary: {e}"))
        })?;

        if !page_dict.has(b"MediaBox") {
            page_dict.set(
                "MediaBox",
                vec![
                    Object::Real(0.0),
                    Object::Real(0.0),
                    Object::Real(612.0),
                    Object::Real(792.0),
                ],
            );
        }
        // `/CropBox` defaults to `/MediaBox`; materialize it so the standalone
        // page is unambiguous regardless of what the original tree relied on.
        if !page_dict.has(b"CropBox") {
            if let Ok(media_box) = page_dict.get(b"MediaBox") {
                let media_box = media_box.clone();
                page_dict.set("CropBox", media_box);
            }
        }
        // `/Rotate` defaults to 0 when absent; materialize the default so page
        // orientation is preserved explicitly.
        if !page_dict.has(b"Rotate") {
            page_dict.set("Rotate", Object::Integer(0));
        }

        // 3) Confirm the page's referenced resources and content streams
        //    survived `prune_objects`. A dangling reference here means a glyph
        //    program, image, or content stream was dropped and the segment
        //    would render incorrectly, so surface it as a structural error.
        confirm_resources_present(doc, page_id)?;
    }

    Ok(())
}

/// Walk the `/Parent` chain from a leaf page collecting any inheritable
/// attribute (`/MediaBox`, `/CropBox`, `/Rotate`, `/Resources`) that is set on
/// an ancestor `/Pages` node. The nearest ancestor wins, and an attribute
/// already present on the leaf is not collected (the caller only sets missing
/// ones). Cycles in the `/Parent` chain are guarded against.
fn resolve_inherited_attrs(
    doc: &Document,
    page_id: ObjectId,
) -> Result<Vec<(Vec<u8>, Object)>, SplitMergeError> {
    let mut collected: BTreeMap<Vec<u8>, Object> = BTreeMap::new();

    let leaf = doc.get_dictionary(page_id).map_err(|e| {
        SplitMergeError::Structure(format!("page {page_id:?} is not a dictionary: {e}"))
    })?;

    // Start the walk at the leaf's parent; attributes on the leaf itself are
    // already authoritative and handled by the caller.
    let mut parent_ref = leaf.get(b"Parent").and_then(Object::as_reference).ok();
    let mut seen: HashSet<ObjectId> = HashSet::new();

    while let Some(parent_id) = parent_ref {
        if !seen.insert(parent_id) {
            return Err(SplitMergeError::Structure(format!(
                "cycle in /Parent chain at {parent_id:?}"
            )));
        }

        let node = match doc.get_dictionary(parent_id) {
            Ok(node) => node,
            // A missing parent node is not fatal: a leaf with no inheritable
            // ancestor simply has nothing to collect.
            Err(_) => break,
        };

        for key in INHERITABLE_ATTRS {
            if !collected.contains_key(key) {
                if let Ok(value) = node.get(key) {
                    // Resolve a referenced value to a concrete object where
                    // possible so the flattened leaf is self-contained.
                    collected.insert(key.to_vec(), value.clone());
                }
            }
        }

        parent_ref = node.get(b"Parent").and_then(Object::as_reference).ok();
    }

    Ok(collected.into_iter().collect())
}

/// Confirm every object a page references for rendering survived the prune:
/// its `/Resources` (and the `/Font`, `/XObject`, `/ExtGState` sub-resources
/// within) plus its content streams. Returns a structural error on the first
/// dangling reference so a corrupt segment is never written.
fn confirm_resources_present(doc: &Document, page_id: ObjectId) -> Result<(), SplitMergeError> {
    // Content streams.
    for content_id in doc.get_page_contents(page_id) {
        if !doc.has_object(content_id) {
            return Err(SplitMergeError::Structure(format!(
                "page {page_id:?} references missing content stream {content_id:?}"
            )));
        }
    }

    // Resources dictionary plus its sub-resource categories. The leaf has been
    // normalized to carry `/Resources` directly when it was inherited, but it
    // may still be a reference, so resolve it explicitly.
    let page_dict = doc.get_dictionary(page_id).map_err(|e| {
        SplitMergeError::Structure(format!("page {page_id:?} is not a dictionary: {e}"))
    })?;

    let resources_dict = match page_dict.get(b"Resources") {
        Ok(Object::Reference(id)) => {
            if !doc.has_object(*id) {
                return Err(SplitMergeError::Structure(format!(
                    "page {page_id:?} references missing /Resources {id:?}"
                )));
            }
            doc.get_dictionary(*id).ok()
        }
        Ok(Object::Dictionary(dict)) => Some(dict),
        // No `/Resources` at all is legal for a page with no marked content.
        _ => None,
    };

    if let Some(resources) = resources_dict {
        for category in [b"Font".as_slice(), b"XObject", b"ExtGState"] {
            confirm_category_present(doc, page_id, resources, category)?;
        }
    }

    Ok(())
}

/// Confirm every entry in one resource sub-dictionary (`/Font`, `/XObject`,
/// `/ExtGState`) resolves to an object still present in the document.
fn confirm_category_present(
    doc: &Document,
    page_id: ObjectId,
    resources: &Dictionary,
    category: &[u8],
) -> Result<(), SplitMergeError> {
    let category_dict = match resources.get(category) {
        Ok(Object::Reference(id)) => {
            if !doc.has_object(*id) {
                return Err(SplitMergeError::Structure(format!(
                    "page {page_id:?} references missing {} dictionary {id:?}",
                    String::from_utf8_lossy(category)
                )));
            }
            doc.get_dictionary(*id).ok()
        }
        Ok(Object::Dictionary(dict)) => Some(dict),
        _ => None,
    };

    if let Some(category_dict) = category_dict {
        for (_name, value) in category_dict.iter() {
            if let Object::Reference(id) = value {
                if !doc.has_object(*id) {
                    return Err(SplitMergeError::Structure(format!(
                        "page {page_id:?} references missing {} resource {id:?}",
                        String::from_utf8_lossy(category)
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Resolve a single page's effective `/MediaBox`, `/CropBox`, and `/Rotate`
/// for the merge step's flat page tree. Each attribute is taken from the leaf
/// page itself when present, otherwise from the nearest ancestor `/Pages` node
/// via the inherited-attribute walk (which guards against `/Parent` cycles).
/// Returned values reference objects in the SOURCE document, so the caller
/// renumbers them through its id remap before applying them to the new leaf.
///
/// Only the geometry attributes are returned here; `/Resources` and all glyph,
/// image, and graphics-state objects are carried over wholesale by the
/// merge step's object-by-object id remap, so they are intentionally excluded.
fn resolve_page_geometry(
    doc: &Document,
    page_id: ObjectId,
) -> Result<Vec<(Vec<u8>, Object)>, SplitMergeError> {
    let leaf = doc.get_dictionary(page_id).map_err(|e| {
        SplitMergeError::Structure(format!("page {page_id:?} is not a dictionary: {e}"))
    })?;

    // Values inherited from ancestor `/Pages` nodes; the leaf's own entries
    // take precedence over anything found on the way up the `/Parent` chain.
    let inherited = resolve_inherited_attrs(doc, page_id)?;
    let mut inherited_map: BTreeMap<&[u8], Object> = BTreeMap::new();
    for (key, value) in &inherited {
        inherited_map.insert(key.as_slice(), value.clone());
    }

    let mut geometry: Vec<(Vec<u8>, Object)> = Vec::new();
    for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate"] {
        if let Ok(value) = leaf.get(key) {
            geometry.push((key.to_vec(), value.clone()));
        } else if let Some(value) = inherited_map.get(key) {
            geometry.push((key.to_vec(), value.clone()));
        }
    }

    Ok(geometry)
}

/// Concatenate `ordered_paths` (already in global page order) into a single
/// `output_path`, using lopdf only. Returns the merged page count.
///
/// The merged document keeps a single coherent `Catalog` + flat `Pages` tree:
/// every object each segment contains is renumbered through a per-segment id
/// remap and copied in, so each page's `/Resources` (and the `/Font`,
/// `/XObject`, and `/ExtGState` sub-resources it references) is carried over -
/// no glyph programs or images are dropped. For each appended page the flat
/// tree discards the segment's intermediate `/Pages` nodes, so `/MediaBox`,
/// `/CropBox`, and `/Rotate` are explicitly copied onto the new leaf (with a
/// US Letter `/MediaBox` fallback if a segment somehow omits it), and the
/// page's resource references are confirmed to resolve in the merged document
/// before it is appended. Pages keep the same 0-based global index they held
/// in source order. Returns the merged page count so the caller can assert it
/// against `total_pages`.
pub fn merge_pdfs(ordered_paths: &[PathBuf], output_path: &Path) -> Result<usize, SplitMergeError> {
    if ordered_paths.is_empty() {
        return Err(SplitMergeError::Structure(
            "merge requires at least one input segment".into(),
        ));
    }
    let mut merged_doc = Document::with_version("1.7");
    let mut total_pages = 0usize;

    // Initialize the merged document with a proper Catalog and a single, empty
    // flat Pages tree that every appended page is reparented onto.
    let pages_id = merged_doc.add_object(Object::Dictionary(dictionary!(
        "Type" => "Pages",
        "Count" => 0,
        "Kids" => vec![],
    )));

    let catalog_id = merged_doc.add_object(Object::Dictionary(dictionary!(
        "Type" => "Catalog",
        "Pages" => pages_id,
    )));

    merged_doc.trailer.set("Root", catalog_id);

    let mut next_object_id = merged_doc.max_id + 1;
    let mut document_metadata_copied = false;

    for path in ordered_paths {
        let mut doc = Document::load(path).map_err(|e| SplitMergeError::Load {
            path: path.clone(),
            source: e,
        })?;
        doc.decompress();

        let page_map = doc.get_pages();
        let page_count = page_map.len();

        // Map old IDs to new IDs. Every object the segment contains is copied
        // and renumbered below, so each page's `/Resources` (and the `/Font`,
        // `/XObject`, and `/ExtGState` sub-resources it references) is carried
        // through this remap - no glyph programs or images are dropped.
        let mut id_map: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
        for &id in doc.objects.keys() {
            id_map.insert(id, (next_object_id, 0));
            next_object_id += 1;
        }

        let document_metadata = if document_metadata_copied {
            None
        } else {
            let mut info = doc.trailer.get(b"Info").ok().cloned();
            if let Some(value) = info.as_mut() {
                renumber_object(value, &id_map);
            }
            let mut catalog_entries = Vec::new();
            if let Ok(catalog) = doc.catalog() {
                for key in CATALOG_METADATA_KEYS {
                    if let Ok(value) = catalog.get(key) {
                        let mut mapped = value.clone();
                        renumber_object(&mut mapped, &id_map);
                        if !matches!(mapped, Object::Null) {
                            catalog_entries.push((key.to_vec(), mapped));
                        }
                    }
                }
            }
            Some((info, catalog_entries))
        };

        // Resolve each page's effective `/MediaBox`, `/CropBox`, and `/Rotate`
        // BEFORE `doc.objects` is moved below. The merged document uses a flat
        // page tree that discards the segment's intermediate `/Pages` nodes, so
        // any geometry inherited from an ancestor would be lost; capture it
        // here (leaf value first, else walk the `/Parent` chain) keyed by the
        // page's NEW id and materialize it onto the leaf after insertion. Any
        // referenced value is renumbered so it points at the copied object.
        // The page order is captured here too so pages are appended in source
        // order and land at the same 0-based global index they held on input.
        let mut page_geometry: BTreeMap<ObjectId, Vec<(Vec<u8>, Object)>> = BTreeMap::new();
        let mut ordered_new_page_ids: Vec<ObjectId> = Vec::with_capacity(page_count);
        for page_num in 1..=page_count as u32 {
            // `get_pages` is contiguous 1..=page_count; a missing entry means a
            // malformed page tree, so surface it as a structural error rather
            // than panicking.
            let old_page_id = *page_map.get(&page_num).ok_or_else(|| {
                SplitMergeError::Structure(format!(
                    "segment {} is missing page {page_num} during merge",
                    path.display()
                ))
            })?;
            let new_page_id = *id_map.get(&old_page_id).ok_or_else(|| {
                SplitMergeError::Structure(format!(
                    "segment {} page {page_num} object {old_page_id:?} was not copied",
                    path.display()
                ))
            })?;

            let mut geometry = resolve_page_geometry(&doc, old_page_id)?;
            for (_key, value) in geometry.iter_mut() {
                renumber_object(value, &id_map);
            }
            page_geometry.insert(new_page_id, geometry);
            ordered_new_page_ids.push(new_page_id);
        }

        // Copy renumbered objects to merged_doc.
        for (id, mut object) in doc.objects {
            renumber_object(&mut object, &id_map);
            let new_id = *id_map.get(&id).ok_or_else(|| {
                SplitMergeError::Structure(format!(
                    "segment {} object {id:?} missing from id remap during merge",
                    path.display()
                ))
            })?;
            merged_doc.objects.insert(new_id, object);
        }

        if let Some((info, catalog_entries)) = document_metadata {
            if let Some(info) = info {
                if !matches!(info, Object::Null) {
                    merged_doc.trailer.set("Info", info);
                }
            }
            let catalog = merged_doc.get_dictionary_mut(catalog_id).map_err(|error| {
                SplitMergeError::Structure(format!("merged catalog is not a dictionary: {error}"))
            })?;
            for (key, value) in catalog_entries {
                catalog.set(key, value);
            }
            document_metadata_copied = true;
        }

        // Append pages to the flat Pages tree in source order.
        for new_page_id in ordered_new_page_ids {
            // 1) Reparent onto the flat tree and explicitly copy the page's
            //    resolved `/MediaBox`, `/CropBox`, and `/Rotate` so its size and
            //    orientation survive the discarded intermediate `/Pages` nodes.
            //    A missing `/MediaBox` (shouldn't occur after split
            //    normalization, but be defensive) falls back to US Letter so
            //    the merged page is still sized rather than unrenderable.
            {
                let page_dict = merged_doc
                    .get_object_mut(new_page_id)
                    .and_then(|obj| obj.as_dict_mut())
                    .map_err(|e| {
                        SplitMergeError::Structure(format!(
                            "merged page {new_page_id:?} is not a dictionary: {e}"
                        ))
                    })?;

                page_dict.set("Parent", pages_id);

                if let Some(geometry) = page_geometry.get(&new_page_id) {
                    for (key, value) in geometry {
                        page_dict.set(key.clone(), value.clone());
                    }
                }
                materialize_geometry_defaults(page_dict);
            }

            // 2) Confirm the page's `/Resources` (and the `/Font`, `/XObject`,
            //    `/ExtGState` entries within) plus its content streams resolve
            //    in the merged document, so no glyph program or image was
            //    silently dropped by the structural copy.
            confirm_resources_present(&merged_doc, new_page_id)?;

            // 3) Append the page reference to the flat Pages tree and bump its
            //    Count.
            append_page_to_tree(&mut merged_doc, pages_id, new_page_id)?;

            total_pages += 1;
        }
    }

    merged_doc.max_id = next_object_id - 1;

    let output_parent = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(output_parent)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".dcpp-merge-")
        .suffix(".pdf")
        .tempfile_in(output_parent)?;
    merged_doc
        .save_to(&mut staged)
        .map_err(|source| SplitMergeError::Save {
            path: output_path.to_path_buf(),
            source: lopdf::Error::IO(source),
        })?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    let staged_path = staged.into_temp_path();
    let staged_path_ref: &Path = staged_path.as_ref();

    let validated = Document::load(staged_path_ref).map_err(|source| SplitMergeError::Load {
        path: staged_path.to_path_buf(),
        source,
    })?;
    let validated_pages = validated.get_pages().len();
    if validated_pages != total_pages {
        return Err(SplitMergeError::Structure(format!(
            "staged merge page count mismatch: expected {total_pages}, got {validated_pages}"
        )));
    }

    staged_path
        .persist(output_path)
        .map_err(|error| SplitMergeError::Io(error.error))?;
    Ok(total_pages)
}

/// Ensure a merged leaf page carries explicit geometry. `/MediaBox` is required
/// for a page to be sized; when neither the source leaf nor an ancestor
/// `/Pages` node provided one (it shouldn't after split normalization, but a
/// segment produced elsewhere might omit it) fall back to US Letter so the
/// merged page still renders. `/CropBox` defaults to `/MediaBox` and `/Rotate`
/// defaults to 0 when absent, materialized here so the flat-tree leaf is
/// self-contained.
fn materialize_geometry_defaults(page_dict: &mut Dictionary) {
    if !page_dict.has(b"MediaBox") {
        page_dict.set(
            "MediaBox",
            vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(612.0),
                Object::Real(792.0),
            ],
        );
    }
    if !page_dict.has(b"CropBox") {
        if let Ok(media_box) = page_dict.get(b"MediaBox") {
            let media_box = media_box.clone();
            page_dict.set("CropBox", media_box);
        }
    }
    if !page_dict.has(b"Rotate") {
        page_dict.set("Rotate", Object::Integer(0));
    }
}

/// Append `page_id` to the flat `/Pages` tree's `/Kids` array and increment its
/// `/Count`. The tree was created by `merge_pdfs` with both a `/Kids` array and
/// an integer `/Count`, but this returns a structural error rather than
/// panicking if it is ever malformed.
fn append_page_to_tree(
    doc: &mut Document,
    pages_id: ObjectId,
    page_id: ObjectId,
) -> Result<(), SplitMergeError> {
    let pages_root = doc
        .get_object_mut(pages_id)
        .and_then(|obj| obj.as_dict_mut())
        .map_err(|e| {
            SplitMergeError::Structure(format!(
                "merged /Pages tree {pages_id:?} is not a dictionary: {e}"
            ))
        })?;

    let kids = pages_root
        .get_mut(b"Kids")
        .and_then(|obj| obj.as_array_mut())
        .map_err(|e| {
            SplitMergeError::Structure(format!("merged /Pages tree is missing a /Kids array: {e}"))
        })?;
    kids.push(Object::Reference(page_id));

    match pages_root.get_mut(b"Count") {
        Ok(Object::Integer(count)) => *count += 1,
        Ok(_) => {
            return Err(SplitMergeError::Structure(
                "merged /Pages tree /Count is not an integer".to_string(),
            ));
        }
        Err(e) => {
            return Err(SplitMergeError::Structure(format!(
                "merged /Pages tree is missing a /Count: {e}"
            )));
        }
    }

    Ok(())
}

fn renumber_object(object: &mut Object, id_map: &BTreeMap<ObjectId, ObjectId>) {
    match object {
        Object::Reference(ref mut id) => {
            if let Some(&new_id) = id_map.get(id) {
                *id = new_id;
            }
        }
        Object::Dictionary(ref mut dict) => {
            for (_, value) in dict.iter_mut() {
                renumber_object(value, id_map);
            }
        }
        Object::Array(ref mut array) => {
            for value in array.iter_mut() {
                renumber_object(value, id_map);
            }
        }
        Object::Stream(ref mut stream) => {
            for (_, value) in stream.dict.iter_mut() {
                renumber_object(value, id_map);
            }
        }
        _ => {}
    }
}
