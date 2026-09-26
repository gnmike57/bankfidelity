use reqwest::StatusCode;
use serde_json::json;

#[derive(thiserror::Error, Debug)]
pub enum LocalLlmError {
    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Middleware Error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("API Error (HTTP {0}): {1}")]
    Api(StatusCode, String),
    #[error("Invalid Response: {0}")]
    InvalidResponse(String),
}

/// Default OpenAI-compatible endpoint (Ollama's port; llama-server, vLLM and
/// SGLang can all bind there too).
pub const DEFAULT_LLM_URL: &str = "http://127.0.0.1:11434/v1";
/// Historical default model tag, kept so existing Ollama/llama-server setups
/// keep working with zero configuration.
pub const DEFAULT_LLM_MODEL: &str = "qwen2.5-coder-7b-instruct-q4_k_m";
const DEFAULT_TIMEOUT_SECS: u64 = 90;

/// Connection settings for an OpenAI-compatible local inference server.
///
/// Resolved from `<PREFIX>_URL`, `<PREFIX>_MODEL`, `<PREFIX>_API_KEY` and
/// `<PREFIX>_TIMEOUT_SECS` (e.g. `LOCAL_LLM_*` / `LOCAL_VLM_*`), so the same
/// binary can target Ollama on a laptop or vLLM on a GX10 without a rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEndpointConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
}

impl LocalEndpointConfig {
    /// Resolve from the process environment.
    pub fn from_env(prefix: &str, default_url: &str, default_model: &str) -> Self {
        Self::from_lookup(prefix, default_url, default_model, |name| {
            std::env::var(name).ok()
        })
    }

    /// Resolve using an arbitrary lookup (pure; used by tests).
    pub fn from_lookup(
        prefix: &str,
        default_url: &str,
        default_model: &str,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let get = |suffix: &str| {
            lookup(&format!("{prefix}_{suffix}"))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Self {
            base_url: get("URL")
                .unwrap_or_else(|| default_url.to_string())
                .trim_end_matches('/')
                .to_string(),
            model: get("MODEL").unwrap_or_else(|| default_model.to_string()),
            api_key: get("API_KEY"),
            timeout_secs: get("TIMEOUT_SECS")
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
        }
    }
}

pub struct LocalLlmClient {
    pub http: reqwest_middleware::ClientWithMiddleware,
    pub base_url: String,
    pub model: String,
    api_key: Option<String>,
}

impl Default for LocalLlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalLlmClient {
    pub fn new() -> Self {
        Self::with_config(LocalEndpointConfig::from_env(
            "LOCAL_LLM",
            DEFAULT_LLM_URL,
            DEFAULT_LLM_MODEL,
        ))
    }

    pub fn with_config(cfg: LocalEndpointConfig) -> Self {
        let reqwest_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let http = reqwest_middleware::ClientBuilder::new(reqwest_client).build();
        Self {
            http,
            base_url: cfg.base_url,
            model: cfg.model,
            api_key: cfg.api_key,
        }
    }

    async fn post_chat(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, LocalLlmError> {
        let mut req = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(LocalLlmError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(resp.json().await?)
    }

    pub async fn explain_imbalance(
        &self,
        transactions_json: &str,
        opening_balance: f64,
        closing_balance: f64,
        imbalance: f64,
    ) -> Result<String, LocalLlmError> {
        let prompt = format!(
            "You are a helpful, local forensic accounting AI embedded in the BankStatementFidelity editor.\n\
             The user's bank statement has a mathematical imbalance of ${:.2}.\n\
             Opening Balance: ${:.2}\n\
             Closing Balance: ${:.2}\n\
             Transactions:\n{}\n\n\
             Please explain briefly and clearly why the math doesn't add up and what the user should check. Keep it concise.",
             imbalance, opening_balance, closing_balance, transactions_json
        );

        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": "You are a local forensic accounting assistant." },
                { "role": "user", "content": prompt }
            ],
            "temperature": 0.2
        });

        let json_resp = self.post_chat(&body).await?;
        let text = json_resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("No explanation returned.")
            .to_string();

        Ok(text)
    }

    pub async fn apply_natural_language_edit(
        &self,
        prompt: &str,
        transactions: &[crate::engine::model::Transaction],
    ) -> Result<Vec<crate::engine::model::Transaction>, LocalLlmError> {
        let txs_json = serde_json::to_string(transactions).unwrap_or_default();
        let user_prompt = format!(
            "Instruction: {}\n\nTransactions (JSON):\n{}\n\nReturn ONLY the modified transactions as a valid JSON array matching the input structure. Do not include markdown formatting or commentary.",
            prompt, txs_json
        );

        let body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a local financial AI orchestrator. Apply the user's natural language edit precisely to the transactions. If amounts change, strictly cascade the running balance. Return ONLY valid JSON."
                },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": 0.1
        });

        let json_resp = self.post_chat(&body).await?;
        let text = json_resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("[]");

        let cleaned = text.trim();
        let cleaned = if cleaned.starts_with("```json") {
            cleaned
                .trim_start_matches("```json")
                .trim_end_matches("```")
                .trim()
        } else {
            cleaned
        };

        serde_json::from_str(cleaned)
            .map_err(|e| LocalLlmError::InvalidResponse(format!("Failed to parse JSON: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::collections::HashMap;

    fn cfg(env: &[(&str, &str)]) -> LocalEndpointConfig {
        let map: HashMap<String, String> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        LocalEndpointConfig::from_lookup("LOCAL_LLM", DEFAULT_LLM_URL, DEFAULT_LLM_MODEL, |k| {
            map.get(k).cloned()
        })
    }

    #[test]
    fn defaults_preserve_legacy_behaviour() {
        let c = cfg(&[]);
        assert_eq!(c.base_url, DEFAULT_LLM_URL);
        assert_eq!(c.model, DEFAULT_LLM_MODEL);
        assert_eq!(c.api_key, None);
        assert_eq!(c.timeout_secs, 90);
    }

    #[test]
    fn overrides_are_trimmed_and_validated() {
        let c = cfg(&[
            ("LOCAL_LLM_URL", " http://gx10.local:8000/v1/ "),
            ("LOCAL_LLM_MODEL", "Qwen/Qwen3-30B-A3B"),
            ("LOCAL_LLM_API_KEY", "  "),
            ("LOCAL_LLM_TIMEOUT_SECS", "0"),
        ]);
        assert_eq!(c.base_url, "http://gx10.local:8000/v1");
        assert_eq!(c.model, "Qwen/Qwen3-30B-A3B");
        assert_eq!(c.api_key, None, "blank key must read as unset");
        assert_eq!(c.timeout_secs, 90, "zero timeout must fall back");
    }
}
