use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A single edit requested by the user in global document coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalEdit {
    pub page: usize,
    pub bbox: [f32; 4],
    pub old_text: String,
    pub new_text: String,
    pub description: String,
    pub deep_font_replication: bool,
}

/// An edit targeted at a specific segment file.
#[derive(Debug, Clone)]
pub struct LocalEdit {
    pub local_page: usize,
    pub bbox: [f32; 4],
    pub old_text: String,
    pub new_text: String,
    pub description: String,
    pub deep_font_replication: bool,
}

/// Error returned when grouping global-page edits into per-segment buckets.
///
/// Per Requirement 8.5, an edit whose `global_page` is out of range must abort
/// the operation (identifying the offending page) rather than be silently
/// dropped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GroupEditsError {
    #[error("edit references global page {global_page} which is out of range (total_pages = {total_pages})")]
    OutOfRange {
        global_page: usize,
        total_pages: usize,
    },
}

/// Metadata for a single PDF segment (at most `max_pages_per_segment` pages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentInfo {
    /// 0-based segment ordinal (matches its position in `SegmentMap.segments`).
    pub index: usize,
    /// Path to the original `segment_NNN.pdf` produced by the split engine.
    pub path: PathBuf,
    /// Global index of this segment's first page.
    pub page_offset: usize,
    /// Number of pages in this segment, in the range 1..=3.
    pub page_count: usize,
    /// Set true once an edit has been applied to this segment.
    pub edited: bool,
    /// Output of `apply_many_edits` for this segment, if it was edited.
    pub edited_path: Option<PathBuf>,
}

/// The mapping between a global multi-page document and its local segments.
///
/// Immutable description of how a document was split. All translation between
/// global page numbers and `(segment index, local page)` pairs goes through here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentMap {
    /// Path to the original (pre-split) source document.
    pub original_path: PathBuf,
    /// Total page count == sum(seg.page_count); preserved on merge.
    pub total_pages: usize,
    /// Maximum pages per segment (== 3 for the PyMuPDF Pro limit).
    pub max_pages_per_segment: usize,
    /// Ordered segment metadata (ascending by `index`/`page_offset`).
    pub segments: Vec<SegmentInfo>,
    /// Temp directory holding the segment files for this document.
    pub temp_dir: PathBuf,
}

impl SegmentMap {
    /// Create a new map from a list of segments and the document-level metadata.
    pub fn new(
        segments: Vec<SegmentInfo>,
        original_path: PathBuf,
        temp_dir: PathBuf,
        max_pages_per_segment: usize,
    ) -> Self {
        let total_pages = segments.iter().map(|s| s.page_count).sum();
        Self {
            original_path,
            total_pages,
            max_pages_per_segment,
            segments,
            temp_dir,
        }
    }

    /// Validate that segments form one exact, contiguous, ordered partition of
    /// the original document and that edited-state metadata is internally
    /// consistent. This must pass before any segmented mutation or merge.
    pub fn validate_structure(&self) -> Result<(), String> {
        if self.max_pages_per_segment == 0 {
            return Err("max_pages_per_segment must be greater than zero".into());
        }
        if self.segments.is_empty() {
            return Err("segment map must contain at least one segment".into());
        }

        let mut expected_offset = 0usize;
        for (position, segment) in self.segments.iter().enumerate() {
            if segment.index != position {
                return Err(format!(
                    "segment position {position} carries non-canonical index {}",
                    segment.index
                ));
            }
            if segment.page_count == 0 || segment.page_count > self.max_pages_per_segment {
                return Err(format!(
                    "segment {position} page_count {} is outside 1..={}",
                    segment.page_count, self.max_pages_per_segment
                ));
            }
            if segment.page_offset != expected_offset {
                return Err(format!(
                    "segment {position} starts at page {}, expected {expected_offset}",
                    segment.page_offset
                ));
            }
            if segment.edited != segment.edited_path.is_some() {
                return Err(format!(
                    "segment {position} edited flag and edited_path disagree"
                ));
            }
            expected_offset = expected_offset
                .checked_add(segment.page_count)
                .ok_or_else(|| "segment page offsets overflowed usize".to_string())?;
        }
        if expected_offset != self.total_pages {
            return Err(format!(
                "segment pages total {expected_offset}, map reports {}",
                self.total_pages
            ));
        }
        Ok(())
    }

    /// Resolve a global page index to a `(segment_index, local_page)` pair.
    ///
    /// Returns `None` (without mutating state) when `global_page >= total_pages`
    /// (Requirement 6.5).
    pub fn resolve(&self, global_page: usize) -> Option<(usize, usize)> {
        if global_page >= self.total_pages {
            return None;
        }
        for (i, seg) in self.segments.iter().enumerate() {
            if global_page >= seg.page_offset && global_page < seg.page_offset + seg.page_count {
                return Some((i, global_page - seg.page_offset));
            }
        }
        None
    }

    /// Resolve a global page to the segment file currently backing it and its
    /// local page within that segment (Requirement 7.1).
    ///
    /// Returns the segment's current merge-input path - its `edited_path` when
    /// an edit has been applied, otherwise the original segment `path` - paired
    /// with the local page. Returns `None` (without mutating state) when
    /// `global_page` has no mapping (`global_page >= total_pages`), so the
    /// render path can abort cleanly (Requirement 7.4). Pure: never mutates.
    pub fn locate(&self, global_page: usize) -> Option<(PathBuf, usize)> {
        let (seg_idx, local_page) = self.resolve(global_page)?;
        let seg = &self.segments[seg_idx];
        let path = seg.edited_path.clone().unwrap_or_else(|| seg.path.clone());
        Some((path, local_page))
    }

    /// Convert a `(segment_index, local_page)` pair back to a global page index.
    ///
    /// Returns `None` (without mutating state) when `segment_index >=
    /// segments.len()` or `local_page >= segment.page_count` (Requirement 6.6).
    pub fn to_global(&self, segment_index: usize, local_page: usize) -> Option<usize> {
        let seg = self.segments.get(segment_index)?;
        if local_page < seg.page_count {
            Some(seg.page_offset + local_page)
        } else {
            None
        }
    }

    /// Group global-page edits into per-segment buckets keyed by segment index,
    /// with each edit translated to its local page within the segment.
    ///
    /// Errors (identifying the offending `global_page`) when any edit's
    /// `global_page >= total_pages`, instead of silently dropping it
    /// (Requirement 8.5). Buckets are keyed and ordered by segment index.
    pub fn group_edits_by_segment(
        &self,
        edits: &[GlobalEdit],
    ) -> Result<BTreeMap<usize, Vec<LocalEdit>>, GroupEditsError> {
        let mut groups: BTreeMap<usize, Vec<LocalEdit>> = BTreeMap::new();

        for edit in edits {
            let (seg_idx, local_page) =
                self.resolve(edit.page).ok_or(GroupEditsError::OutOfRange {
                    global_page: edit.page,
                    total_pages: self.total_pages,
                })?;
            groups.entry(seg_idx).or_default().push(LocalEdit {
                local_page,
                bbox: edit.bbox,
                old_text: edit.old_text.clone(),
                new_text: edit.new_text.clone(),
                description: edit.description.clone(),
                deep_font_replication: edit.deep_font_replication,
            });
        }

        Ok(groups)
    }

    /// Ordered list of paths to feed merge: `edited_path` where present, else
    /// the original segment path, in ascending segment-index order.
    pub fn ordered_merge_paths(&self) -> Vec<PathBuf> {
        self.segments
            .iter()
            .map(|s| s.edited_path.clone().unwrap_or_else(|| s.path.clone()))
            .collect()
    }
}

/// Summary report after merging segments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeReport {
    /// Path to the final merged output PDF.
    pub final_path: PathBuf,
    /// Page count of the merged output (must equal `SegmentMap.total_pages`).
    pub merged_pages: usize,
    /// Number of segments that had at least one edit applied.
    pub segments_edited: usize,
    /// Non-fatal warnings recorded during apply/merge (e.g. cleanup or
    /// resource-integrity warnings).
    pub warnings: Vec<String>,
    /// Global pages flagged for visual review (fidelity could not be fully
    /// reproduced).
    pub review_flags: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
struct PageGeometry {
    media_box: [f32; 4],
    crop_box: [f32; 4],
    rotation: i64,
}

fn page_box(
    document: &lopdf::Document,
    page_id: lopdf::ObjectId,
    key: &[u8],
) -> Result<Option<[f32; 4]>, crate::engine::pdf_split_merge::SplitMergeError> {
    let page = document.get_dictionary(page_id).map_err(|error| {
        crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
            "page {page_id:?} is not a dictionary: {error}"
        ))
    })?;
    let object = match page.get(key) {
        Ok(object) => object,
        Err(_) => return Ok(None),
    };
    let (_, object) = document.dereference(object).map_err(|error| {
        crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
            "page {page_id:?} {} cannot be dereferenced: {error}",
            String::from_utf8_lossy(key)
        ))
    })?;
    let values = object.as_array().map_err(|error| {
        crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
            "page {page_id:?} {} is not an array: {error}",
            String::from_utf8_lossy(key)
        ))
    })?;
    if values.len() != 4 {
        return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
            format!(
                "page {page_id:?} {} has {} values, expected four",
                String::from_utf8_lossy(key),
                values.len()
            ),
        ));
    }
    let mut result = [0.0f32; 4];
    for (index, value) in values.iter().enumerate() {
        result[index] = value.as_float().map_err(|error| {
            crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
                "page {page_id:?} {} value {index} is not numeric: {error}",
                String::from_utf8_lossy(key)
            ))
        })?;
    }
    Ok(Some(result))
}

fn page_geometry(
    document: &lopdf::Document,
    page_id: lopdf::ObjectId,
) -> Result<PageGeometry, crate::engine::pdf_split_merge::SplitMergeError> {
    let media_box = page_box(document, page_id, b"MediaBox")?.ok_or_else(|| {
        crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
            "page {page_id:?} has no MediaBox"
        ))
    })?;
    let crop_box = page_box(document, page_id, b"CropBox")?.unwrap_or(media_box);
    let page = document.get_dictionary(page_id).map_err(|error| {
        crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
            "page {page_id:?} is not a dictionary: {error}"
        ))
    })?;
    let rotation = match page.get(b"Rotate") {
        Ok(object) => {
            let (_, object) = document.dereference(object).map_err(|error| {
                crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
                    "page {page_id:?} Rotate cannot be dereferenced: {error}"
                ))
            })?;
            object.as_i64().map_err(|error| {
                crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
                    "page {page_id:?} Rotate is not an integer: {error}"
                ))
            })?
        }
        Err(_) => 0,
    };
    Ok(PageGeometry {
        media_box,
        crop_box,
        rotation: rotation.rem_euclid(360),
    })
}

fn document_geometry(
    path: &Path,
) -> Result<Vec<PageGeometry>, crate::engine::pdf_split_merge::SplitMergeError> {
    let document = lopdf::Document::load(path).map_err(|source| {
        crate::engine::pdf_split_merge::SplitMergeError::Load {
            path: path.to_path_buf(),
            source,
        }
    })?;
    document
        .get_pages()
        .values()
        .map(|page_id| page_geometry(&document, *page_id))
        .collect()
}

/// Verify that an edited segment retains the exact page membership and visible
/// page geometry of its source. Text/content changes are allowed; page count,
/// MediaBox, CropBox, and rotation changes are not.
pub fn validate_segment_replacement(
    source: &Path,
    edited: &Path,
    expected_pages: usize,
) -> Result<(), crate::engine::pdf_split_merge::SplitMergeError> {
    let source_geometry = document_geometry(source)?;
    if source_geometry.len() != expected_pages {
        return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
            format!(
                "source segment has {} pages, expected {expected_pages}",
                source_geometry.len()
            ),
        ));
    }
    let edited_geometry = document_geometry(edited)?;
    if edited_geometry.len() != expected_pages {
        return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
            format!(
                "edited segment has {} pages, expected {expected_pages}",
                edited_geometry.len()
            ),
        ));
    }
    if edited_geometry != source_geometry {
        return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
            "edited segment page boxes or rotations differ from the source".into(),
        ));
    }
    Ok(())
}

/// Orchestrates the split-process-merge lifecycle for large documents.
pub struct SegmentManager {
    pub temp_dir: tempfile::TempDir,
}

impl SegmentManager {
    pub fn new() -> Result<Self, std::io::Error> {
        let temp_dir = tempfile::Builder::new().prefix("bank-stmt-").tempdir()?;
        Ok(Self { temp_dir })
    }

    pub fn temp_path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    /// Split a document into segments and return the map.
    ///
    /// On success builds the `SegmentMap` (populating `original_path`,
    /// `temp_dir`, `max_pages_per_segment`, and per-segment `index`/`edited`/
    /// `edited_path`) whose `total_pages` equals the original page count
    /// (Requirement 2.6). Splitting uses only the pure-Rust `lopdf` module
    /// (`pdf_split_merge::split_pdf`) - no Python worker or PyMuPDF is involved.
    ///
    /// On split failure (Requirement 12.1): abort preparation and build no map,
    /// best-effort remove any partial `segment_*.pdf` files written into the
    /// per-document temp dir plus the temp dir itself (Requirement 12.2), and
    /// surface the `lopdf` error (which already carries the offending path). The
    /// original source file is never written by `split_pdf` (it only reads
    /// `src_path` and writes into the temp `out_dir`), so it is left unmodified
    /// at its original path (Requirement 12.3).
    pub fn prepare(
        &self,
        src_path: &Path,
        max_pages: usize,
    ) -> Result<SegmentMap, crate::engine::pdf_split_merge::SplitMergeError> {
        let segments = match crate::engine::pdf_split_merge::split_pdf(
            src_path,
            self.temp_path(),
            max_pages,
        ) {
            Ok(segments) => segments,
            Err(e) => {
                // Proactively remove partial segment files and the temp dir so
                // sensitive statement content is not left on disk and no
                // partial state survives a failed preparation (Req 12.2). The
                // owned `TempDir` is still cleaned on drop, but `prepare` only
                // borrows `self`, so clear its contents best-effort here.
                self.cleanup_temp_contents();
                return Err(e);
            }
        };
        let infos = segments
            .into_iter()
            .enumerate()
            .map(|(i, s)| SegmentInfo {
                index: i,
                path: s.path,
                page_offset: s.page_offset,
                page_count: s.page_count,
                edited: false,
                edited_path: None,
            })
            .collect();
        let map = SegmentMap::new(
            infos,
            src_path.to_path_buf(),
            self.temp_path().to_path_buf(),
            max_pages,
        );
        map.validate_structure()
            .map_err(crate::engine::pdf_split_merge::SplitMergeError::Structure)?;
        Ok(map)
    }

    /// Resolve a global page to its segment file + local page through the map
    /// for the render/edit path (Requirement 7.1). Thin accessor over
    /// [`SegmentMap::locate`]; the manager does not own the map (the runtime
    /// holds it), so it is passed in. Returns a job-meaningful error that
    /// identifies the requested global page when no mapping exists
    /// (Requirement 7.4).
    pub fn locate(&self, map: &SegmentMap, global_page: usize) -> Result<(PathBuf, usize), String> {
        map.locate(global_page).ok_or_else(|| {
            format!(
                "Global page {} has no mapping (total_pages = {})",
                global_page, map.total_pages
            )
        })
    }

    /// Best-effort removal of the segment files in the per-document temp dir
    /// without consuming the owned `TempDir`. Used on split failure so partial
    /// `segment_*.pdf` files do not linger (Requirement 12.2). Errors are
    /// swallowed: cleanup is best-effort and the `TempDir` drop is the backstop.
    fn cleanup_temp_contents(&self) {
        if let Ok(entries) = std::fs::read_dir(self.temp_path()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        // Remove the temp directory itself. `TempDir` tolerates the directory
        // already being gone when it is later dropped.
        let _ = std::fs::remove_dir_all(self.temp_path());
    }

    /// Apply edits to segments and merge them.
    pub fn apply_and_merge<F>(
        &self,
        map: &SegmentMap,
        edits: Vec<GlobalEdit>,
        output_path: &Path,
        mut apply_fn: F,
    ) -> Result<MergeReport, crate::engine::pdf_split_merge::SplitMergeError>
    where
        F: FnMut(&PathBuf, &PathBuf, Vec<LocalEdit>) -> Result<(), String>,
    {
        map.validate_structure()
            .map_err(crate::engine::pdf_split_merge::SplitMergeError::Structure)?;
        let grouped = map.group_edits_by_segment(&edits).map_err(|error| {
            crate::engine::pdf_split_merge::SplitMergeError::Structure(error.to_string())
        })?;
        let segments_edited = grouped.len();
        let mut final_paths = Vec::with_capacity(map.segments.len());
        let mut expected_geometry = Vec::with_capacity(map.total_pages);

        for (index, segment) in map.segments.iter().enumerate() {
            let source_geometry = document_geometry(&segment.path)?;
            if source_geometry.len() != segment.page_count {
                return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
                    format!(
                        "segment {index} source page count {} does not match map count {}",
                        source_geometry.len(),
                        segment.page_count
                    ),
                ));
            }
            expected_geometry.extend(source_geometry.iter().cloned());
            let segment_edits = grouped.get(&index).cloned().unwrap_or_default();

            if segment_edits.is_empty() {
                final_paths.push(segment.path.clone());
                continue;
            }

            let edited_path = self
                .temp_path()
                .join(format!("segment_{index:03}_edited.pdf"));
            if edited_path.exists() {
                std::fs::remove_file(&edited_path)?;
            }
            if let Err(error) = apply_fn(&segment.path, &edited_path, segment_edits) {
                let _ = std::fs::remove_file(&edited_path);
                return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
                    format!("failed to apply edits to segment {index}: {error}"),
                ));
            }
            if !edited_path.is_file() {
                return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
                    format!("segment {index} apply reported success without an output artifact"),
                ));
            }
            validate_segment_replacement(&segment.path, &edited_path, segment.page_count).map_err(
                |error| {
                    crate::engine::pdf_split_merge::SplitMergeError::Structure(format!(
                        "segment {index} replacement is invalid: {error}"
                    ))
                },
            )?;
            final_paths.push(edited_path);
        }

        if final_paths.len() != map.segments.len() {
            return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
                "segment merge membership is incomplete".into(),
            ));
        }

        let output_parent = output_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(output_parent)?;
        let staged_path = tempfile::Builder::new()
            .prefix(".dcpp-segment-merge-")
            .suffix(".pdf")
            .tempfile_in(output_parent)?
            .into_temp_path();
        std::fs::remove_file(&staged_path)?;

        let merged_pages = crate::engine::pdf_split_merge::merge_pdfs(&final_paths, &staged_path)?;
        if merged_pages != map.total_pages {
            return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
                format!(
                    "merged page count {merged_pages} does not match map total {}",
                    map.total_pages
                ),
            ));
        }
        let merged_geometry = document_geometry(&staged_path)?;
        if merged_geometry != expected_geometry {
            return Err(crate::engine::pdf_split_merge::SplitMergeError::Structure(
                "merged page order, boxes, or rotations differ from the segment map".into(),
            ));
        }
        std::fs::File::open(&staged_path)?.sync_all()?;
        staged_path
            .persist(output_path)
            .map_err(|error| crate::engine::pdf_split_merge::SplitMergeError::Io(error.error))?;

        Ok(MergeReport {
            final_path: output_path.to_path_buf(),
            merged_pages,
            segments_edited,
            warnings: Vec::new(),
            review_flags: Vec::new(),
        })
    }

    /// Best-effort cleanup of temporary files.
    pub fn cleanup(self) {
        // TempDir cleans up on drop.
    }
}
