use dual_core_pdf_pipeline::ai::gemini_client::{GeminiClient, GeminiError};
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::pdf::DocumentLayout;
use mockito::Server;
use serde_json::json;

#[tokio::test]
async fn test_propose_balance_adjustments_success() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/v1beta/models/gemini-pro-latest:generateContent?key=fake-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": "{\"adjustments\":[{\"page\":1,\"line_on_page\":1,\"old_running_balance\":10.0,\"new_running_balance\":20.0,\"reason\":\"fix\",\"confidence\":0.9}],\"overall_strategy\":\"fix\",\"confidence\":0.95}"
                    }]
                }
            }]
        }).to_string())
        .create_async().await;

    let _app_config = AppConfig::default();
    let client = GeminiClient::with_base_url("fake-key".to_string(), server.url());

    let layout = DocumentLayout {
        total_pages: 1,
        ..Default::default()
    };

    let result = client.propose_balance_adjustments(&[], 10.0, &layout).await;

    assert!(result.is_ok());
    let plan = result.unwrap();
    assert_eq!(plan.confidence, 0.95);
    assert_eq!(plan.adjustments.len(), 1);

    mock.assert_async().await;
}

#[tokio::test]
async fn test_propose_balance_adjustments_low_confidence() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/v1beta/models/gemini-pro-latest:generateContent?key=fake-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": "{\"adjustments\":[],\"overall_strategy\":\"unsure\",\"confidence\":0.5}"
                    }]
                }
            }]
        }).to_string())
        .create_async().await;

    let _app_config = AppConfig::default();
    let client = GeminiClient::with_base_url("fake-key".to_string(), server.url());

    let layout = DocumentLayout {
        total_pages: 1,
        ..Default::default()
    };

    let result = client.propose_balance_adjustments(&[], 10.0, &layout).await;

    assert!(matches!(result, Err(GeminiError::LowConfidence(0.5))));

    mock.assert_async().await;
}

#[tokio::test]
async fn test_gemini_retry_on_429() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock(
            "POST",
            "/v1beta/models/gemini-pro-latest:generateContent?key=fake-key",
        )
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body("Too Many Requests")
        .expect_at_least(1)
        .create_async()
        .await;

    let mock_flash = server
        .mock(
            "POST",
            "/v1beta/models/gemini-flash-latest:generateContent?key=fake-key",
        )
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body("Too Many Requests")
        .expect_at_least(1)
        .create_async()
        .await;

    let client = GeminiClient::with_base_url("fake-key".to_string(), server.url());

    let layout = DocumentLayout {
        total_pages: 1,
        ..Default::default()
    };

    let result = client.propose_balance_adjustments(&[], 10.0, &layout).await;

    assert!(result.is_err());

    mock.assert_async().await;
    mock_flash.assert_async().await;
}
