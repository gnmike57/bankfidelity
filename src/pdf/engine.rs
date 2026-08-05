use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Unsupported operation for this engine")]
    Unsupported,
    #[error("Failed to load document: {0}")]
    LoadFailed(String),
    #[error("Failed to render page: {0}")]
    RenderFailed(String),
    #[error("Failed to extract text: {0}")]
    ExtractFailed(String),
    #[error("Failed to apply change: {0}")]
    ApplyFailed(String),
    #[error("Layout analysis failed: {0}")]
    LayoutFailed(String),
    #[error("Font subset does not cover required characters: {0}")]
    FontCoverageMissing(String),
    #[error("PDF is an encrypted/image scan, editing in place failed: {0}")]
    EncryptedOrRasterized(String),
    /// The bbox supplied to `apply_change` doesn't sufficiently overlap any
    /// real text span on the page. This guards against editing the wrong row
    /// when the document has multiple cells with the same value (e.g. a
    /// transaction amount that equals the closing balance below it).
    #[error("Row drifted: bbox [{x0:.1}, {y0:.1}, {x1:.1}, {y1:.1}] does not overlap any span by ≥{required:.0}% (best={best:.0}%)")]
    RowDrifted {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        required: f32,
        best: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCapabilities {
    pub supports_redaction: bool,
    pub supports_cjk: bool,
    pub supports_embedded_fonts: bool,
    pub estimated_fidelity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceOutcome {
    pub success: bool,
    pub font_used: String,
    pub overflow: bool,
    pub obj_id: Option<String>,
}

/// Extracted text geometry in canonical page coordinates.
///
/// `bbox` is always `[x0, y0, x1, y1]` in PDF points with the origin at the
/// visible top-left, matching PyMuPDF spans, rendered-page clicks, GUI overlays,
/// and Python edit rectangles. Engines convert internal geometry at their own
/// implementation boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub page: usize,
    pub text: String,
    pub bbox: [f32; 4],
    pub font: String,
    pub size: f32,
    pub obj_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub png_bytes: Vec<u8>,
    pub width_pts: f32,
    pub height_pts: f32,
}

// Re-export DocumentLayout from engine layer
pub use crate::engine::layout::DocumentLayout;

/// Compute the fraction of `bbox`'s area that overlaps `span_bbox`. Returns
/// `0.0` when the rectangles don't intersect at all, `1.0` when `bbox` is
/// entirely inside `span_bbox` (and equal in area), or any value in between.
///
/// Both rectangles use canonical top-left page coordinates:
/// `[x0, y0, x1, y1]` with `x1 > x0` and `y1 > y0`. Out-of-order rects yield
/// `0.0` rather than panicking.
pub fn bbox_overlap_fraction(bbox: [f32; 4], span_bbox: [f32; 4]) -> f32 {
    let area = |r: [f32; 4]| {
        let w = (r[2] - r[0]).max(0.0);
        let h = (r[3] - r[1]).max(0.0);
        w * h
    };
    let bbox_area = area(bbox);
    if bbox_area <= 0.0 {
        return 0.0;
    }
    let inter = [
        bbox[0].max(span_bbox[0]),
        bbox[1].max(span_bbox[1]),
        bbox[2].min(span_bbox[2]),
        bbox[3].min(span_bbox[3]),
    ];
    if inter[2] <= inter[0] || inter[3] <= inter[1] {
        return 0.0;
    }
    area(inter) / bbox_area
}

/// Resolve the dominant text span at `bbox` on a given page and return how
/// much of `bbox` overlaps that span. Returns `None` when nothing on the
/// page intersects `bbox` at all.
///
/// Used by `apply_change` for the row-drift guard: if the caller-supplied
/// bbox doesn't overlap any real span by ≥`required` (e.g. 0.5 = 50%), we
/// refuse to apply the edit rather than risk redacting the wrong cell.
pub fn dominant_span_overlap(
    blocks: &[TextBlock],
    page: usize,
    bbox: [f32; 4],
) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for (idx, b) in blocks.iter().enumerate() {
        if b.page != page {
            continue;
        }
        let f = bbox_overlap_fraction(bbox, b.bbox);
        if f <= 0.0 {
            continue;
        }
        match best {
            None => best = Some((idx, f)),
            Some((_, best_f)) if f > best_f => best = Some((idx, f)),
            _ => {}
        }
    }
    best
}

/// The core trait for PDF rendering and manipulation.
pub trait PdfEngine: Send + Sync + std::fmt::Debug {
    fn capabilities(&self) -> EngineCapabilities;

    fn render_page(&self, path: &Path, page: usize, dpi: f32) -> Result<RenderedPage, EngineError>;

    fn get_text_blocks(&self, path: &Path, page: usize) -> Result<Vec<TextBlock>, EngineError>;

    fn find_text_block_at_click(
        &self,
        path: &Path,
        page: usize,
        x: f32,
        y: f32,
    ) -> Result<Option<TextBlock>, EngineError>;

    #[allow(clippy::too_many_arguments)]
    fn apply_change(
        &self,
        input: &Path,
        output: &Path,
        page: usize,
        bbox: [f32; 4],
        new_text: &str,
        old_text: &str,
        font_path: Option<&Path>,
    ) -> Result<ReplaceOutcome, EngineError>;

    fn analyze_layout(&self, path: &Path) -> Result<DocumentLayout, EngineError>;

    fn apply_many_edits(
        &self,
        _input: &Path,
        _output: &Path,
        _edits_json: &str,
        _font_path: Option<&Path>,
    ) -> Result<usize, EngineError> {
        Err(EngineError::Unsupported)
    }

    fn clone_pages(
        &self,
        _input: &Path,
        _output: &Path,
        _page_indices: Vec<usize>,
    ) -> Result<usize, EngineError> {
        Err(EngineError::Unsupported)
    }

    fn remove_pages(
        &self,
        _input: &Path,
        _output: &Path,
        _page_indices: Vec<usize>,
    ) -> Result<usize, EngineError> {
        Err(EngineError::Unsupported)
    }

    /// Apply a change after first asserting that `bbox` overlaps a real text
    /// span on `page` by at least `required_overlap` (0..1, e.g. 0.5 = 50%).
    /// Returns `EngineError::RowDrifted` when the guard fails.
    ///
    /// Engines that can't enumerate text blocks (e.g. the mupdf path) just
    /// fall through to `apply_change` without the guard.
    #[allow(clippy::too_many_arguments)]
    fn apply_change_guarded(
        &self,
        input: &Path,
        output: &Path,
        page: usize,
        bbox: [f32; 4],
        new_text: &str,
        old_text: &str,
        font_path: Option<&Path>,
        required_overlap: f32,
    ) -> Result<ReplaceOutcome, EngineError> {
        match self.get_text_blocks(input, page) {
            Ok(blocks) if !blocks.is_empty() => {
                let best_frac = dominant_span_overlap(&blocks, page, bbox)
                    .map(|(_, f)| f)
                    .unwrap_or(0.0);
                if best_frac < required_overlap {
                    return Err(EngineError::RowDrifted {
                        x0: bbox[0],
                        y0: bbox[1],
                        x1: bbox[2],
                        y1: bbox[3],
                        required: required_overlap * 100.0,
                        best: best_frac * 100.0,
                    });
                }
            }
            // No blocks available (engine doesn't support extraction, or
            // genuinely empty page). Skip the guard rather than blocking
            // legitimate edits on a page we can't introspect.
            _ => {}
        }
        self.apply_change(input, output, page, bbox, new_text, old_text, font_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(page: usize, bbox: [f32; 4]) -> TextBlock {
        TextBlock {
            page,
            text: "100.00".into(),
            bbox,
            font: String::new(),
            size: 12.0,
            obj_id: None,
        }
    }

    #[test]
    fn bbox_overlap_full_match_returns_one() {
        let b = [10.0, 10.0, 50.0, 30.0];
        assert!((bbox_overlap_fraction(b, b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bbox_overlap_no_intersection_returns_zero() {
        let a = [0.0, 0.0, 10.0, 10.0];
        let b = [100.0, 100.0, 110.0, 110.0];
        assert_eq!(bbox_overlap_fraction(a, b), 0.0);
    }

    #[test]
    fn bbox_overlap_half_overlap_returns_half() {
        // 20x20 bbox, 50% inside a 10x20 strip on the right half
        let bbox = [0.0, 0.0, 20.0, 20.0];
        let span = [10.0, 0.0, 30.0, 20.0];
        let f = bbox_overlap_fraction(bbox, span);
        assert!((f - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bbox_overlap_zero_area_bbox_returns_zero() {
        let bbox = [10.0, 10.0, 10.0, 10.0]; // degenerate
        let span = [0.0, 0.0, 100.0, 100.0];
        assert_eq!(bbox_overlap_fraction(bbox, span), 0.0);
    }

    #[test]
    fn dominant_span_overlap_picks_the_best_match() -> anyhow::Result<()> {
        // Two spans on page 0; bbox sits closer to the second.
        let blocks = vec![
            block(0, [0.0, 0.0, 20.0, 20.0]),    // far from bbox
            block(0, [50.0, 50.0, 100.0, 70.0]), // partial overlap
            block(1, [55.0, 55.0, 75.0, 75.0]),  // wrong page, must not match
        ];
        let bbox = [55.0, 55.0, 75.0, 75.0];
        let (idx, frac) = dominant_span_overlap(&blocks, 0, bbox)
            .ok_or_else(|| anyhow::anyhow!("No overlap found"))?;
        assert_eq!(idx, 1);
        assert!((frac - 0.75).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn dominant_span_overlap_returns_none_when_nothing_intersects() {
        let blocks = vec![block(0, [0.0, 0.0, 10.0, 10.0])];
        assert!(dominant_span_overlap(&blocks, 0, [200.0, 200.0, 210.0, 210.0]).is_none());
    }

    /// Regression for Item #1 of Stage 2: an attacker (or stale state)
    /// supplies a bbox that overlaps multiple "100.00" text cells. The
    /// resolver picks the one with the largest fractional overlap, NOT the
    /// first one in document order.
    #[test]
    fn dominant_span_overlap_prefers_larger_fraction_over_document_order() -> anyhow::Result<()> {
        // Two cells with identical text; only one has a near-perfect bbox match.
        let blocks = vec![
            block(0, [50.0, 50.0, 100.0, 70.0]),    // partial overlap
            block(0, [200.0, 200.0, 250.0, 220.0]), // exact overlap with target
        ];
        let bbox = [200.0, 200.0, 250.0, 220.0];
        let (idx, _) = dominant_span_overlap(&blocks, 0, bbox)
            .ok_or_else(|| anyhow::anyhow!("No overlap found"))?;
        assert_eq!(idx, 1, "must pick the cell that the bbox actually covers");
        Ok(())
    }
}
