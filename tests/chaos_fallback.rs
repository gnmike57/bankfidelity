use dual_core_pdf_pipeline::ai::document_ai::BankStatement;
use dual_core_pdf_pipeline::app::config::{AppConfig, ConfigManager, PdfEngineMode};
use dual_core_pdf_pipeline::pdf::{
    DocumentLayout, EngineCapabilities, EngineError, PdfEngine, PdfEngineSelector, RenderedPage,
    ReplaceOutcome, TextBlock,
};
use rust_decimal::Decimal;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
struct MockFailingEngine;

impl PdfEngine for MockFailingEngine {
    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            supports_redaction: false,
            supports_cjk: false,
            supports_embedded_fonts: true,
            estimated_fidelity: 1.0,
        }
    }

    fn get_text_blocks(
        &self,
        _path: &Path,
        _page_num: usize,
    ) -> Result<Vec<TextBlock>, EngineError> {
        Ok(vec![])
    }

    fn apply_change(
        &self,
        _input_path: &Path,
        _output_path: &Path,
        _page_num: usize,
        _rect: [f32; 4],
        _new_text: &str,
        _font_name: &str,
        _font_path: Option<&Path>,
    ) -> Result<ReplaceOutcome, EngineError> {
        Err(EngineError::EncryptedOrRasterized(
            "Simulated scan detected".into(),
        ))
    }

    fn render_page(
        &self,
        _path: &Path,
        _page_num: usize,
        _dpi: f32,
    ) -> Result<RenderedPage, EngineError> {
        Err(EngineError::Unsupported)
    }

    fn find_text_block_at_click(
        &self,
        _path: &Path,
        _page_num: usize,
        _x: f32,
        _y: f32,
    ) -> Result<Option<TextBlock>, EngineError> {
        Ok(None)
    }

    fn analyze_layout(&self, _path: &Path) -> Result<DocumentLayout, EngineError> {
        Err(EngineError::Unsupported)
    }
}

#[tokio::test]
async fn test_chaos_fallback_selector_returns_encrypted_error() {
    let mut cfg_val = AppConfig::default();
    cfg_val.engine_mode = PdfEngineMode::PyMuPdfProPrimary;
    let config = ConfigManager::new(Arc::new(cfg_val));

    let mock_primary = Arc::new(MockFailingEngine);
    let mock_fallback = Arc::new(MockFailingEngine);

    let selector = PdfEngineSelector::new(mock_primary, mock_fallback, config);

    let result = selector.apply_change(
        Path::new("dummy.pdf"),
        Path::new("dummy_out.pdf"),
        0,
        [0.0, 0.0, 10.0, 10.0],
        "test",
        "Helvetica",
        None,
    );

    assert!(matches!(result, Err(EngineError::EncryptedOrRasterized(_))));
}

#[tokio::test]
async fn test_chaos_fallback_triggers_typst_reconstruct() {
    let statement = BankStatement {
        total_pages: 1,
        transactions: vec![],
        bank_name: None,
        opening_balance: Decimal::ZERO,
        closing_balance: Decimal::ZERO,
        account_number: None,
    };
    let typst_engine = dual_core_pdf_pipeline::engine::typst_engine::TypstEngine::new();
    let out_path = std::env::temp_dir().join(format!("reconstruct_{}.pdf", uuid::Uuid::new_v4()));

    let result = typst_engine.reconstruct_pdf(&statement, &out_path).await;
    assert!(
        result.is_ok(),
        "Typst reconstruct should succeed or fallback gracefully. Err: {:?}",
        result.err()
    );

    let _ = std::fs::remove_file(out_path);
}
