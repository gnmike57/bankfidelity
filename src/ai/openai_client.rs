use crate::app::config::{AiProviderMode, AppConfig};
use crate::engine::model::Transaction;
use crate::engine::transfer::TransferPlan;
use reqwest::StatusCode;

#[derive(thiserror::Error, Debug)]
pub enum OpenAiError {
    #[error("Missing API Key")]
    MissingKey,
    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Middleware Error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("API Error (HTTP {0}): {1}")]
    Api(StatusCode, String),
    #[error("Invalid Response: {0}")]
    InvalidResponse(String),
    #[error("Format error: {0}")]
    Format(String),
}

pub struct OpenAiClient {
    pub api_key: String,
    pub http: reqwest_middleware::ClientWithMiddleware,
    pub base_url: String,
    pub model: String,
}

impl OpenAiClient {
    pub fn from_app_config(cfg: &AppConfig) -> Result<Self, OpenAiError> {
        let (api_key, base_url, model) = match cfg.ai_provider {
            AiProviderMode::GroqApiKey => {
                let k = cfg.groq_api_key.clone().ok_or(OpenAiError::MissingKey)?;
                (
                    k,
                    "https://api.groq.com/openai/v1".to_string(),
                    "llama-3.3-70b-versatile".to_string(),
                )
            }
            AiProviderMode::OpenRouterApiKey => {
                let k = cfg
                    .openrouter_api_key
                    .clone()
                    .ok_or(OpenAiError::MissingKey)?;
                (
                    k,
                    "https://openrouter.ai/api/v1".to_string(),
                    cfg.openrouter_model.clone(),
                )
            }
            AiProviderMode::MistralApiKey => {
                let k = cfg.mistral_api_key.clone().ok_or(OpenAiError::MissingKey)?;
                (
                    k,
                    "https://api.mistral.ai/v1".to_string(),
                    cfg.mistral_model.clone(),
                )
            }
            _ => return Err(OpenAiError::MissingKey),
        };
        Ok(Self {
            api_key,
            http: crate::app::config::global_http_client(),
            base_url,
            model,
        })
    }

    pub fn with_base_url(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            http: crate::app::config::global_http_client(),
            base_url,
            model,
        }
    }

    pub async fn from_app_config_async(cfg: &AppConfig) -> Result<Self, OpenAiError> {
        Self::from_app_config(cfg)
    }

    pub async fn ping(&self) -> Result<(), OpenAiError> {
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
            Err(OpenAiError::Api(s, b))
        }
    }

    async fn post_json(&self, sys: &str, user: &str) -> Result<String, OpenAiError> {
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
            return Err(OpenAiError::Api(status, text));
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| OpenAiError::Format(e.to_string()))?;
        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| OpenAiError::Format("No content returned".to_string()))?
            .to_string();

        Ok(content)
    }

    pub async fn propose_balance_adjustments(
        &self,
        transactions: &[Transaction],
        imbalance: f64,
        _layout: &crate::engine::layout::DocumentLayout,
    ) -> Result<crate::ai::gemini_client::GeminiBalancePlan, OpenAiError> {
        let sys = "You are a mathematical auditor. You receive a JSON list of bank transactions. Identify OCR errors and return a JSON object containing an 'adjustments' array, 'overall_strategy' string, and 'confidence' number (0.0 to 1.0). Each adjustment needs 'page', 'line_on_page', 'old_running_balance', 'new_running_balance', 'reason', 'confidence'.";

        let tx_json =
            serde_json::to_string(transactions).map_err(|e| OpenAiError::Format(e.to_string()))?;
        let user = format!("Imbalance: {}\nTransactions: {}", imbalance, tx_json);

        let out = self.post_json(sys, &user).await?;
        let plan: crate::ai::gemini_client::GeminiBalancePlan =
            serde_json::from_str(&out).map_err(|e| OpenAiError::Format(e.to_string()))?;
        Ok(plan)
    }

    pub async fn validate_parse_completeness(
        &self,
        transactions: &[Transaction],
        opening: f64,
        closing: f64,
        pages: usize,
    ) -> Result<crate::ai::gemini_client::GeminiCompletenessReport, OpenAiError> {
        let sys = "You are a completion validator. Check if transactions list mathematically bridges opening and closing. Return JSON: { \"completeness_score\": 0.9, \"notes\": \"Looks good\", \"missing_rows\": [], \"math_consistent\": true }";
        let user = format!(
            "Op: {}, Cl: {}, Pages: {}, Txs: {}",
            opening,
            closing,
            pages,
            serde_json::to_string(transactions).map_err(|e| OpenAiError::Format(e.to_string()))?
        );
        let out = self.post_json(sys, &user).await?;
        let plan: crate::ai::gemini_client::GeminiCompletenessReport =
            serde_json::from_str(&out).map_err(|e| OpenAiError::Format(e.to_string()))?;
        Ok(plan)
    }

    pub async fn verify_statement_mathematics(
        &self,
        transactions_json: &str,
        opening: f64,
    ) -> Result<bool, OpenAiError> {
        let sys = "You are a mathematical auditor. Double-check if the bank statement's math adds up. Return JSON: { \"is_math_consistent\": true }";
        let user = format!("Op: {}, Txs: {}", opening, transactions_json);
        let out = self.post_json(sys, &user).await?;
        let parsed: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| OpenAiError::Format(e.to_string()))?;
        Ok(parsed["is_math_consistent"].as_bool().unwrap_or(false))
    }

    pub async fn plan_transaction_transfer(
        &self,
        source_transactions: &[Transaction],
        target_transactions: &[Transaction],
        correction_hint: Option<&str>,
    ) -> Result<TransferPlan, OpenAiError> {
        let scrubbed_source = crate::ai::gemini_client::scrub_pii(source_transactions);
        let scrubbed_target = crate::ai::gemini_client::scrub_pii(target_transactions);

        let sys = "You are an expert financial document analyst. You need to plan how to transfer \
             transactions from a SOURCE bank statement to a TARGET bank statement.\n\n\
             Analyze both formats (date style, number format, description conventions, \
             column layout) and produce a transfer plan. For each source transaction, \
             specify which target page and line it should land on. Convert dates to the \
             target's format. Adapt descriptions to match the target's style. \
             If the source has more transactions than the target's pages can hold, \
             specify pages_to_clone (which target page to duplicate for overflow). \
             If the source has fewer, specify pages_to_remove. \
             Each mapping must reference a source_index (0-based into the source list).\n\n\
             Return a JSON object with these exact fields:\n\
             - mappings: array of { source_index: int, target_page: int, target_line: int, converted_date: string, adapted_description: string }\n\
             - output_page_count: int\n\
             - pages_to_clone: array of ints\n\
             - pages_to_remove: array of ints\n\
             - strategy: string\n\
             - confidence: number (0.0 to 1.0)".to_string();

        let user = format!(
            "SOURCE statement transactions ({} rows):\n{}\n\nTARGET statement transactions ({} rows):\n{}",
            source_transactions.len(),
            serde_json::to_string(&scrubbed_source).unwrap_or_default(),
            target_transactions.len(),
            serde_json::to_string(&scrubbed_target).unwrap_or_default(),
        );

        let prompt = if let Some(hint) = correction_hint {
            format!(
                "{}\n\nCRITICAL CORRECTION HINT from previous failed attempt:\n{hint}\n\nPlease adjust your plan to resolve this error.",
                user
            )
        } else {
            user
        };

        let out = self.post_json(&sys, &prompt).await?;
        let plan: TransferPlan =
            serde_json::from_str(&out).map_err(|e| OpenAiError::Format(e.to_string()))?;

        if plan.confidence < 0.5 {
            return Err(OpenAiError::Format(format!(
                "Low confidence: {}",
                plan.confidence
            )));
        }

        Ok(plan)
    }

    pub async fn verify_transfer_math(
        &self,
        mapped_transactions: &[crate::engine::transfer::MappedTransaction],
        opening_balance: rust_decimal::Decimal,
    ) -> Result<bool, OpenAiError> {
        let sys = "You are a forensic accountant. Double-check if Opening + Debits - Credits = Final Balance. Return JSON: { \"is_math_consistent\": true }";
        let user = format!(
            "Op: {}, Txs: {}",
            opening_balance,
            serde_json::to_string(mapped_transactions).unwrap_or_default()
        );
        let out = self.post_json(sys, &user).await?;
        let parsed: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| OpenAiError::Format(e.to_string()))?;
        Ok(parsed["is_math_consistent"].as_bool().unwrap_or(false))
    }

    pub async fn repair_extracted_transactions(
        &self,
        transactions: &[Transaction],
        opening_balance: rust_decimal::Decimal,
        closing_balance: rust_decimal::Decimal,
        raw_ocr_text: &str,
        error_message: &str,
    ) -> Result<Vec<Transaction>, OpenAiError> {
        let sys = "You are an expert financial data repair AI. The OCR extraction failed math verification. Return ONLY JSON array of repaired transactions.";
        let scrubbed = crate::ai::gemini_client::scrub_pii(transactions);
        let user = format!(
            "Opening: {}\nClosing: {}\nError: {}\nCurrent: {}\nRaw OCR: {}",
            opening_balance,
            closing_balance,
            error_message,
            serde_json::to_string(&scrubbed).unwrap_or_default(),
            raw_ocr_text
        );
        let out = self.post_json(sys, &user).await?;
        let repaired: Vec<Transaction> =
            serde_json::from_str(&out).map_err(|e| OpenAiError::Format(e.to_string()))?;
        Ok(repaired)
    }

    pub async fn parse_transactions_from_text(
        &self,
        raw_text: &str,
    ) -> Result<Vec<Transaction>, OpenAiError> {
        let sys = "You are an expert financial data extraction AI. Extract all bank statement transactions from the provided OCR text. Return ONLY a JSON array of Transaction objects. Each transaction must have: date (YYYY-MM-DD), description, amount (negative for debit, positive for credit), balance (optional), and category (optional).";
        let user = format!("Raw OCR Text:\n{}", raw_text);
        let out = self.post_json(sys, &user).await?;
        let parsed: Vec<Transaction> =
            serde_json::from_str(&out).map_err(|e| OpenAiError::Format(e.to_string()))?;
        Ok(parsed)
    }
}
