use crate::ai::document_ai::BankStatement;
use crate::ai::openai_client::OpenAiClient;
use crate::app::config::{AiProviderMode, AppConfig};
use crate::pdf::PdfEngine;
use std::path::Path;
use std::sync::Arc;

pub async fn parse_statement_openrouter(
    pdf_path: &Path,
    engine: Arc<dyn PdfEngine>,
    config: Arc<AppConfig>,
) -> Result<BankStatement, String> {
    // 1. Get OpenRouter Client
    let mut or_cfg = (*config).clone();
    or_cfg.ai_provider = AiProviderMode::OpenRouterApiKey;
    let client = OpenAiClient::from_app_config_async(&or_cfg)
        .await
        .map_err(|e| format!("Failed to create OpenRouter client: {}", e))?;

    // 2. Extract raw text via Pdfium
    let layout = engine
        .analyze_layout(pdf_path)
        .map_err(|e| format!("layout analysis failed: {e}"))?;

    let total_pages = layout.total_pages;
    if total_pages == 0 {
        return Err("PDF has 0 pages".into());
    }

    let mut full_text = String::new();
    for page in 0..total_pages {
        let blocks = engine.get_text_blocks(pdf_path, page).unwrap_or_default();
        for block in blocks {
            full_text.push_str(&block.text);
            full_text.push(' ');
        }
        full_text.push('\n');
    }

    if full_text.trim().is_empty() {
        return Err("Extracted text is empty. Cannot parse with OpenRouter.".into());
    }

    // 3. Call OpenRouter
    tracing::info!(
        "[openrouter_parser] Sending {} characters to OpenRouter for transaction extraction...",
        full_text.len()
    );
    let transactions = client
        .parse_transactions_from_text(&full_text)
        .await
        .map_err(|e| format!("OpenRouter parsing failed: {}", e))?;

    tracing::info!(
        "[openrouter_parser] Extracted {} transactions.",
        transactions.len()
    );

    // 4. Construct BankStatement
    let statement = BankStatement {
        total_pages,
        transactions,
        opening_balance: rust_decimal::Decimal::ZERO,
        closing_balance: rust_decimal::Decimal::ZERO,
        account_number: None,
        bank_name: None,
    };

    Ok(statement)
}
