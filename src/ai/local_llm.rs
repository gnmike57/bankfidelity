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

pub struct LocalLlmClient {
    pub http: reqwest_middleware::ClientWithMiddleware,
    pub base_url: String,
}

impl Default for LocalLlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalLlmClient {
    pub fn new() -> Self {
        let reqwest_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let http = reqwest_middleware::ClientBuilder::new(reqwest_client).build();

        Self {
            http,
            base_url: "http://127.0.0.1:11434/v1".to_string(),
        }
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
            "model": "qwen2.5-coder-7b-instruct-q4_k_m",
            "messages": [
                { "role": "system", "content": "You are a local forensic accounting assistant." },
                { "role": "user", "content": prompt }
            ],
            "temperature": 0.2
        });

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(LocalLlmError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let json_resp: serde_json::Value = resp.json().await?;
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
            "model": "qwen2.5-coder-7b-instruct-q4_k_m",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a local financial AI orchestrator. Apply the user's natural language edit precisely to the transactions. If amounts change, strictly cascade the running balance. Return ONLY valid JSON."
                },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": 0.1
        });

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(LocalLlmError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let json_resp: serde_json::Value = resp.json().await?;
        let text = json_resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("[]");
        
        let cleaned = text.trim();
        let cleaned = if cleaned.starts_with("```json") {
            cleaned.trim_start_matches("```json").trim_end_matches("```").trim()
        } else {
            cleaned
        };

        serde_json::from_str(cleaned).map_err(|e| LocalLlmError::InvalidResponse(format!("Failed to parse JSON: {}", e)))
    }
}
