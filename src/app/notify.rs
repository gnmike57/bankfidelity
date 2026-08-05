//! Outbound notifications: fire-and-forget webhooks for audit/observability.
//!
//! The runtime invokes this on each successful edit so external systems
//! (Slack, Discord, n8n, custom audit collectors) can react in real time.
//! All failures are logged at WARN and never propagate.

use serde_json::json;

#[derive(Debug, Clone)]
pub struct WebhookPayload<'a> {
    pub event: &'a str,
    pub page: usize,
    pub old_text: &'a str,
    pub new_text: &'a str,
    pub description: &'a str,
}

pub async fn send_webhook(url: &str, payload: WebhookPayload<'_>) {
    if url.trim().is_empty() {
        return;
    }
    let body = json!({
        "event": payload.event,
        "page": payload.page,
        "old_text": payload.old_text,
        "new_text": payload.new_text,
        "description": payload.description,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("[webhook] build failed: {}", e);
            return;
        }
    };

    match client.post(url).json(&body).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                tracing::info!(
                    "[webhook] delivered ({}) -> {}",
                    payload.event,
                    resp.status()
                );
            } else {
                tracing::warn!("[webhook] non-success status: {}", resp.status());
            }
        }
        Err(e) => tracing::warn!("[webhook] post failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_url() {
        let payload = WebhookPayload {
            event: "test_event",
            page: 1,
            old_text: "old",
            new_text: "new",
            description: "test desc",
        };
        // Should return immediately without doing anything.
        send_webhook("", payload.clone()).await;
        send_webhook("   ", payload).await;
    }
}
