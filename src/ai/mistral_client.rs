//! Mistral AI Platform Client (Specialist Model 5).
//!
//! Direct REST integration with the official Mistral AI API
//! (https://api.mistral.ai/v1/chat/completions) for financial ledger reasoning,
//! format adaptation, and mathematical balance verification.
//!
//! Excludes OpenAI, Anthropic, Gemini, Qwen, Groq, and DeepSeek.

use crate::app::config::AppConfig;
use crate::engine::model::Transaction;
use crate::engine::transfer::TransferPlan;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum MistralError {
    #[error("Missing MISTRAL_API_KEY")]
    MissingKey,
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Middleware error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("Mistral API error (HTTP {0}): {1}")]
    Api(StatusCode, String),
    #[error("Invalid JSON response from Mistral: {0}")]
    InvalidResponse(String),
    #[error("Formatting error: {0}")]
    Format(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MistralBalancePlan {
    pub adjustments: Vec<MistralBalanceAdjustment>,
    pub overall_strategy: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MistralBalanceAdjustment {
    pub page: usize,
    pub line_on_page: usize,
    pub old_running_balance: f64,
    pub new_running_balance: f64,
    pub reason: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MistralCompletenessReport {
    pub completeness_score: f32,
    pub notes: String,
    pub missing_rows: Vec<String>,
    pub math_consistent: bool,
}

pub struct MistralClient {
    pub api_key: String,
    pub http: reqwest_middleware::ClientWithMiddleware,
    pub base_url: String,
    pub model: String,
}

impl MistralClient {
    pub fn from_app_config(cfg: &AppConfig) -> Result<Self, MistralError> {
        let api_key = cfg
            .mistral_api_key
            .clone()
            .or_else(|| std::env::var("MISTRAL_API_KEY").ok())
            .ok_or(MistralError::MissingKey)?;

        let model = if cfg.mistral_model.is_empty() {
            "mistral-large-latest".to_string()
        } else {
            cfg.mistral_model.clone()
        };

        Ok(Self {
            api_key,
            http: crate::app::config::global_http_client(),
            base_url: "https://api.mistral.ai/v1".to_string(),
            model,
        })
    }

    pub async fn from_app_config_async(cfg: &AppConfig) -> Result<Self, MistralError> {
        Self::from_app_config(cfg)
    }

    pub async fn ping(&self) -> Result<(), MistralError> {
        let url = format!("{}/models", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(self.api_key.trim())
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let s = resp.status();
            let b = resp.text().await.unwrap_or_default();
            Err(MistralError::Api(s, b))
        }
    }

    async fn post_json(&self, sys: &str, user: &str) -> Result<String, MistralError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": sys },
                { "role": "user", "content": user }
            ],
            "response_format": { "type": "json_object" },
            "temperature": 0.0
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(self.api_key.trim())
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(MistralError::Api(status, text));
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| MistralError::Format(e.to_string()))?;
        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| MistralError::Format("No content returned in choices[0]".to_string()))?
            .to_string();

        Ok(content)
    }

    pub async fn propose_balance_adjustments(
        &self,
        transactions: &[Transaction],
        imbalance: f64,
        _layout: &crate::engine::layout::DocumentLayout,
    ) -> Result<crate::ai::gemini_client::GeminiBalancePlan, MistralError> {
        let sys = "You are a mathematical auditor. Analyze bank transactions for discrepancy and return a strict JSON object with 'adjustments' array (each with page, line_on_page, old_running_balance, new_running_balance, reason, confidence), 'overall_strategy' string, and 'confidence' float.";
        let tx_json =
            serde_json::to_string(transactions).map_err(|e| MistralError::Format(e.to_string()))?;
        let user = format!("Imbalance: {}\nTransactions: {}", imbalance, tx_json);

        let out = self.post_json(sys, &user).await?;
        let plan: crate::ai::gemini_client::GeminiBalancePlan =
            serde_json::from_str(&out).map_err(|e| MistralError::Format(e.to_string()))?;
        Ok(plan)
    }

    pub async fn validate_parse_completeness(
        &self,
        transactions: &[Transaction],
        opening: f64,
        closing: f64,
        pages: usize,
    ) -> Result<crate::ai::gemini_client::GeminiCompletenessReport, MistralError> {
        let sys = "You are a completion validator. Check if transactions list mathematically bridges opening and closing. Return JSON: { \"completeness_score\": 0.9, \"notes\": \"Looks good\", \"missing_rows\": [], \"math_consistent\": true }";
        let user = format!(
            "Op: {}, Cl: {}, Pages: {}, Txs: {}",
            opening,
            closing,
            pages,
            serde_json::to_string(transactions).map_err(|e| MistralError::Format(e.to_string()))?
        );
        let out = self.post_json(sys, &user).await?;
        let plan: crate::ai::gemini_client::GeminiCompletenessReport =
            serde_json::from_str(&out).map_err(|e| MistralError::Format(e.to_string()))?;
        Ok(plan)
    }

    pub async fn plan_transaction_transfer(
        &self,
        source_transactions: &[Transaction],
        target_transactions: &[Transaction],
        correction_hint: Option<&str>,
    ) -> Result<TransferPlan, MistralError> {
        let scrubbed_source = crate::ai::gemini_client::scrub_pii(source_transactions);
        let scrubbed_target = crate::ai::gemini_client::scrub_pii(target_transactions);

        let sys = "You are an expert financial document specialist. You plan how to transfer transactions \
                   from a SOURCE bank statement to a TARGET bank statement.\n\
                   Map columns, convert date formats, and adapt descriptions to match target bank styles.\n\
                   Return JSON: { mappings: [{source_index, target_page, target_line, converted_date, adapted_description}], \
                   output_page_count, pages_to_clone, pages_to_remove, strategy, confidence }";

        let user = format!(
            "SOURCE ({} rows):\n{}\n\nTARGET ({} rows):\n{}",
            source_transactions.len(),
            serde_json::to_string(&scrubbed_source).unwrap_or_default(),
            target_transactions.len(),
            serde_json::to_string(&scrubbed_target).unwrap_or_default(),
        );

        let prompt = if let Some(hint) = correction_hint {
            format!("{user}\n\nCORRECTION HINT:\n{hint}")
        } else {
            user
        };

        let out = self.post_json(sys, &prompt).await?;
        let plan: TransferPlan =
            serde_json::from_str(&out).map_err(|e| MistralError::Format(e.to_string()))?;
        Ok(plan)
    }

    pub async fn apply_natural_language_edit(
        &self,
        instructions: &str,
        current_transactions: &[Transaction],
    ) -> Result<Vec<Transaction>, MistralError> {
        let sys = "You are a financial statement editor. You receive a list of bank transactions and an instruction.\
Apply the requested changes (edit descriptions, amounts, dates, or add/remove transactions).\
Return JSON: { \"transactions\": [ ... updated transactions array with exact Transaction schema ... ] }";

        let user = format!(
            "Instruction: {}\nCurrent Transactions:\n{}",
            instructions,
            serde_json::to_string(current_transactions)
                .map_err(|e| MistralError::Format(e.to_string()))?
        );

        let out = self.post_json(sys, &user).await?;
        let parsed: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| MistralError::Format(e.to_string()))?;

        let txs: Vec<Transaction> = serde_json::from_value(
            parsed
                .get("transactions")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .map_err(|e| MistralError::Format(e.to_string()))?;

        Ok(txs)
    }

    pub async fn verify_statement_mathematics(
        &self,
        transactions_json: &str,
        opening: f64,
    ) -> Result<bool, MistralError> {
        let sys = "You are a mathematical auditor. Double-check if the bank statement's math adds up. Return JSON: { \"is_math_consistent\": true }";
        let user = format!("Op: {}, Txs: {}", opening, transactions_json);
        let out = self.post_json(sys, &user).await?;
        let parsed: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| MistralError::Format(e.to_string()))?;
        Ok(parsed["is_math_consistent"].as_bool().unwrap_or(false))
    }

    pub async fn verify_transfer_math(
        &self,
        mapped_transactions: &[crate::engine::transfer::MappedTransaction],
        opening_balance: rust_decimal::Decimal,
    ) -> Result<bool, MistralError> {
        let mut running = opening_balance;
        for tx in mapped_transactions {
            if let Some(d) = tx.debit {
                running += d;
            }
            if let Some(c) = tx.credit {
                running -= c;
            }
            if running != tx.running_balance {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub async fn repair_extracted_transactions(
        &self,
        transactions: &[Transaction],
        opening_balance: rust_decimal::Decimal,
        closing_balance: rust_decimal::Decimal,
        raw_ocr_text: &str,
        error_message: &str,
    ) -> Result<Vec<Transaction>, MistralError> {
        let sys = "You are a financial OCR correction specialist. Fix discrepancies in transaction amounts to balance opening and closing balances. Return JSON: { \"transactions\": [ ... ] }";
        let user = format!(
            "Op: {}, Cl: {}\nError: {}\nRaw: {}\nTxs: {}",
            opening_balance,
            closing_balance,
            error_message,
            raw_ocr_text,
            serde_json::to_string(transactions).unwrap_or_default()
        );
        let out = self.post_json(sys, &user).await?;
        let parsed: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| MistralError::Format(e.to_string()))?;
        let repaired: Vec<Transaction> = serde_json::from_value(
            parsed
                .get("transactions")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )
        .map_err(|e| MistralError::Format(e.to_string()))?;
        Ok(repaired)
    }
}
