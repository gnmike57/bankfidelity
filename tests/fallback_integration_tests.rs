use dual_core_pdf_pipeline::engine::layout::DocumentLayout;
use dual_core_pdf_pipeline::pdf::engine::{
    EngineCapabilities, RenderedPage, ReplaceOutcome, TextBlock,
};
use dual_core_pdf_pipeline::pdf::{EngineError, PdfEngine};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
struct DummyEngine;
impl PdfEngine for DummyEngine {
    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            supports_redaction: false,
            supports_cjk: false,
            supports_embedded_fonts: false,
            estimated_fidelity: 0.0,
        }
    }
    fn render_page(
        &self,
        _path: &std::path::Path,
        _page: usize,
        _dpi: f32,
    ) -> Result<RenderedPage, EngineError> {
        Ok(RenderedPage {
            png_bytes: vec![],
            width_pts: 100.0,
            height_pts: 100.0,
        })
    }
    fn get_text_blocks(
        &self,
        _path: &std::path::Path,
        _page: usize,
    ) -> Result<Vec<TextBlock>, EngineError> {
        Ok(vec![])
    }
    fn find_text_block_at_click(
        &self,
        _path: &std::path::Path,
        _page: usize,
        _x: f32,
        _y: f32,
    ) -> Result<Option<TextBlock>, EngineError> {
        Ok(None)
    }
    fn apply_change(
        &self,
        _i: &std::path::Path,
        _o: &std::path::Path,
        _p: usize,
        _b: [f32; 4],
        _n: &str,
        _ot: &str,
        _fp: Option<&std::path::Path>,
    ) -> Result<ReplaceOutcome, EngineError> {
        Ok(ReplaceOutcome {
            success: true,
            font_used: "MockFont".into(),
            overflow: false,
            obj_id: None,
        })
    }
    fn analyze_layout(&self, path: &std::path::Path) -> Result<DocumentLayout, EngineError> {
        if !path.exists() {
            return Err(EngineError::ExtractFailed(
                "Failed to parse statement offline".to_string(),
            ));
        }
        Ok(DocumentLayout {
            total_pages: 1,
            pages: vec![],
            has_consistent_headers: true,
            has_consistent_footers: true,
            overall_style: "Standard".to_string(),
            layout_confidence: 1.0,
        })
    }
}

#[tokio::test]
async fn test_offline_fallback_can_be_triggered() {
    // Instead, we will simulate the exact fallback invocation that occurs in the runtime

    // Create a dummy PDF engine
    let engine = Arc::new(DummyEngine) as Arc<dyn PdfEngine>;
    let engine_for_tokio = engine.clone();

    // Execute the exact fallback logic present in `runtime.rs:3342`
    let path = PathBuf::from("nonexistent.pdf");

    let result = tokio::task::spawn_blocking(move || {
        // Here we simulate the fallback call
        dual_core_pdf_pipeline::engine::offline_parser::parse_statement_offline(
            &path,
            engine_for_tokio,
        )
    })
    .await;

    match result {
        Ok(Ok(_)) => panic!("Should not succeed on nonexistent PDF"),
        Ok(Err(e)) => {
            // The fallback executes and cleanly returns an EngineError, it does NOT panic.
            assert!(
                e.to_string().contains("Failed to parse statement offline"),
                "Fallback successfully ran and safely handled the missing file"
            );
        }
        Err(e) => {
            panic!("Fallback panicked instead of returning an error: {:?}", e);
        }
    }
}
