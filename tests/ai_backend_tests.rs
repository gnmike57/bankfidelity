use dual_core_pdf_pipeline::ai::backend::AiBackend;
use dual_core_pdf_pipeline::ai::gemini_client::GeminiClient;
use dual_core_pdf_pipeline::ai::openai_client::OpenAiClient;
use dual_core_pdf_pipeline::app::config::AiProviderMode;
use mockito::Server;
use serde_json::json;

#[tokio::test]
async fn test_ai_backend_cascade_success_on_primary() {
    let mut server = Server::new_async().await;

    // The primary model (OpenRouter) should succeed immediately.
    let mock_or = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "choices": [{
                    "message": {
                        "content": "{\"is_math_consistent\": true}"
                    }
                }]
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    // Gemini and Groq should NOT be called.
    let mock_gemini = server
        .mock(
            "POST",
            "/v1beta/models/gemini-pro-latest:generateContent?key=gemini-key",
        )
        .with_status(500)
        .expect(0)
        .create_async()
        .await;

    let mut backend = AiBackend::new_mock();
    backend.primary = AiProviderMode::OpenRouterApiKey;
    backend.openrouter = Some(OpenAiClient::with_base_url(
        "or-key".to_string(),
        server.url(),
        "or-model".to_string(),
    ));
    backend.gemini = Some(GeminiClient::with_base_url(
        "gemini-key".to_string(),
        server.url(),
    ));
    backend.groq = Some(OpenAiClient::with_base_url(
        "groq-key".to_string(),
        server.url(),
        "groq-model".to_string(),
    ));

    let result = backend.verify_statement_mathematics("[]", 10.0).await;

    assert!(result.is_ok());
    assert!(result.unwrap());

    mock_or.assert_async().await;
    mock_gemini.assert_async().await;
}

#[tokio::test]
async fn test_ai_backend_cascade_fallback() {
    let mut server = Server::new_async().await;

    // The primary model (OpenRouter) fails with 500 Server Error
    let mock_or = server
        .mock("POST", "/chat/completions")
        .with_status(500)
        .expect(4) // 1 initial + 3 retries
        .create_async().await;

    // Cascade to Gemini, which succeeds!
    let mock_gemini = server
        .mock(
            "POST",
            "/v1beta/models/gemini-pro-latest:generateContent?key=gemini-key",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "candidates": [{
                    "content": {
                        "parts": [{
                            "text": "{\"is_mathematically_sound\": true}"
                        }]
                    }
                }]
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let mut backend = AiBackend::new_mock();
    backend.primary = AiProviderMode::OpenRouterApiKey;
    backend.openrouter = Some(OpenAiClient::with_base_url(
        "or-key".to_string(),
        server.url(),
        "or-model".to_string(),
    ));
    backend.gemini = Some(GeminiClient::with_base_url(
        "gemini-key".to_string(),
        server.url(),
    ));
    backend.groq = Some(OpenAiClient::with_base_url(
        "groq-key".to_string(),
        server.url(),
        "groq-model".to_string(),
    ));

    let result = backend.verify_statement_mathematics("[]", 10.0).await;

    assert!(result.is_ok());
    assert!(result.unwrap());

    mock_or.assert_async().await;
    mock_gemini.assert_async().await;
}

#[tokio::test]
async fn test_ai_backend_cascade_all_failed() {
    let mut server = Server::new_async().await;

    let mock_or = server
        .mock("POST", "/chat/completions")
        .with_status(500)
        .expect_at_least(1)
        .create_async()
        .await;

    let mock_gemini = server
        .mock(
            "POST",
            "/v1beta/models/gemini-pro-latest:generateContent?key=gemini-key",
        )
        .with_status(500)
        .expect_at_least(1)
        .create_async()
        .await;

    // We don't even need to mock groq if it uses the same URL as OR since mock_or matches all /chat/completions.
    // Wait, groq has the exact same path /chat/completions?
    // Let's just expect at least 1 hit on the /chat/completions mock.

    let mut backend = AiBackend::new_mock();
    backend.primary = AiProviderMode::OpenRouterApiKey;
    backend.openrouter = Some(OpenAiClient::with_base_url(
        "or-key".to_string(),
        server.url(),
        "or-model".to_string(),
    ));
    backend.gemini = Some(GeminiClient::with_base_url(
        "gemini-key".to_string(),
        server.url(),
    ));
    backend.groq = Some(OpenAiClient::with_base_url(
        "groq-key".to_string(),
        server.url(),
        "groq-model".to_string(),
    ));

    let result = backend.verify_statement_mathematics("[]", 10.0).await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("all failed"));

    mock_or.assert_async().await;
    mock_gemini.assert_async().await;
}
