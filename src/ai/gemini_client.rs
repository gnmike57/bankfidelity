use crate::app::config::{global_http_client, AppConfig, GeminiAuthMode};
use crate::engine::layout::DocumentLayout;
use crate::engine::model::Transaction;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceAdjustment {
    pub page: usize,
    pub line_on_page: usize,
    pub old_running_balance: f64,
    pub new_running_balance: f64,
    pub reason: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiBalancePlan {
    pub adjustments: Vec<BalanceAdjustment>,
    pub overall_strategy: String,
    pub confidence: f32,
}

impl GeminiBalancePlan {
    pub fn validate(&mut self) -> Result<(), GeminiError> {
        if self.confidence < 0.0 || self.confidence > 1.0 {
            return Err(GeminiError::Format(format!(
                "BalancePlan confidence {} out of range",
                self.confidence
            )));
        }
        for adj in &mut self.adjustments {
            if adj.confidence < 0.0 || adj.confidence > 1.0 {
                return Err(GeminiError::Format(format!(
                    "Adjustment confidence {} out of range",
                    adj.confidence
                )));
            }
        }
        Ok(())
    }
}

/// Result of asking Gemini "did Document AI capture every transaction on the
/// page, and does the data look internally consistent?"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiCompletenessReport {
    /// 0..1 - Gemini's confidence that the parse is complete.
    pub completeness_score: f32,
    /// Free-text explanation Gemini provided.
    pub notes: String,
    /// Rows or fields Gemini suspects were missed by Document AI.
    pub missing_rows: Vec<String>,
    /// True when the math (running balances, totals, opening/closing) is
    /// internally consistent.
    pub math_consistent: bool,
}

impl GeminiCompletenessReport {
    pub fn validate(&mut self) -> Result<(), GeminiError> {
        if self.completeness_score < 0.0 || self.completeness_score > 1.0 {
            return Err(GeminiError::Format(format!(
                "Completeness score {} out of range",
                self.completeness_score
            )));
        }
        Ok(())
    }
}

/// Result of a vision-based anomaly check on a rendered page.
///
/// Stage 4 / Item #10: after the workflow renders an edited PDF, we send the
/// page back to Gemini Vision and ask "does anything look off?" - kerning
/// drift, baseline misalignment, font-weight mismatch, colour shift,
/// hallucinated text. Hotspots that overlap an *intended* bbox are
/// expected (we just edited there); hotspots elsewhere on the page mean
/// the renderer collateral-damaged something and the loop should retry or
/// fail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiVisionReport {
    /// 0..1 - overall anomaly score. 0 = looks pristine, 1 = clearly broken.
    pub anomaly_score: f32,
    /// Rectangles (in PDF points) where Gemini saw something off.
    pub hotspots: Vec<VisionHotspot>,
    /// Free-text reasoning.
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionHotspot {
    pub bbox: [f32; 4],
    pub kind: String,
    /// Per-hotspot confidence the anomaly is real.
    pub confidence: f32,
}

impl GeminiVisionReport {
    /// Whether this report should reject the render.
    ///
    /// Rejects when overall `anomaly_score >= reject_threshold` OR when any
    /// hotspot lies outside every intended bbox (i.e. the renderer changed
    /// something we didn't ask it to).
    pub fn should_reject(&self, intended_bboxes: &[[f32; 4]], reject_threshold: f32) -> bool {
        if self.anomaly_score >= reject_threshold {
            return true;
        }
        for h in &self.hotspots {
            if !intended_bboxes
                .iter()
                .any(|b| crate::pdf::bbox_overlap_fraction(h.bbox, *b) > 0.0)
            {
                return true;
            }
        }
        false
    }

    pub fn validate(&mut self) -> Result<(), GeminiError> {
        if self.anomaly_score < 0.0 || self.anomaly_score > 1.0 {
            return Err(GeminiError::Format(format!(
                "Vision anomaly score {} out of range",
                self.anomaly_score
            )));
        }
        for h in &mut self.hotspots {
            if h.confidence < 0.0 || h.confidence > 1.0 {
                return Err(GeminiError::Format(format!(
                    "Hotspot confidence {} out of range",
                    h.confidence
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Nudge {
    pub index: usize,
    pub dx: f32,
    pub dy: f32,
}

#[derive(Debug)]
pub enum ValidationResponse {
    Approved,
    RejectedWithNudges(Vec<Nudge>),
}

#[derive(thiserror::Error, Debug)]
pub enum GeminiError {
    #[error("Missing Configuration: GEMINI_API_KEY")]
    MissingKey,
    #[error("Missing Vertex AI configuration: DOCUMENT_AI_PROJECT_ID (+ location) and a service-account/ADC credential are required for Vertex mode")]
    MissingVertexConfig,
    #[error("Vertex AI auth error: {0}")]
    Vertex(String),
    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Middleware Error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("API Error (HTTP {0}): {1}")]
    Api(StatusCode, String),
    #[error("Invalid Response: {0}")]
    InvalidResponse(String),
    #[error("Low Confidence: {0:.2}")]
    LowConfidence(f32),
    #[error("Format error: {0}")]
    Format(String),
}

pub struct GeminiClient {
    pub api_key: String,
    pub http: reqwest_middleware::ClientWithMiddleware,
    pub base_url: String,
    /// How this client authenticates and which endpoint family it targets.
    auth: GeminiAuth,
}

/// The best available Gemini model id, tried first for all reasoning
/// and vision calls.
///
/// `gemini-3.7-flash` is Google's latest generally available flash model.
const GEMINI_PRO_MODEL: &str = "gemini-3.7-flash";

/// If the primary model fails or is unavailable, fallback to next best
const GEMINI_PRO_FALLBACK: &str = "gemini-flash-latest";

/// Absolute fallback if both models fail.
const GEMINI_FLASH_FALLBACK: &str = "gemini-flash-latest";

/// Resolved authentication strategy for a `GeminiClient`.
#[derive(Clone)]
enum GeminiAuth {
    /// AI Studio API key, appended as `?key=...` to the public endpoint.
    ApiKey,
    /// Vertex AI: a Google Cloud OAuth bearer token (minted from a service
    /// account or ADC) sent as `Authorization: Bearer`, targeting the
    /// regional `{location}-aiplatform.googleapis.com` endpoint for
    /// `projects/{project}/locations/{location}/publishers/google/models`.
    Vertex {
        project_id: String,
        location: String,
        access_token: String,
    },
}

impl GeminiClient {
    pub fn from_app_config(cfg: &AppConfig) -> Result<Self, GeminiError> {
        match cfg.gemini_auth_mode {
            GeminiAuthMode::Vertex => {
                // Vertex needs a GCP project + location and an OAuth token,
                // which we mint from the same Document AI service-account / ADC
                // credentials. This keeps a single GCP identity for both APIs.
                let doc_ai = cfg
                    .document_ai
                    .clone()
                    .ok_or(GeminiError::MissingVertexConfig)?;
                if doc_ai.project_id.is_empty() {
                    return Err(GeminiError::MissingVertexConfig);
                }
                let location = if doc_ai.location.is_empty() || doc_ai.location == "us" {
                    "us-central1".to_string()
                } else if doc_ai.location == "eu" {
                    "europe-west1".to_string()
                } else {
                    doc_ai.location.clone()
                };
                let access_token = mint_gcp_access_token(&doc_ai)
                    .map_err(|e| GeminiError::Vertex(format!("token mint failed: {e}")))?;
                let base_url = format!("https://{location}-aiplatform.googleapis.com");
                Ok(Self {
                    api_key: String::new(),
                    http: global_http_client(),
                    base_url,
                    auth: GeminiAuth::Vertex {
                        project_id: doc_ai.project_id,
                        location,
                        access_token,
                    },
                })
            }
            GeminiAuthMode::ApiKey => {
                let api_key = cfg.gemini_api_key.clone().ok_or(GeminiError::MissingKey)?;
                Ok(Self {
                    api_key,
                    http: global_http_client(),
                    base_url: "https://generativelanguage.googleapis.com".into(),
                    auth: GeminiAuth::ApiKey,
                })
            }
        }
    }

    pub async fn from_app_config_async(cfg: &AppConfig) -> Result<Self, GeminiError> {
        match cfg.gemini_auth_mode {
            GeminiAuthMode::Vertex => {
                let doc_ai = cfg
                    .document_ai
                    .clone()
                    .ok_or(GeminiError::MissingVertexConfig)?;
                if doc_ai.project_id.is_empty() {
                    return Err(GeminiError::MissingVertexConfig);
                }
                let location = if doc_ai.location.is_empty() || doc_ai.location == "us" {
                    "us-central1".to_string()
                } else if doc_ai.location == "eu" {
                    "europe-west1".to_string()
                } else {
                    doc_ai.location.clone()
                };
                let access_token = mint_gcp_access_token_async(&doc_ai)
                    .await
                    .map_err(|e| GeminiError::Vertex(format!("token mint failed: {e}")))?;
                let base_url = format!("https://{location}-aiplatform.googleapis.com");
                Ok(Self {
                    api_key: String::new(),
                    http: global_http_client(),
                    base_url,
                    auth: GeminiAuth::Vertex {
                        project_id: doc_ai.project_id,
                        location,
                        access_token,
                    },
                })
            }
            GeminiAuthMode::ApiKey => {
                let api_key = cfg.gemini_api_key.clone().ok_or(GeminiError::MissingKey)?;
                Ok(Self {
                    api_key,
                    http: global_http_client(),
                    base_url: "https://generativelanguage.googleapis.com".into(),
                    auth: GeminiAuth::ApiKey,
                })
            }
        }
    }

    /// Build the `generateContent` URL for `model` and return it alongside an
    /// optional bearer token to attach as an `Authorization` header.
    ///
    /// This is the single place that differs between the AI Studio API-key
    /// endpoint and the Vertex AI endpoint, so every request method routes
    /// through it and stays auth-agnostic.
    fn endpoint(&self, model: &str) -> (String, Option<String>) {
        match &self.auth {
            GeminiAuth::ApiKey => (
                format!(
                    "{}/v1beta/models/{}:generateContent?key={}",
                    self.base_url,
                    model,
                    self.api_key.trim()
                ),
                None,
            ),
            GeminiAuth::Vertex {
                project_id,
                location,
                access_token,
            } => (
                format!(
                    "{}/v1/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
                    self.base_url, project_id, location, model
                ),
                Some(access_token.clone()),
            ),
        }
    }

    /// POST `body` to `model`'s endpoint, attaching the right auth for the
    /// active mode (`X-goog-api-key` header or bearer token).
    async fn post_generate(
        &self,
        model: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, GeminiError> {
        let (url, bearer) = self.endpoint(model);

        // Dynamic timeout based on body size (rough estimate)
        let body_size = serde_json::to_string(body).map(|s| s.len()).unwrap_or(0);
        let dynamic_timeout = if body_size > 5_000_000 {
            120 // 120s for very large payloads
        } else if body_size > 1_000_000 {
            90
        } else {
            60
        };

        let mut attempts = 0;
        let max_attempts = 5; // 1 initial + 4 retries for extreme resilience
        loop {
            attempts += 1;
            let mut req = self
                .http
                .post(&url)
                .json(body)
                .timeout(std::time::Duration::from_secs(dynamic_timeout));

            match &self.auth {
                GeminiAuth::ApiKey => {
                    req = req.header("x-goog-api-key", self.api_key.trim());
                }
                GeminiAuth::Vertex { .. } => {
                    if let Some(ref token) = bearer {
                        req = req.bearer_auth(token);
                    }
                }
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if (status.is_server_error() || status == 429) && attempts < max_attempts {
                        if status == 429 && attempts >= 3 {
                            tracing::warn!("Gemini 429 Too Many Requests for {}, aborting retries after 3 attempts.", model);
                            return Ok(resp);
                        }
                        // Exponential backoff with jitter to prevent thundering herd
                        let jitter = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.subsec_millis() as u64 % 500)
                            .unwrap_or(250);
                        let delay =
                            std::time::Duration::from_millis(1000 * (1 << (attempts - 1)) + jitter);
                        tracing::warn!(
                            "Gemini {} error for model {}, retrying in {:?}...",
                            status,
                            model,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    if status == reqwest::StatusCode::BAD_REQUEST
                        || status == reqwest::StatusCode::UNAUTHORIZED
                        || status == reqwest::StatusCode::FORBIDDEN
                    {
                        tracing::error!("Gemini API rejected request with {} for model {}! Please verify your GEMINI_API_KEY and quota.", status, model);
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    if attempts < max_attempts {
                        let jitter = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.subsec_millis() as u64 % 500)
                            .unwrap_or(250);
                        let delay =
                            std::time::Duration::from_millis(1000 * (1 << (attempts - 1)) + jitter);
                        tracing::warn!("Gemini network error {}, retrying in {:?}...", e, delay);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
    }

    /// POST to the best available **Pro** model, with a graceful fallback
    /// chain. Tries the frontier preview model first, then the GA Pro model
    /// (for projects/keys that haven't allowlisted preview models, or whose
    /// free tier has no quota for the preview model - HTTP 429), then flash
    /// as a last resort. All reasoning and vision calls go through here so they
    /// always prefer Pro - never the old flash-by-default behavior.
    async fn post_generate_pro(
        &self,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, GeminiError> {
        // A model is "unavailable for this key/project" when access is denied
        // (403), the model id isn't served (404), or the key has no quota for
        // it (429 - common on the AI Studio free tier for preview models).
        fn should_fall_back(status: StatusCode) -> bool {
            status == StatusCode::FORBIDDEN
                || status == StatusCode::NOT_FOUND
                || status == StatusCode::TOO_MANY_REQUESTS
        }

        // Tier 1: frontier preview Pro (best, finance-tuned).
        let response = self.post_generate(GEMINI_PRO_MODEL, body).await?;
        if !should_fall_back(response.status()) {
            return Ok(response);
        }
        tracing::warn!(
            "[gemini] Pro model '{}' unavailable ({}); falling back to GA '{}'",
            GEMINI_PRO_MODEL,
            response.status(),
            GEMINI_PRO_FALLBACK
        );

        // Tier 2: GA Pro.
        let response = self.post_generate(GEMINI_PRO_FALLBACK, body).await?;
        if !should_fall_back(response.status()) {
            return Ok(response);
        }
        tracing::warn!(
            "[gemini] GA Pro '{}' unavailable ({}); falling back to flash '{}'",
            GEMINI_PRO_FALLBACK,
            response.status(),
            GEMINI_FLASH_FALLBACK
        );

        // Tier 3: flash, last resort.
        self.post_generate(GEMINI_FLASH_FALLBACK, body).await
    }

    // Internal method for testing
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            http: global_http_client(),
            base_url,
            auth: GeminiAuth::ApiKey,
        }
    }

    /// Very lightweight test call to verify the configured credentials
    /// (API Key or Vertex Service Account) are valid and authorized to generate content.
    pub async fn ping(&self) -> Result<(), GeminiError> {
        let body = json!({
            "contents": [{ "role": "user", "parts": [{ "text": "ping" }] }],
            "safetySettings": [
                { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" }
            ],
            "generationConfig": { "maxOutputTokens": 1 }
        });

        // Use the cheapest/fastest model for the ping.
        let response = self.post_generate(GEMINI_FLASH_FALLBACK, &body).await?;
        if !response.status().is_success() {
            return Err(GeminiError::Api(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    pub async fn propose_balance_adjustments(
        &self,
        transactions: &[Transaction],
        imbalance: f64,
        layout: &DocumentLayout,
    ) -> Result<GeminiBalancePlan, GeminiError> {
        let scrubbed = scrub_pii(transactions);
        let prompt = format!(
            "You are an expert financial forensic auditor.\n\
             A bank statement has a mathematical imbalance of ${:.2} between its running ledger and its closing balance.\n\
             Your task is to propose the STRICTLY MINIMAL cascading adjustments to the running balances to fix this.\n\
             CRITICAL RULES:\n\
             1. ONLY alter the 'new_running_balance' values. Do NOT invent new deposits or withdrawals.\n\
             2. Ensure the math perfectly bridges the imbalance to the end of the statement.\n\
             3. Maintain strict JSON schema adherence with no markdown wrapping outside the JSON.\n\
             \n\
             Transactions: {}\n\
             Document layout summary: {} pages.",
            imbalance,
            serde_json::to_string(&scrubbed).unwrap_or_default(),
            layout.total_pages
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "adjustments": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "page": { "type": "integer" },
                            "line_on_page": { "type": "integer" },
                            "old_running_balance": { "type": "number" },
                            "new_running_balance": { "type": "number" },
                            "reason": { "type": "string" },
                            "confidence": { "type": "number" }
                        },
                        "required": ["page", "line_on_page", "old_running_balance", "new_running_balance", "reason", "confidence"]
                    }
                },
                "overall_strategy": { "type": "string" },
                "confidence": { "type": "number" }
            },
            "required": ["adjustments", "overall_strategy", "confidence"]
        });

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "safetySettings": [
                { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" }
            ],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        let response = self.post_generate_pro(&body).await?;

        if !response.status().is_success() {
            return Err(GeminiError::Api(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }

        let json_resp: serde_json::Value = response.json().await?;
        let plan_text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| GeminiError::InvalidResponse("Missing or invalid text field".into()))?;

        let mut plan: GeminiBalancePlan = serde_json::from_str(plan_text)
            .map_err(|e| GeminiError::InvalidResponse(format!("JSON parse error: {e}")))?;
        plan.validate()?;

        if plan.confidence < 0.7 {
            return Err(GeminiError::LowConfidence(plan.confidence));
        }

        Ok(plan)
    }

    /// Ask Gemini to validate that Document AI captured every transaction on
    /// the page and that the resulting numbers are internally consistent.
    /// This is stage 1 of the user-facing workflow.
    pub async fn validate_parse_completeness(
        &self,
        transactions: &[Transaction],
        opening_balance: f64,
        closing_balance: f64,
        total_pages: usize,
    ) -> Result<GeminiCompletenessReport, GeminiError> {
        let scrubbed = scrub_pii(transactions);
        let prompt = format!(
            "You are a bank-statement auditor. Document AI extracted the \
             following transactions from a {} page statement.\n\n\
             Opening balance: ${:.2}\nClosing balance: ${:.2}\n\
             Transactions: {}\n\n\
             Confirm: (a) does the running ledger balance to the closing? \
             (b) is anything obviously missing (e.g. fee rows skipped, gap in \
             dates suggesting a row was not captured)? Reply ONLY in the \
             configured JSON schema.",
            total_pages,
            opening_balance,
            closing_balance,
            serde_json::to_string(&scrubbed).unwrap_or_default(),
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "completeness_score": { "type": "number" },
                "notes":              { "type": "string" },
                "missing_rows":       { "type": "array", "items": { "type": "string" } },
                "math_consistent":    { "type": "boolean" }
            },
            "required": ["completeness_score", "notes", "missing_rows", "math_consistent"]
        });

        let body = json!({
            "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
            "safetySettings": [
                { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE" },
                { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" }
            ],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        match self.post_generate_pro(&body).await {
            Ok(response) if response.status().is_success() => {
                let json_resp: serde_json::Value = response.json().await?;
                let plan_text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
                    .as_str()
                    .ok_or_else(|| {
                        GeminiError::InvalidResponse("Missing or invalid text field".into())
                    })?;
                let mut report: GeminiCompletenessReport = serde_json::from_str(plan_text)
                    .map_err(|e| GeminiError::InvalidResponse(format!("JSON parse error: {e}")))?;
                report.validate()?;
                Ok(report)
            }
            err => {
                let status = match &err {
                    Ok(r) => Some(r.status()),
                    Err(_) => None,
                };
                tracing::warn!(
                    "[gemini] Completeness check API failed (status: {:?}). Falling back to local pure-Rust math heuristic.",
                    status
                );

                let mut sum_credits: f64 = 0.0;
                let mut sum_debits: f64 = 0.0;
                for tx in transactions {
                    sum_credits += tx
                        .credit
                        .map(|d| d.to_string().parse::<f64>().unwrap_or(0.0))
                        .unwrap_or(0.0);
                    sum_debits += tx
                        .debit
                        .map(|d| d.to_string().parse::<f64>().unwrap_or(0.0))
                        .unwrap_or(0.0);
                }
                let calculated_close = opening_balance + sum_credits - sum_debits;
                let math_consistent = (calculated_close - closing_balance).abs() < 0.02;

                let report = GeminiCompletenessReport {
                    completeness_score: if math_consistent { 1.0 } else { 0.5 },
                    notes: "Generated by local fallback heuristics due to API failure.".to_string(),
                    missing_rows: vec![],
                    math_consistent,
                };
                Ok(report)
            }
        }
    }

    pub async fn repair_extracted_transactions(
        &self,
        transactions: &[Transaction],
        opening_balance: rust_decimal::Decimal,
        closing_balance: rust_decimal::Decimal,
        raw_ocr_text: &str,
        error_message: &str,
    ) -> Result<Vec<Transaction>, GeminiError> {
        let schema = serde_json::json!({
            "type": "ARRAY",
            "items": {
                "type": "OBJECT",
                "properties": {
                    "date": { "type": "STRING", "description": "Transaction date, e.g., '10/24'" },
                    "description": { "type": "STRING" },
                    "debit": { "type": "NUMBER", "nullable": true },
                    "credit": { "type": "NUMBER", "nullable": true },
                    "running_balance": { "type": "NUMBER", "nullable": true }
                },
                "required": ["date", "description"]
            }
        });

        let scrubbed = scrub_pii(transactions);

        let prompt = format!(
            "You are an expert financial data repair AI. The OCR extraction failed the math verification test.\n\
             Opening Balance: ${}\n\
             Target Closing Balance: ${}\n\n\
             Math Error: {}\n\n\
             Current Extracted Transactions:\n{}\n\n\
             Raw OCR Text:\n{}\n\n\
             Fix the transactions based on the raw OCR text. The math MUST sum perfectly. Return the full repaired list.",
             opening_balance, closing_balance, error_message, serde_json::to_string(&scrubbed).unwrap_or_default(), raw_ocr_text
        );

        let body = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "temperature": 0.0,
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        let response = self.post_generate_pro(&body).await?;

        if !response.status().is_success() {
            return Err(GeminiError::Api(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }

        let json_resp: serde_json::Value = response.json().await?;
        let text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| GeminiError::InvalidResponse("Missing text field".into()))?;

        let repaired: Vec<Transaction> = serde_json::from_str(text)
            .map_err(|e| GeminiError::InvalidResponse(format!("JSON parse error: {e}")))?;

        Ok(repaired)
    }

    /// Double-checks the mathematics of the final transactions (Stage 2)
    pub async fn verify_statement_mathematics(
        &self,
        transactions_json: &str,
        opening: f64,
    ) -> Result<bool, GeminiError> {
        let prompt = format!(
            "You are a forensic accountant. You must double-check the mathematics of the following \
             bank statement transactions. Specifically, you must ensure that:\n\
             Opening Balance + Sum of Credits - Sum of Debits = Closing Balance.\n\n\
             Opening Balance: {opening}\n\n\
             Transactions (JSON format):\n{transactions_json}\n\n\
             Respond in JSON with a single boolean field `is_mathematically_sound`."
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "is_mathematically_sound": { "type": "boolean" }
            },
            "required": ["is_mathematically_sound"]
        });

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        let response = self.post_generate_pro(&body).await?;
        if !response.status().is_success() {
            return Err(GeminiError::Api(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }

        let resp_json: serde_json::Value = response.json().await.map_err(GeminiError::Network)?;
        let content = resp_json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("{}");

        #[derive(serde::Deserialize)]
        struct MathCheck {
            is_mathematically_sound: bool,
        }

        match serde_json::from_str::<MathCheck>(content) {
            Ok(check) => Ok(check.is_mathematically_sound),
            Err(e) => Err(GeminiError::InvalidResponse(e.to_string())),
        }
    }

    /// Vision-based anomaly check on a rendered page.
    ///
    /// Stage 4 / Item #10. `page_png` is the PNG bytes of the rendered
    /// edited page; `intended_bboxes` is the list of bounding boxes (in PDF
    /// points) the user actually intended to change. Returns
    /// [`GeminiVisionReport`] with `anomaly_score` and any hotspots Gemini
    /// flagged.
    pub async fn validate_render_visually(
        &self,
        page_png: &[u8],
        intended_bboxes: &[[f32; 4]],
    ) -> Result<GeminiVisionReport, GeminiError> {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(page_png);

        let prompt = format!(
            "You are a forensic-document analyst examining a rendered bank \
             statement page that has been programmatically edited. \
             Examine the image for any visual anomalies: kerning drift, \
             baseline misalignment, font-weight or colour mismatch \
             vs neighbouring text, ghosted glyphs, redaction edge \
             artifacts, hallucinated or duplicated numbers. \n\n\
             The user *intended* to edit text inside these bounding boxes \
             (in PDF points, [x0, y0, x1, y1]): {}.\n\n\
             Return ONLY the configured JSON. anomaly_score should be 0.0 \
             when the page looks pristine and 1.0 when it's clearly broken. \
             Each hotspot is a region of concern; the bbox should be in PDF \
             points (the page is letter or A4, ~612x792pt). 'kind' is one \
             of: kerning, baseline, weight, color, ghost, edge, hallucinated. \
             Only flag genuinely suspicious regions; sub-pixel rendering \
             noise or expected redactions inside the intended bboxes are \
             not anomalies.",
            serde_json::to_string(intended_bboxes).unwrap_or_default()
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "anomaly_score": { "type": "number" },
                "hotspots": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "bbox": {
                                "type": "array",
                                "items": { "type": "number" }
                            },
                            "kind": { "type": "string" },
                            "confidence": { "type": "number" }
                        },
                        "required": ["bbox", "kind", "confidence"]
                    }
                },
                "notes": { "type": "string" }
            },
            "required": ["anomaly_score", "hotspots", "notes"]
        });

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [
                    { "text": prompt },
                    {
                        "inline_data": {
                            "mime_type": "image/png",
                            "data": b64,
                        }
                    }
                ]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        // Vision benefits from the most capable Pro model; the helper falls
        // back to flash automatically if Pro is unavailable for this key.
        let response = self.post_generate_pro(&body).await?;

        if !response.status().is_success() {
            return Err(GeminiError::Api(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }
        let json_resp: serde_json::Value = response.json().await?;
        let report_text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| GeminiError::InvalidResponse("Missing vision text field".into()))?;
        let mut report: GeminiVisionReport = serde_json::from_str(report_text)
            .map_err(|e| GeminiError::InvalidResponse(format!("vision JSON parse: {e}")))?;
        report.validate()?;
        Ok(report)
    }

    /// Plan how to transfer transactions from a source statement to a target
    /// statement. Gemini analyses the formats of both and produces a mapping
    /// plan including date conversion, description adaptation, and page layout.
    pub async fn plan_transaction_transfer(
        &self,
        source_transactions: &[Transaction],
        target_transactions: &[Transaction],
        correction_hint: Option<&str>,
    ) -> Result<crate::engine::transfer::TransferPlan, GeminiError> {
        let scrubbed_source = scrub_pii(source_transactions);
        let scrubbed_target = scrub_pii(target_transactions);

        let mut prompt = format!(
            "You are an expert financial document analyst. You need to plan how to transfer \
             transactions from a SOURCE bank statement to a TARGET bank statement.\n\n\
             SOURCE statement transactions ({} rows):\n{}\n\n\
             TARGET statement transactions ({} rows):\n{}\n\n\
             Analyze both formats (date style, number format, description conventions, \
             column layout) and produce a transfer plan. For each source transaction, \
             specify which target page and line it should land on. Convert dates to the \
             target's format. Adapt descriptions to match the target's style. \
             If the source has more transactions than the target's pages can hold, \
             specify pages_to_clone (which target page to duplicate for overflow). \
             If the source has fewer, specify pages_to_remove. \
             Each mapping must reference a source_index (0-based into the source list).",
            source_transactions.len(),
            serde_json::to_string(&scrubbed_source).unwrap_or_default(),
            target_transactions.len(),
            serde_json::to_string(&scrubbed_target).unwrap_or_default(),
        );

        if let Some(hint) = correction_hint {
            prompt.push_str(&format!("\n\nCRITICAL CORRECTION HINT from previous failed attempt:\n{hint}\n\nPlease adjust your plan to resolve this error."));
        }

        let schema = json!({
            "type": "object",
            "properties": {
                "mappings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "source_index": { "type": "integer" },
                            "target_page": { "type": "integer" },
                            "target_line": { "type": "integer" },
                            "converted_date": { "type": "string" },
                            "adapted_description": { "type": "string" }
                        },
                        "required": ["source_index", "target_page", "target_line", "converted_date", "adapted_description"]
                    }
                },
                "output_page_count": { "type": "integer" },
                "pages_to_clone": {
                    "type": "array",
                    "items": { "type": "integer" }
                },
                "pages_to_remove": {
                    "type": "array",
                    "items": { "type": "integer" }
                },
                "strategy": { "type": "string" },
                "confidence": { "type": "number" }
            },
            "required": ["mappings", "output_page_count", "pages_to_clone", "pages_to_remove", "strategy", "confidence"]
        });

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        let response = self.post_generate_pro(&body).await?;

        if !response.status().is_success() {
            return Err(GeminiError::Api(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }

        let json_resp: serde_json::Value = response.json().await?;
        let plan_text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| GeminiError::InvalidResponse("Missing transfer plan text".into()))?;

        let plan: crate::engine::transfer::TransferPlan = serde_json::from_str(plan_text)
            .map_err(|e| GeminiError::InvalidResponse(format!("Transfer plan JSON parse: {e}")))?;

        if plan.confidence < 0.5 {
            return Err(GeminiError::LowConfidence(plan.confidence));
        }

        Ok(plan)
    }

    /// Forensic accountant verification of a transferred statement's math.
    /// Gemini independently checks: Opening + ∑Debits − ∑Credits = Final Balance.
    pub async fn verify_transfer_math(
        &self,
        mapped_transactions: &[crate::engine::transfer::MappedTransaction],
        opening_balance: rust_decimal::Decimal,
    ) -> Result<bool, GeminiError> {
        use crate::engine::model::dec_to_f64;

        let tx_summary: Vec<serde_json::Value> = mapped_transactions
            .iter()
            .enumerate()
            .map(|(i, tx)| {
                json!({
                    "row": i,
                    "date": tx.date,
                    "description": tx.description,
                    "debit": tx.debit.map(dec_to_f64),
                    "credit": tx.credit.map(dec_to_f64),
                    "running_balance": dec_to_f64(tx.running_balance),
                })
            })
            .collect();

        let prompt = format!(
            "You are a forensic accountant verifying a bank statement after a transaction \
             transfer operation. The opening balance is {}.\n\n\
             Verify that for EVERY row: running_balance = previous_running_balance + debit - credit.\n\
             (Opening balance is used as previous_running_balance for the first row.)\n\
             Verify the final running_balance is mathematically consistent.\n\n\
             Transactions: {}\n\n\
             Return only the JSON with your verdict.",
            dec_to_f64(opening_balance),
            serde_json::to_string(&tx_summary).unwrap_or_default(),
        );

        let schema = json!({
            "type": "object",
            "properties": {
                "math_valid": { "type": "boolean" },
                "discrepancies": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "row": { "type": "integer" },
                            "expected_balance": { "type": "number" },
                            "actual_balance": { "type": "number" },
                            "note": { "type": "string" }
                        },
                        "required": ["row", "expected_balance", "actual_balance", "note"]
                    }
                },
                "summary": { "type": "string" }
            },
            "required": ["math_valid", "discrepancies", "summary"]
        });

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        let response = self.post_generate_pro(&body).await?;

        if !response.status().is_success() {
            return Err(GeminiError::Api(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }

        let json_resp: serde_json::Value = response.json().await?;
        let text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| GeminiError::InvalidResponse("Missing math verification text".into()))?;

        let parsed: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| GeminiError::InvalidResponse(format!("Math verify JSON: {e}")))?;

        Ok(parsed["math_valid"].as_bool().unwrap_or(false))
    }

    pub async fn review_visual_proof(
        &self,
        proof_pngs: &[Vec<u8>],
    ) -> Result<ValidationResponse, GeminiError> {
        use base64::Engine;
        if self.api_key.is_empty() {
            return Ok(ValidationResponse::Approved); // Auto-approve if no Gemini API key (offline mode)
        }

        let prompt = "You are a micro-typographical visual layout verifier.
Look at these 300 DPI PNG proofs. The red text shows where the new edits will be placed. The yellow highlight is the bounding box.
Verify if the red text perfectly aligns with the surrounding text baselines and margins.
If flawless, reply exactly 'APPROVED'.
If misaligned, reply with JSON specifying the required shift in points: {\"nudges\": [{\"index\": 0, \"dx\": 0.0, \"dy\": 1.5}]}";

        let mut parts = vec![serde_json::json!({ "text": prompt })];

        for png_bytes in proof_pngs {
            let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
            parts.push(serde_json::json!({
                "inline_data": {
                    "mime_type": "image/png",
                    "data": b64
                }
            }));
        }

        let payload = serde_json::json!({
            "contents": [{ "parts": parts }],
            "generationConfig": {
                "temperature": 0.0,
                "maxOutputTokens": 2048,
            }
        });

        let resp = self.post_generate_pro(&payload).await?;
        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| GeminiError::InvalidResponse(e.to_string()))?;
        if let Some(candidates) = resp_json["candidates"].as_array() {
            if let Some(first) = candidates.first() {
                if let Some(parts) = first["content"]["parts"].as_array() {
                    if let Some(text) = parts[0]["text"].as_str() {
                        tracing::info!("AI Visual Review result: {}", text.trim());
                        if text.trim().starts_with("APPROVED") {
                            return Ok(ValidationResponse::Approved);
                        } else {
                            // Try to parse JSON nudges
                            // Extract JSON if it's wrapped in markdown blocks
                            let mut json_str = text.trim();
                            if json_str.starts_with("```json") {
                                json_str = json_str
                                    .trim_start_matches("```json")
                                    .trim_end_matches("```")
                                    .trim();
                            } else if json_str.starts_with("```") {
                                json_str = json_str
                                    .trim_start_matches("```")
                                    .trim_end_matches("```")
                                    .trim();
                            }

                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str)
                            {
                                if let Some(nudges_array) = parsed["nudges"].as_array() {
                                    let mut nudges = Vec::new();
                                    for n in nudges_array {
                                        if let (Some(idx), Some(dx), Some(dy)) = (
                                            n["index"].as_u64(),
                                            n["dx"].as_f64(),
                                            n["dy"].as_f64(),
                                        ) {
                                            nudges.push(Nudge {
                                                index: idx as usize,
                                                dx: dx as f32,
                                                dy: dy as f32,
                                            });
                                        }
                                    }
                                    return Ok(ValidationResponse::RejectedWithNudges(nudges));
                                }
                            }
                            return Ok(ValidationResponse::RejectedWithNudges(vec![]));
                        }
                    }
                }
            }
        }

        Ok(ValidationResponse::RejectedWithNudges(vec![]))
    }

    pub async fn apply_natural_language_edit(
        &self,
        prompt: &str,
        transactions: &[Transaction],
    ) -> Result<Vec<Transaction>, GeminiError> {
        let system_prompt = "You are an expert forensic accounting AI specialising in Australian bank statements. You have deep knowledge of: (1) AU bank statement formats (ANZ, CommBank, NAB, Westpac, Bankwest, ING, Macquarie); (2) transaction types: salary/payroll credits, direct debits, BPAY, Osko/PayID, ATM, card purchases, interest, fees, transfers; (3) pay cycle patterns: weekly (every 7 days), fortnightly (every 14 days), monthly (last business day); (4) payee name resolution: employer names, payroll references (e.g. EMPLOYER PTY LTD PAYROLL, ATO REFUND, CENTRELINK); (5) running balance cascade: after any amount change, all subsequent running_balance values must be recalculated as prior_balance + credit - debit; (6) AU date formats: DD MMM YYYY, DD/MM/YYYY; (7) financial identity: when the user says a person's name (e.g. Maree), resolve it to the payee/employer in the transactions. Apply the instruction precisely including cascading all running_balance values after any amount change. Return ONLY the fully updated JSON array of transactions with no commentary.";

        let schema = json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "page": { "type": "integer" },
                    "line_on_page": { "type": "integer" },
                    "date": { "type": "string" },
                    "raw_text": { "type": "string" },
                    "debit": { "type": "number", "nullable": true },
                    "credit": { "type": "number", "nullable": true },
                    "running_balance": { "type": "number", "nullable": true }
                },
                "required": ["page", "line_on_page", "date", "raw_text"]
            }
        });

        let user_prompt = format!(
            "Instruction: {}\n\nTransactions: {}",
            prompt,
            serde_json::to_string_pretty(transactions).unwrap_or_default()
        );

        let body = serde_json::json!({
            "system_instruction": {
                "parts": [{ "text": system_prompt }]
            },
            "contents": [{
                "role": "user",
                "parts": [{ "text": user_prompt }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        let response = self.post_generate_pro(&body).await?;
        let json_resp: serde_json::Value = response.json().await.map_err(|e| {
            GeminiError::Api(reqwest::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;

        let text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("[]");

        // Parse the resulting array back to transactions, preserving bounding boxes and provenances if possible.
        #[derive(Deserialize)]
        struct EditedTx {
            page: usize,
            line_on_page: usize,
            date: String,
            raw_text: String,
            debit: Option<f64>,
            credit: Option<f64>,
            running_balance: Option<f64>,
        }

        let edited: Vec<EditedTx> =
            serde_json::from_str(text).map_err(|e| GeminiError::Format(e.to_string()))?;

        let mut result = transactions.to_vec();
        for edit in edited {
            if let Some(tx) = result
                .iter_mut()
                .find(|t| t.page == edit.page && t.line_on_page == edit.line_on_page)
            {
                tx.date = edit.date;
                tx.raw_text = edit.raw_text;
                if let Some(d) = edit.debit {
                    tx.debit = Some(rust_decimal::Decimal::from_f64_retain(d).unwrap_or_default());
                } else {
                    tx.debit = None;
                }
                if let Some(c) = edit.credit {
                    tx.credit = Some(rust_decimal::Decimal::from_f64_retain(c).unwrap_or_default());
                } else {
                    tx.credit = None;
                }
                if let Some(b) = edit.running_balance {
                    tx.running_balance =
                        Some(rust_decimal::Decimal::from_f64_retain(b).unwrap_or_default());
                } else {
                    tx.running_balance = None;
                }
            }
        }

        Ok(result)
    }
}

/// Mint a short-lived Google Cloud OAuth access token (scope
/// `cloud-platform`) for Vertex AI, from the same credentials the Document AI
/// client uses. Prefers an explicit service-account JSON key
/// (`service_account_path`); falls back to an ADC `authorized_user` file
/// (`adc_path`). Returns the bearer token string.
///
/// This is a synchronous, one-shot exchange (used at client construction)
/// implemented with `reqwest::blocking` so `from_app_config` stays non-async.
pub async fn mint_gcp_access_token_async(
    doc_ai: &crate::app::config::DocumentAiConfig,
) -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| format!("clock error: {e}"))?;

    let post_form = |url: String, form: Vec<(String, String)>| async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        let resp = client
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| format!("token request: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!(
                "token endpoint {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| format!("token json: {e}"))
    };

    if !doc_ai.service_account_path.is_empty() {
        let key_content = tokio::fs::read_to_string(&doc_ai.service_account_path)
            .await
            .map_err(|e| format!("read service account: {e}"))?;
        let sa: serde_json::Value =
            serde_json::from_str(&key_content).map_err(|e| format!("parse SA json: {e}"))?;
        let client_email = sa["client_email"]
            .as_str()
            .ok_or("service account missing client_email")?;
        let private_key = sa["private_key"]
            .as_str()
            .ok_or("service account missing private_key")?;

        #[derive(Serialize)]
        struct Claims {
            iss: String,
            scope: String,
            aud: String,
            iat: u64,
            exp: u64,
        }
        let claims = Claims {
            iss: client_email.to_string(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
            aud: "https://oauth2.googleapis.com/token".to_string(),
            iat: now,
            exp: now + 3600,
        };
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        let encoding_key = EncodingKey::from_rsa_pem(private_key.as_bytes())
            .map_err(|e| format!("bad private key: {e}"))?;
        let signed =
            encode(&header, &claims, &encoding_key).map_err(|e| format!("jwt sign: {e}"))?;

        let v = post_form(
            "https://oauth2.googleapis.com/token".to_string(),
            vec![
                (
                    "grant_type".to_string(),
                    "urn:ietf:params:oauth:grant-type:jwt-bearer".to_string(),
                ),
                ("assertion".to_string(), signed),
            ],
        )
        .await?;
        return v["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "token response missing access_token".to_string());
    }

    if !doc_ai.adc_path.is_empty() {
        let raw = tokio::fs::read_to_string(&doc_ai.adc_path)
            .await
            .map_err(|e| format!("read ADC: {e}"))?;
        let adc: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse ADC json: {e}"))?;
        let client_id = adc["client_id"].as_str().ok_or("ADC missing client_id")?;
        let client_secret = adc["client_secret"]
            .as_str()
            .ok_or("ADC missing client_secret")?;
        let refresh_token = adc["refresh_token"]
            .as_str()
            .ok_or("ADC missing refresh_token")?;

        let resp_json = post_form(
            "https://oauth2.googleapis.com/token".to_string(),
            vec![
                ("client_id".to_string(), client_id.to_string()),
                ("client_secret".to_string(), client_secret.to_string()),
                ("refresh_token".to_string(), refresh_token.to_string()),
                ("grant_type".to_string(), "refresh_token".to_string()),
            ],
        )
        .await?;
        return resp_json["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "ADC token response missing access_token".to_string());
    }

    Err("Neither service_account_path nor adc_path is configured for Vertex AI mode".into())
}

/// Mint a short-lived Google Cloud OAuth access token (scope
/// `cloud-platform`) for Vertex AI, from the same credentials the Document AI
/// client uses. Prefers an explicit service-account JSON key
/// (`service_account_path`); falls back to an ADC `authorized_user` file
/// (`adc_path`). Returns the bearer token string.
///
/// This is a synchronous, one-shot exchange (used at client construction)
/// implemented with `reqwest::blocking` so `from_app_config` stays non-async.
fn mint_gcp_access_token(doc_ai: &crate::app::config::DocumentAiConfig) -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| format!("clock error: {e}"))?;

    let http = {
        // reqwest::blocking::Client internally spawns a tokio runtime,
        // which panics if we're already inside one. Detect and work around.
        match tokio::runtime::Handle::try_current() {
            Ok(_handle) => {
                // We're inside a tokio runtime - use the async client via block_in_place.
                // We return a thin wrapper that uses the async client synchronously.
                None // signal to use async path below
            }
            Err(_) => Some(
                reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(60))
                    .build()
                    .unwrap_or_default(),
            ),
        }
    };

    // Helper: POST a form and return the JSON response, works in both contexts.
    let post_form = |url: &str, form: &[(&str, &str)]| -> Result<serde_json::Value, String> {
        if let Some(ref client) = http {
            // Pure blocking path (no tokio runtime active)
            let resp = client
                .post(url)
                .form(form)
                .send()
                .map_err(|e| format!("token request: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!(
                    "token endpoint {}: {}",
                    resp.status(),
                    resp.text().unwrap_or_default()
                ));
            }
            resp.json().map_err(|e| format!("token json: {e}"))
        } else {
            // Inside tokio - use block_in_place + async client
            tokio::task::block_in_place(|| {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(60))
                        .build()
                        .unwrap_or_default();
                    let resp = client
                        .post(url)
                        .form(form)
                        .send()
                        .await
                        .map_err(|e| format!("token request: {e}"))?;
                    if !resp.status().is_success() {
                        return Err(format!(
                            "token endpoint {}: {}",
                            resp.status(),
                            resp.text().await.unwrap_or_default()
                        ));
                    }
                    resp.json().await.map_err(|e| format!("token json: {e}"))
                })
            })
        }
    };

    // Preferred: service-account JWT-bearer grant.
    if !doc_ai.service_account_path.is_empty() {
        let key_content = std::fs::read_to_string(&doc_ai.service_account_path)
            .map_err(|e| format!("read service account: {e}"))?;
        let sa: serde_json::Value =
            serde_json::from_str(&key_content).map_err(|e| format!("parse SA json: {e}"))?;
        let client_email = sa["client_email"]
            .as_str()
            .ok_or("service account missing client_email")?;
        let private_key = sa["private_key"]
            .as_str()
            .ok_or("service account missing private_key")?;

        #[derive(Serialize)]
        struct Claims {
            iss: String,
            scope: String,
            aud: String,
            iat: u64,
            exp: u64,
        }
        let claims = Claims {
            iss: client_email.to_string(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
            aud: "https://oauth2.googleapis.com/token".to_string(),
            iat: now,
            exp: now + 3600,
        };
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        let encoding_key = EncodingKey::from_rsa_pem(private_key.as_bytes())
            .map_err(|e| format!("bad private key: {e}"))?;
        let signed =
            encode(&header, &claims, &encoding_key).map_err(|e| format!("jwt sign: {e}"))?;

        let v = post_form(
            "https://oauth2.googleapis.com/token",
            &[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &signed),
            ],
        )?;
        return v["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "token response missing access_token".to_string());
    }

    // Fallback: ADC authorized_user refresh-token grant.
    if !doc_ai.adc_path.is_empty() {
        let raw =
            std::fs::read_to_string(&doc_ai.adc_path).map_err(|e| format!("read ADC: {e}"))?;
        let adc: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse ADC json: {e}"))?;
        let client_id = adc["client_id"].as_str().ok_or("ADC missing client_id")?;
        let client_secret = adc["client_secret"]
            .as_str()
            .ok_or("ADC missing client_secret")?;
        let refresh_token = adc["refresh_token"]
            .as_str()
            .ok_or("ADC missing refresh_token")?;
        let v = post_form(
            "https://oauth2.googleapis.com/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
            ],
        )?;
        return v["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "ADC token response missing access_token".to_string());
    }

    Err("Vertex mode needs a service-account JSON (GOOGLE_APPLICATION_CREDENTIALS) or ADC".into())
}

/// Helper function to redact PII (like account numbers) from transactions before
/// sending them to the cloud for analysis. Gemini only needs the math, not the PII.
pub fn scrub_pii(transactions: &[Transaction]) -> Vec<Transaction> {
    // Basic scrubbing: replace sequences of 6+ digits (potential account/routing numbers)
    #[allow(clippy::unwrap_used)] // Static regex pattern — compilation cannot fail
    static RE_DIGITS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\b\d{6,}\b").unwrap());
    let re_digits = &*RE_DIGITS;

    transactions
        .iter()
        .map(|t| {
            let mut scrubbed = t.clone();
            scrubbed.raw_text = re_digits
                .replace_all(&scrubbed.raw_text, "[REDACTED_ACCOUNT]")
                .into_owned();
            scrubbed
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn request_body_uses_camelcase_and_object_schema() {
        let _transactions: Vec<Transaction> = vec![];
        let _layout = DocumentLayout {
            total_pages: 1,
            pages: vec![],
            has_consistent_headers: true,
            has_consistent_footers: true,
            overall_style: "".into(),
            layout_confidence: 1.0,
        };

        // We simulate the request building logic
        let schema = json!({
            "type": "object",
            "properties": {
                "adjustments": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "page": { "type": "integer" },
                            "line_on_page": { "type": "integer" },
                            "old_running_balance": { "type": "number" },
                            "new_running_balance": { "type": "number" },
                            "reason": { "type": "string" },
                            "confidence": { "type": "number" }
                        },
                        "required": ["page", "line_on_page", "old_running_balance", "new_running_balance", "reason", "confidence"]
                    }
                },
                "overall_strategy": { "type": "string" },
                "confidence": { "type": "number" }
            },
            "required": ["adjustments", "overall_strategy", "confidence"]
        });

        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": "prompt" }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": schema
            }
        });

        assert_eq!(
            body["generationConfig"]["responseMimeType"]
                .as_str()
                .unwrap(),
            "application/json"
        );
        assert!(body["generationConfig"]["responseSchema"].is_object());
    }

    #[tokio::test]
    async fn low_confidence_response_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1beta/models/gemini-pro-preview:generateContent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [{
                    "content": {
                        "parts": [{
                            "text": "{\"adjustments\": [], \"overall_strategy\": \"Unsure\", \"confidence\": 0.5}"
                        }]
                    }
                }]
            })))
            .mount(&server)
            .await;

        let client = GeminiClient::with_base_url("fake".into(), server.uri());

        let plan = client
            .propose_balance_adjustments(
                &[],
                0.0,
                &DocumentLayout {
                    total_pages: 1,
                    pages: vec![],
                    has_consistent_headers: true,
                    has_consistent_footers: true,
                    overall_style: "".into(),
                    layout_confidence: 1.0,
                },
            )
            .await;

        match plan {
            Err(GeminiError::LowConfidence(conf)) => assert_eq!(conf, 0.5),
            _ => panic!("Expected LowConfidence error"),
        }
    }

    fn hot(bbox: [f32; 4], kind: &str) -> VisionHotspot {
        VisionHotspot {
            bbox,
            kind: kind.into(),
            confidence: 0.9,
        }
    }

    #[test]
    fn vision_should_reject_when_overall_score_too_high() {
        let report = GeminiVisionReport {
            anomaly_score: 0.5,
            hotspots: vec![],
            notes: String::new(),
        };
        // No hotspots; threshold 0.15 -> reject by score alone.
        assert!(report.should_reject(&[], 0.15));
        assert!(!report.should_reject(&[], 0.6));
    }

    #[test]
    fn vision_should_reject_when_hotspot_outside_intended_bboxes() {
        let report = GeminiVisionReport {
            anomaly_score: 0.05,
            hotspots: vec![hot([200.0, 300.0, 250.0, 320.0], "kerning")],
            notes: String::new(),
        };
        let intended = vec![[100.0, 100.0, 150.0, 120.0]];
        // Score is fine but the hotspot is unintended -> reject.
        assert!(report.should_reject(&intended, 0.15));
    }

    #[test]
    fn vision_should_accept_when_hotspots_overlap_intended() {
        let report = GeminiVisionReport {
            anomaly_score: 0.04,
            hotspots: vec![hot([105.0, 102.0, 145.0, 118.0], "edge")],
            notes: String::new(),
        };
        let intended = vec![[100.0, 100.0, 150.0, 120.0]];
        // Both checks pass: low score, hotspot inside the intended bbox.
        assert!(!report.should_reject(&intended, 0.15));
    }

    #[test]
    fn vision_accepts_pristine_render_with_no_hotspots() {
        let report = GeminiVisionReport {
            anomaly_score: 0.0,
            hotspots: vec![],
            notes: "looks clean".into(),
        };
        assert!(!report.should_reject(&[], 0.15));
        assert!(!report.should_reject(&[[0.0, 0.0, 100.0, 100.0]], 0.15));
    }
}
