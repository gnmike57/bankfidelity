//! Live AI integration tests.
//!
//! These probe the actual external services with the keys in `.env`. They
//! are kept separate from the deterministic mock-based tests so they only
//! run when the developer asks for them with `--ignored`.
//!
//! Run with:
//!   cargo test --test ai_live -- --ignored --nocapture

use std::sync::Arc;

use dual_core_pdf_pipeline::ai::document_ai::DocumentAiClient;
use dual_core_pdf_pipeline::ai::gemini_client::GeminiClient;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::engine::layout::DocumentLayout;

fn cfg() -> Arc<AppConfig> {
    let _ = dotenvy::dotenv();
    let cfg = AppConfig::from_env().expect("AppConfig::from_env failed; .env not loaded?");
    Arc::new(cfg)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "live network test - requires GEMINI_API_KEY in .env"]
async fn gemini_client_can_propose_a_balance_plan() {
    let cfg = cfg();
    if cfg.gemini_api_key.is_none() {
        eprintln!("SKIP: GEMINI_API_KEY not set");
        return;
    }

    let client = GeminiClient::from_app_config(&cfg).expect("Gemini client construction");
    let layout = DocumentLayout {
        total_pages: 1,
        pages: vec![],
        has_consistent_headers: false,
        has_consistent_footers: false,
        overall_style: "test".into(),
        layout_confidence: 1.0,
    };

    // Empty transaction list with a tiny imbalance should still get a structured
    // response (the model can choose to return zero adjustments, which is fine).
    match client.propose_balance_adjustments(&[], 5.00, &layout).await {
        Ok(plan) => {
            println!("✅ Gemini ok — strategy: {}", plan.overall_strategy);
            println!("   confidence: {:.2}", plan.confidence);
            println!("   adjustments: {}", plan.adjustments.len());
            assert!(plan.confidence >= 0.0 && plan.confidence <= 1.0);
        }
        Err(e) => {
            // LowConfidence is a legitimate response shape, not a failure.
            use dual_core_pdf_pipeline::ai::gemini_client::GeminiError;
            match e {
                GeminiError::LowConfidence(c) => {
                    println!("✅ Gemini ok (low-confidence path): {c:.2}");
                }
                GeminiError::Api(status, msg)
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS =>
                {
                    eprintln!("SKIP: Gemini rate limited (HTTP 429): {msg}");
                }
                other => panic!("Gemini call failed: {other}"),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "live network test - requires DOCUMENT_AI_* in .env"]
async fn document_ai_client_constructs_when_configured() {
    let cfg = cfg();
    if cfg.document_ai.is_none() {
        eprintln!("SKIP: Document AI not configured");
        return;
    }
    if DocumentAiClient::from_app_config(&cfg).is_err() {
        eprintln!("SKIP: Document AI missing full valid credentials");
        return;
    }
    println!("✅ Document AI client ready");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "live network test - requires DOCUMENT_AI_* in .env + a sample PDF"]
async fn document_ai_can_parse_a_real_statement() {
    let cfg = cfg();
    if cfg.document_ai.is_none() {
        eprintln!("SKIP: Document AI not configured");
        return;
    }
    let sa_path = &cfg.document_ai.as_ref().unwrap().service_account_path;
    let adc_path = &cfg.document_ai.as_ref().unwrap().adc_path;
    if (sa_path.is_empty() || !std::path::Path::new(sa_path).exists())
        && (adc_path.is_empty() || !std::path::Path::new(adc_path).exists())
    {
        eprintln!("SKIP: Document AI credentials file not found");
        return;
    }

    let pdf = std::path::PathBuf::from("examples/sample.pdf");
    if !pdf.exists() {
        eprintln!("SKIP: examples/sample.pdf not present");
        return;
    }

    let client = DocumentAiClient::from_app_config(&cfg).expect("ctor");
    match client.parse_entire_statement(&pdf, None).await {
        Ok(stmt) => {
            println!("✅ Document AI parsed sample.pdf");
            println!("   pages: {}", stmt.total_pages);
            println!("   transactions: {}", stmt.transactions.len());
            println!("   opening: {:.2}", stmt.opening_balance);
            println!("   closing: {:.2}", stmt.closing_balance);
        }
        Err(e) => panic!("Document AI call failed: {e}"),
    }
}
