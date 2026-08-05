use base64::{engine::general_purpose, Engine as _};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::fs;
use std::time::Duration;

#[derive(Debug, Serialize)]
struct VisionRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: Vec<ContentItem>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentItem {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Debug, Deserialize)]
struct VisionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct VisionDecision {
    passed: bool,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisionVerificationOutcome {
    Passed(String),
    Rejected(String),
    Unavailable(String),
}

fn parse_decision(content: &str) -> Result<VisionDecision, String> {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    serde_json::from_str(json).map_err(|error| format!("invalid Vision AI decision JSON: {error}"))
}

pub async fn verify_with_vision(
    api_key: &str,
    orig_img_path: &str,
    edit_img_path: &str,
) -> VisionVerificationOutcome {
    verify_with_vision_endpoint(
        api_key,
        orig_img_path,
        edit_img_path,
        "https://api.openai.com/v1/chat/completions",
        Duration::from_secs(20),
    )
    .await
}

async fn verify_with_vision_endpoint(
    api_key: &str,
    orig_img_path: &str,
    edit_img_path: &str,
    endpoint: &str,
    timeout: Duration,
) -> VisionVerificationOutcome {
    if api_key.trim().is_empty() {
        return VisionVerificationOutcome::Unavailable(
            "Vision AI provider was requested without a configured key".into(),
        );
    }
    let orig_b64 = match encode_image(orig_img_path) {
        Ok(image) => image,
        Err(error) => return VisionVerificationOutcome::Unavailable(error),
    };
    let edit_b64 = match encode_image(edit_img_path) {
        Ok(image) => image,
        Err(error) => return VisionVerificationOutcome::Unavailable(error),
    };

    let client = match reqwest::Client::builder()
        .connect_timeout(timeout.min(Duration::from_secs(5)))
        .timeout(timeout)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return VisionVerificationOutcome::Unavailable(format!(
                "Vision AI client initialization failed: {error}"
            ));
        }
    };
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let authorization = match HeaderValue::from_str(&format!("Bearer {api_key}")) {
        Ok(value) => value,
        Err(error) => {
            return VisionVerificationOutcome::Unavailable(format!(
                "invalid Vision AI authorization header: {error}"
            ));
        }
    };
    headers.insert(AUTHORIZATION, authorization);

    let prompt = "You are a financial document auditor. I will provide an original page and an edited page. \
                  Are there any semantic differences or corrupted visual artifacts in the edited page that suggest the layout is broken? \
                  Ignore intended textual edits to balances or dates. Reply STRICTLY with a JSON object: {\"passed\": true/false, \"reason\": \"...\"}";

    let req_body = VisionRequest {
        model: "gpt-4o".to_string(), // Or Claude if using Anthropic API format
        max_tokens: 300,
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![
                ContentItem::Text {
                    text: prompt.to_string(),
                },
                ContentItem::ImageUrl {
                    image_url: ImageUrl {
                        url: format!("data:image/png;base64,{}", orig_b64),
                    },
                },
                ContentItem::ImageUrl {
                    image_url: ImageUrl {
                        url: format!("data:image/png;base64,{}", edit_b64),
                    },
                },
            ],
        }],
    };

    let response = match client
        .post(endpoint)
        .headers(headers)
        .json(&req_body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return VisionVerificationOutcome::Unavailable(format!(
                "Vision AI request failed: {error}"
            ));
        }
    };
    let response = match response.error_for_status() {
        Ok(response) => response,
        Err(error) => {
            return VisionVerificationOutcome::Unavailable(format!(
                "Vision AI returned an error status: {error}"
            ));
        }
    };
    let parsed = match response.json::<VisionResponse>().await {
        Ok(response) => response,
        Err(error) => {
            return VisionVerificationOutcome::Unavailable(format!(
                "Vision AI response decoding failed: {error}"
            ));
        }
    };
    let Some(choice) = parsed.choices.first() else {
        return VisionVerificationOutcome::Unavailable(
            "Vision AI response contained no choices".into(),
        );
    };
    match parse_decision(&choice.message.content) {
        Ok(decision) if decision.passed => VisionVerificationOutcome::Passed(decision.reason),
        Ok(decision) => VisionVerificationOutcome::Rejected(decision.reason),
        Err(error) => VisionVerificationOutcome::Unavailable(error),
    }
}

fn encode_image(path: &str) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read Vision AI image {path}: {error}"))?;
    if bytes.is_empty() {
        return Err(format!("Vision AI image is empty: {path}"));
    }
    Ok(general_purpose::STANDARD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn image_pair() -> (tempfile::TempDir, String, String) {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original.png");
        let edited = directory.path().join("edited.png");
        std::fs::write(&original, b"original-image-bytes").unwrap();
        std::fs::write(&edited, b"edited-image-bytes").unwrap();
        (
            directory,
            original.to_string_lossy().into_owned(),
            edited.to_string_lossy().into_owned(),
        )
    }

    #[test]
    fn parses_strict_and_fenced_decisions() {
        let passed = parse_decision(r#"{"passed":true,"reason":"ok"}"#).unwrap();
        assert!(passed.passed);
        let rejected =
            parse_decision("```json\n{\"passed\":false,\"reason\":\"drift\"}\n```").unwrap();
        assert!(!rejected.passed);
        assert_eq!(rejected.reason, "drift");
    }

    #[test]
    fn rejects_unstructured_provider_text() {
        assert!(parse_decision("probably fine").is_err());
        assert!(parse_decision(r#"{"passed":"yes"}"#).is_err());
        assert!(parse_decision(r#"{"reason":"missing disposition"}"#).is_err());
    }

    #[tokio::test]
    async fn missing_key_is_explicitly_unavailable_without_reading_files() {
        let outcome = verify_with_vision("  ", "missing-original.png", "missing-edited.png").await;
        assert!(matches!(
            outcome,
            VisionVerificationOutcome::Unavailable(message)
                if message.contains("without a configured key")
        ));
    }

    #[tokio::test]
    async fn provider_rejection_is_explicit_and_request_omits_local_paths() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "{\"passed\":false,\"reason\":\"layout drift\"}"}}]
            })))
            .mount(&server)
            .await;
        let (_directory, original, edited) = image_pair();
        let outcome = verify_with_vision_endpoint(
            "test-key",
            &original,
            &edited,
            &server.uri(),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(
            outcome,
            VisionVerificationOutcome::Rejected("layout drift".into())
        );
        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        assert!(!body.contains(&original));
        assert!(!body.contains(&edited));
    }

    #[tokio::test]
    async fn malformed_provider_response_is_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "probably fine"}}]
            })))
            .mount(&server)
            .await;
        let (_directory, original, edited) = image_pair();
        let outcome = verify_with_vision_endpoint(
            "test-key",
            &original,
            &edited,
            &server.uri(),
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(
            outcome,
            VisionVerificationOutcome::Unavailable(message)
                if message.contains("invalid Vision AI decision JSON")
        ));
    }

    #[tokio::test]
    async fn provider_timeout_is_unavailable() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(200))
                    .set_body_json(serde_json::json!({
                        "choices": [{"message": {"content": "{\"passed\":true,\"reason\":\"ok\"}"}}]
                    })),
            )
            .mount(&server)
            .await;
        let (_directory, original, edited) = image_pair();
        let outcome = verify_with_vision_endpoint(
            "test-key",
            &original,
            &edited,
            &server.uri(),
            Duration::from_millis(20),
        )
        .await;
        assert!(matches!(
            outcome,
            VisionVerificationOutcome::Unavailable(message)
                if message.contains("request failed")
        ));
    }
}
