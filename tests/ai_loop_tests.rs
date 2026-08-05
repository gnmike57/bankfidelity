use dual_core_pdf_pipeline::ai::backend::AiBackend;
use dual_core_pdf_pipeline::ai::document_ai::BankStatement;
use dual_core_pdf_pipeline::ai::gemini_client::GeminiClient;
use dual_core_pdf_pipeline::app::config::AiProviderMode;
use dual_core_pdf_pipeline::engine::model::{Provenance, Transaction};
use rust_decimal_macros::dec;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_verify_and_repair_extraction_loop() {
    let _ = dotenvy::dotenv();
    let server = MockServer::start().await;

    let mock_response = r#"{
        "candidates": [
            {
                "content": {
                    "parts": [
                        {
                            "text": "[{\"page\": 1, \"line_on_page\": 1, \"date\": \"2023-10-01\", \"raw_text\": \"Repaired Transaction\", \"debit\": \"50.00\", \"credit\": \"0.00\", \"provenance\": \"Computed\"}]"
                        }
                    ]
                }
            }
        ]
    }"#;

    Mock::given(method("POST"))
        .and(path("/v1beta/models/gemini-pro-latest:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
        .mount(&server)
        .await;

    let backend = AiBackend {
        primary: AiProviderMode::GeminiApiKey,
        gemini: Some(GeminiClient::with_base_url("test_key".into(), server.uri())),
        openrouter: None,
        groq: None,
        mistral: None,
    };

    let stmt = BankStatement {
        opening_balance: dec!(100.0),
        closing_balance: dec!(1000.0),
        transactions: vec![Transaction {
            page: 1,
            line_on_page: 1,
            date: "2023-10-01".into(),
            raw_text: "Broken Transaction".into(),
            debit: Some(dec!(50.0)),
            credit: Some(dec!(0.0)),
            running_balance: None,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Computed,
            category: None,
            canonical: Default::default(),
        }],
        total_pages: 1,
        account_number: None,
        bank_name: None,
    };

    let result = dual_core_pdf_pipeline::ai::repair::verify_and_repair_extraction(
        &backend,
        stmt.clone(),
        "OCR TEXT",
    )
    .await;

    let repaired_stmt = result.unwrap();

    assert_eq!(repaired_stmt.transactions.len(), 1);
    assert_eq!(
        repaired_stmt.transactions[0].raw_text,
        "Repaired Transaction"
    );
}
