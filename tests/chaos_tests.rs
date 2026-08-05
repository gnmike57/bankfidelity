use dual_core_pdf_pipeline::ai::backend::AiBackend;
use dual_core_pdf_pipeline::ai::gemini_client::GeminiClient;
use dual_core_pdf_pipeline::app::config::AiProviderMode;
use rust_decimal_macros::dec;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_chaos_malformed_json_repair() {
    let _ = dotenvy::dotenv();
    let server = MockServer::start().await;

    // A response that is just invalid junk.
    let mock_response = r#"{
        "candidates": [
            {
                "content": {
                    "parts": [
                        {
                            "text": "THIS IS NOT JSON AND SHOULD BREAK THE PARSER [{garbage]"
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

    use dual_core_pdf_pipeline::engine::model::{Provenance, Transaction};

    let tx1 = Transaction {
        page: 1,
        line_on_page: 1,
        date: "2023-01-01".into(),
        raw_text: "Target tx".into(),
        debit: None,
        credit: Some(dec!(100.0)),
        running_balance: None,
        bbox: None,
        field_bboxes: Default::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };

    let result = backend
        .repair_extracted_transactions(
            std::slice::from_ref(&tx1),
            dec!(100.0),
            dec!(100.0),
            "Original Text",
            "Just fix it",
        )
        .await;

    // Should gracefully return an Error, not panic!
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("JSON parse error"));
}
