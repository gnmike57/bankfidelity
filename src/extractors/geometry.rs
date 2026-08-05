use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeometrySource {
    TextLayer,
    Ocr,
    BankTemplate { template_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineGeometry {
    pub page: usize,
    pub line_on_page: usize,
    pub text: String,
    pub bbox: [f32; 4],
    pub confidence: f32,
    pub source: GeometrySource,
}

/// A bounding box detected from table grid analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

#[derive(Error, Debug)]
pub enum ExtractorError {
    #[error("Failed to extract geometry: {0}")]
    ExtractionFailed(String),
}

pub trait GeometryProvider: Send + Sync {
    fn extract_line_geometry(&self, pdf_path: &Path) -> Result<Vec<LineGeometry>, ExtractorError>;
}

/// Phase 5.1: Detect table grid lines from an image using native `imageproc`.
///
/// Converts to grayscale, applies Canny edge detection, then uses the
/// Hough line transform to find horizontal and vertical lines. The
/// intersections of these lines define table cell bounding boxes.
pub fn detect_table_grid(image: &image::DynamicImage) -> Vec<BoundingBox> {
    use image::GrayImage;
    use imageproc::edges::canny;

    let gray: GrayImage = image.to_luma8();

    // Canny edge detection with thresholds tuned for printed documents
    let edges = canny(&gray, 50.0, 150.0);

    // Scan for horizontal and vertical line segments by looking for runs
    // of edge pixels. This is simpler and more reliable than full Hough
    // transforms for structured bank statement tables.
    let (w, h) = (edges.width(), edges.height());
    let mut h_lines: Vec<(u32, u32, u32)> = Vec::new(); // (y, x_start, x_end)
    let mut v_lines: Vec<(u32, u32, u32)> = Vec::new(); // (x, y_start, y_end)

    // Detect horizontal lines (runs of ≥50 edge pixels in a row)
    let min_run = 50u32;
    for y in 0..h {
        let mut run_start = None;
        for x in 0..w {
            if edges.get_pixel(x, y).0[0] > 128 {
                if run_start.is_none() {
                    run_start = Some(x);
                }
            } else if let Some(start) = run_start {
                if x - start >= min_run {
                    h_lines.push((y, start, x));
                }
                run_start = None;
            }
        }
        if let Some(start) = run_start {
            if w - start >= min_run {
                h_lines.push((y, start, w));
            }
        }
    }

    // Detect vertical lines (runs of ≥30 edge pixels in a column)
    let v_min_run = 30u32;
    for x in 0..w {
        let mut run_start = None;
        for y in 0..h {
            if edges.get_pixel(x, y).0[0] > 128 {
                if run_start.is_none() {
                    run_start = Some(y);
                }
            } else if let Some(start) = run_start {
                if y - start >= v_min_run {
                    v_lines.push((x, start, y));
                }
                run_start = None;
            }
        }
        if let Some(start) = run_start {
            if h - start >= v_min_run {
                v_lines.push((x, start, h));
            }
        }
    }

    // Build bounding boxes from consecutive horizontal line pairs
    // intersected with vertical line boundaries
    let mut boxes = Vec::new();

    // Deduplicate h_lines by merging nearby y-values (within 3px)
    let mut unique_ys: Vec<u32> = h_lines.iter().map(|l| l.0).collect();
    unique_ys.sort_unstable();
    unique_ys.dedup_by(|a, b| (*a as i32 - *b as i32).unsigned_abs() < 3);

    // Similarly for vertical x-values
    let mut unique_xs: Vec<u32> = v_lines.iter().map(|l| l.0).collect();
    unique_xs.sort_unstable();
    unique_xs.dedup_by(|a, b| (*a as i32 - *b as i32).unsigned_abs() < 3);

    // Form grid cells from consecutive (y_i, y_{i+1}) × (x_j, x_{j+1})
    for yi in 0..unique_ys.len().saturating_sub(1) {
        for xi in 0..unique_xs.len().saturating_sub(1) {
            let y0 = unique_ys[yi] as f32;
            let y1 = unique_ys[yi + 1] as f32;
            let x0 = unique_xs[xi] as f32;
            let x1 = unique_xs[xi + 1] as f32;

            // Only include cells of reasonable size
            if (x1 - x0) > 20.0 && (y1 - y0) > 8.0 {
                boxes.push(BoundingBox { x0, y0, x1, y1 });
            }
        }
    }

    boxes
}

/// Phase 5: Native text-layer geometry provider using OxidizePdfEngine.
/// Replaces the deleted PyMuPDF and Tesseract providers.
pub struct NativeTextLayerProvider {
    engine: std::sync::Arc<dyn crate::pdf::PdfEngine>,
}

impl NativeTextLayerProvider {
    pub fn new(engine: std::sync::Arc<dyn crate::pdf::PdfEngine>) -> Self {
        Self { engine }
    }
}

impl GeometryProvider for NativeTextLayerProvider {
    fn extract_line_geometry(&self, pdf_path: &Path) -> Result<Vec<LineGeometry>, ExtractorError> {
        let layout = self
            .engine
            .analyze_layout(pdf_path)
            .map_err(|e| ExtractorError::ExtractionFailed(e.to_string()))?;

        let mut geometries = Vec::new();

        for page in 0..layout.total_pages {
            let blocks = self
                .engine
                .get_text_blocks(pdf_path, page)
                .unwrap_or_default();

            for (i, block) in blocks.iter().enumerate() {
                geometries.push(LineGeometry {
                    page,
                    line_on_page: i,
                    text: block.text.clone(),
                    bbox: block.bbox,
                    confidence: 1.0,
                    source: GeometrySource::TextLayer,
                });
            }
        }

        Ok(geometries)
    }
}
