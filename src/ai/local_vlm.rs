//! Local vision-language model client (OpenAI-compatible, e.g. vLLM/SGLang on
//! a GX10). Reads a rendered statement page and returns candidate rows as
//! **strings only**.
//!
//! This is additive evidence, never authoritative: rows are cross-checked
//! against the deterministic offline parser and the exact-decimal ledger
//! gates, so no numeric interpretation happens here.

use base64::Engine as _;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ai::local_llm::LocalEndpointConfig;

const DEFAULT_VLM_URL: &str = "http://127.0.0.1:8000/v1";
const VLM_TIMEOUT_SECS: u64 = 300;

#[derive(thiserror::Error, Debug)]
pub enum LocalVlmError {
    #[error("Local VLM not configured (set LOCAL_VLM_URL and LOCAL_VLM_MODEL)")]
    NotConfigured,
    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("API Error (HTTP {0}): {1}")]
    Api(StatusCode, String),
    #[error("Invalid Response: {0}")]
    InvalidResponse(String),
}

/// One candidate ledger row exactly as the model read it (no normalisation).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VlmRow {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub debit: Option<String>,
    #[serde(default)]
    pub credit: Option<String>,
    #[serde(default)]
    pub balance: Option<String>,
}

pub struct LocalVlmClient {
    http: reqwest::Client,
    cfg: LocalEndpointConfig,
}

/// Cheap local check (no network): both URL and model are configured.
pub fn is_configured() -> bool {
    LocalVlmClient::config_from_env().is_some()
}

impl LocalVlmClient {
    fn config_from_env() -> Option<LocalEndpointConfig> {
        let url_set = std::env::var("LOCAL_VLM_URL").is_ok_and(|v| !v.trim().is_empty());
        let cfg = LocalEndpointConfig::from_env("LOCAL_VLM", DEFAULT_VLM_URL, "");
        (url_set && !cfg.model.is_empty()).then_some(cfg)
    }

    pub fn from_env() -> Result<Self, LocalVlmError> {
        let mut cfg = Self::config_from_env().ok_or(LocalVlmError::NotConfigured)?;
        if std::env::var("LOCAL_VLM_TIMEOUT_SECS").is_err() {
            cfg.timeout_secs = VLM_TIMEOUT_SECS;
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .build()?;
        Ok(Self { http, cfg })
    }

    /// Ask the model to transcribe the ledger rows on one rendered page (PNG).
    pub async fn extract_page_rows(&self, png: &[u8]) -> Result<Vec<VlmRow>, LocalVlmError> {
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        );
        let body = json!({
            "model": self.cfg.model,
            "temperature": 0.0,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": PROMPT },
                    { "type": "image_url", "image_url": { "url": data_url } }
                ]
            }]
        });
        let mut req = self
            .http
            .post(format!("{}/chat/completions", self.cfg.base_url))
            .json(&body);
        if let Some(key) = &self.cfg.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(LocalVlmError::Api(
                status,
                resp.text().await.unwrap_or_default(),
            ));
        }
        let v: serde_json::Value = resp.json().await?;
        let text = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LocalVlmError::InvalidResponse("missing message content".into()))?;
        parse_rows(text)
    }
}

const PROMPT: &str = include_str!("../../assets/vlm_prompt.txt");

/// Strict parse of the model reply: a JSON array, optionally in a code fence.
pub fn parse_rows(text: &str) -> Result<Vec<VlmRow>, LocalVlmError> {
    let t = text.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    serde_json::from_str(t)
        .map_err(|e| LocalVlmError::InvalidResponse(format!("row JSON did not parse: {e}")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn parses_plain_and_fenced_arrays() {
        let raw = r#"[{"date":"01 Jan","description":"Coffee","debit":"4.50","credit":null,"balance":"95.50"}]"#;
        let a = parse_rows(raw).unwrap();
        assert_eq!(a[0].debit.as_deref(), Some("4.50"));
        let b = parse_rows(&format!("```json\n{raw}\n```")).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_non_array_replies() {
        assert!(parse_rows("Sure! Here are the rows").is_err());
        assert!(parse_rows(r#"{"rows":[]}"#).is_err());
    }
}
