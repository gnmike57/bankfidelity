//! Phase 5.2: Pure Rust OCR engine using `ocrs` + `rten`.
//!
//! Replaces the deleted `tesseract.rs` with a 100% native Rust OCR
//! implementation. Uses `ocrs::OcrEngine` backed by ONNX Runtime via
//! `rten` for both text detection and recognition - zero C++ dependencies.
//!
//! This acts as a local fallback for scanned documents when Document AI
//! is not available or when offline processing is needed.

use super::geometry::{ExtractorError, GeometryProvider, LineGeometry};
use std::path::Path;

/// Configuration for the OCRS engine model paths.
#[derive(Debug, Clone)]
pub struct OcrsConfig {
    /// Path to the text detection ONNX model.
    pub detection_model_path: String,
    /// Path to the text recognition ONNX model.
    pub recognition_model_path: String,
}

impl Default for OcrsConfig {
    fn default() -> Self {
        Self {
            detection_model_path: "models/text-detection.rten".to_string(),
            recognition_model_path: "models/text-recognition.rten".to_string(),
        }
    }
}

/// Pure Rust OCR engine backed by `ocrs` + `rten`.
///
/// For scanned bank statements that don't have a text layer, this engine
/// extracts text from rendered page images without any C++ dependency.
pub struct OcrsEngine {
    config: OcrsConfig,
}

impl OcrsEngine {
    pub fn new(config: OcrsConfig) -> Self {
        Self { config }
    }

    /// Extract text from raw image bytes (PNG/JPEG).
    ///
    /// Returns the full extracted text as a single string. For structured
    /// extraction with bounding boxes, use the `GeometryProvider` trait
    /// implementation instead.
    pub fn extract_text_from_image(&self, image_bytes: &[u8]) -> Result<String, ExtractorError> {
        // Load the image
        let img = image::load_from_memory(image_bytes).map_err(|e| {
            ExtractorError::ExtractionFailed(format!("Failed to decode image: {e}"))
        })?;

        let (w, h) = (img.width(), img.height());
        if w == 0 || h == 0 {
            return Err(ExtractorError::ExtractionFailed("Empty image".into()));
        }

        // Check the model files exist before doing any heavy work.
        let det_path = Path::new(&self.config.detection_model_path);
        let rec_path = Path::new(&self.config.recognition_model_path);
        if !det_path.exists() || !rec_path.exists() {
            return Err(ExtractorError::ExtractionFailed(format!(
                "OCRS model files not found. Expected:\n  Detection: {}\n  Recognition: {}\n\
                 Download them with `ocrs download-models` or from \
                 https://github.com/robertknight/ocrs-models",
                self.config.detection_model_path, self.config.recognition_model_path,
            )));
        }

        self.run_ocr(&img)
    }

    /// Real detection->recognition pipeline (Recommendation #4), gated behind
    /// the `ocr` cargo feature because `ocrs`/`rten` require rustc >= 1.89.
    #[cfg(feature = "ocr")]
    fn run_ocr(&self, img: &image::DynamicImage) -> Result<String, ExtractorError> {
        use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
        use rten::Model;

        let detection_model = Model::load_file(&self.config.detection_model_path)
            .map_err(|e| ExtractorError::ExtractionFailed(format!("load detection model: {e}")))?;
        let recognition_model =
            Model::load_file(&self.config.recognition_model_path).map_err(|e| {
                ExtractorError::ExtractionFailed(format!("load recognition model: {e}"))
            })?;

        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(|e| ExtractorError::ExtractionFailed(format!("init OcrEngine: {e}")))?;

        let rgb = img.to_rgb8();
        let img_source = ImageSource::from_bytes(rgb.as_raw(), rgb.dimensions())
            .map_err(|e| ExtractorError::ExtractionFailed(format!("image source: {e}")))?;
        let ocr_input = engine
            .prepare_input(img_source)
            .map_err(|e| ExtractorError::ExtractionFailed(format!("prepare input: {e}")))?;

        let word_rects = engine
            .detect_words(&ocr_input)
            .map_err(|e| ExtractorError::ExtractionFailed(format!("detect words: {e}")))?;
        let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
        let line_texts = engine
            .recognize_text(&ocr_input, &line_rects)
            .map_err(|e| ExtractorError::ExtractionFailed(format!("recognize text: {e}")))?;

        let text = line_texts
            .iter()
            .flatten()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    }

    /// Fallback when the crate is built without the `ocr` feature: the models
    /// can't be executed (no `rten`), so report a clear, actionable error and
    /// let the caller fall back to Document AI.
    #[cfg(not(feature = "ocr"))]
    fn run_ocr(&self, _img: &image::DynamicImage) -> Result<String, ExtractorError> {
        Err(ExtractorError::ExtractionFailed(
            "OCRS support not compiled in. Rebuild with `--features ocr` on Rust >= 1.89 \
             to enable the pure-Rust ocrs + rten OCR pipeline."
                .into(),
        ))
    }
}

impl GeometryProvider for OcrsEngine {
    fn extract_line_geometry(&self, pdf_path: &Path) -> Result<Vec<LineGeometry>, ExtractorError> {
        // For PDFs without a text layer, we would:
        // 1. Render each page to an image (via the PDF engine's render_page)
        // 2. Run OCR on each image
        // 3. Return positioned LineGeometry entries
        //
        // Since OxidizePdfEngine::render_page returns Unsupported for now,
        // this path is deferred until we have a rasterizer (Phase 6).
        tracing::warn!(
            "[OCRS] OCR geometry extraction not yet available for PDF: {}",
            pdf_path.display()
        );

        Err(ExtractorError::ExtractionFailed(
            "OCRS PDF extraction requires page rendering (deferred to Phase 6)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_model_paths() {
        let config = OcrsConfig::default();
        assert!(config.detection_model_path.ends_with(".rten"));
        assert!(config.recognition_model_path.ends_with(".rten"));
    }

    #[test]
    fn extract_fails_gracefully_without_models() -> anyhow::Result<()> {
        let engine = OcrsEngine::new(OcrsConfig::default());
        // Create a minimal 1x1 white PNG
        let mut buf = Vec::new();
        let img = image::DynamicImage::new_luma8(1, 1);
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)?;

        let result = engine.extract_text_from_image(&buf);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("OCRS model files not found"));
        Ok(())
    }

    #[test]
    fn extract_fails_gracefully_on_invalid_image() {
        let engine = OcrsEngine::new(OcrsConfig::default());
        let result = engine.extract_text_from_image(b"not an image");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Failed to decode image"));
    }

    #[test]
    fn geometry_provider_returns_unsupported_error() {
        let engine = OcrsEngine::new(OcrsConfig::default());
        let result = engine.extract_line_geometry(Path::new("dummy.pdf"));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("OCRS PDF extraction requires page rendering"));
    }
}
