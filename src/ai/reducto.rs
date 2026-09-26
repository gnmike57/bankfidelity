use crate::app::config::AppConfig;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs;
use tokio::sync::Mutex;

const REDUCTO_API_BASE: &str = "https://platform.reducto.ai";

#[derive(Debug, thiserror::Error)]
pub enum ReductoError {
    #[error("Missing Configuration: {0}")]
    MissingConfig(&'static str),
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Middleware Error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("API Error (HTTP {0}): {1}")]
    Api(StatusCode, String),
    #[error("Parse Error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("System Error: {0}")]
    System(String),
}

#[derive(Serialize)]
struct ParseRequest {
    input: String,
    enhance: Option<EnhanceOptions>,
    retrieval: Option<RetrievalOptions>,
    formatting: Option<FormattingOptions>,
}

#[derive(Serialize)]
struct ExtractRequest {
    input: String,
    instructions: ExtractInstructions,
}

#[derive(Serialize)]
struct SplitRequest {
    input: String,
    split_description: String,
}

#[derive(Serialize)]
struct ClassifyRequest {
    input: String,
    classification_schema: ClassifySchema,
}

#[derive(Serialize)]
struct EnhanceOptions {
    agentic: Vec<AgenticScope>,
}

#[derive(Serialize)]
struct AgenticScope {
    scope: String,
}

#[derive(Serialize)]
struct RetrievalOptions {
    chunking: ChunkingOptions,
}

#[derive(Serialize)]
struct ChunkingOptions {
    chunk_mode: String,
}

#[derive(Serialize)]
struct FormattingOptions {
    table_output_format: String,
}

#[derive(Serialize)]
struct ExtractInstructions {
    schema: serde_json::Value,
}

#[derive(Serialize)]
struct ClassifySchema {
    categories: Vec<String>,
}

#[derive(Deserialize)]
struct UploadResponse {
    file_id: String,
}

#[derive(Deserialize)]
struct ReductoResponse {
    result: ReductoResult,
}

#[derive(Deserialize)]
struct ReductoExtractResponse {
    result: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ReductoResult {
    #[serde(rename = "type")]
    res_type: String,
    url: Option<String>,
    chunks: Option<serde_json::Value>,
    classification: Option<String>,
    sections: Option<serde_json::Value>,
}

pub struct ReductoClient {
    raw_http: reqwest::Client,
    api_key: String,
    upload_cache: Arc<Mutex<HashMap<PathBuf, (String, SystemTime)>>>,
}

impl ReductoClient {
    pub fn from_app_config(_cfg: &AppConfig) -> Result<Self, ReductoError> {
        let api_key = std::env::var("REDUCTO_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            return Err(ReductoError::MissingConfig("REDUCTO_API_KEY is not set"));
        }

        let raw_http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        let upload_cache = Arc::new(Mutex::new(HashMap::new()));

        Ok(Self {
            raw_http,
            api_key,
            upload_cache,
        })
    }

    /// Internal JSON POST helper with exponential backoff on 429/503 rate limits
    async fn post_json_with_retry<Req: Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        payload: &Req,
    ) -> Result<Resp, ReductoError> {
        let max_retries = 2usize;
        let mut delay = std::time::Duration::from_millis(500);

        for attempt in 0..=max_retries {
            let res = self
                .raw_http
                .post(format!("{}{}", REDUCTO_API_BASE, endpoint))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(payload)
                .send()
                .await;

            match res {
                Ok(r) => {
                    let status = r.status();
                    if status.is_success() {
                        return Ok(r.json().await?);
                    }
                    if (status == StatusCode::TOO_MANY_REQUESTS
                        || status == StatusCode::SERVICE_UNAVAILABLE)
                        && attempt < max_retries
                    {
                        tracing::warn!(
                            "[reducto] HTTP {} on {}. Retrying in {:?} (attempt {}/{})",
                            status,
                            endpoint,
                            delay,
                            attempt + 1,
                            max_retries
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    let body = r.text().await.unwrap_or_default();
                    return Err(ReductoError::Api(status, body));
                }
                Err(e) => {
                    if attempt < max_retries {
                        tracing::warn!(
                            "[reducto] Network error on {}: {}. Retrying in {:?} (attempt {}/{})",
                            endpoint,
                            e,
                            delay,
                            attempt + 1,
                            max_retries
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    return Err(ReductoError::Network(e));
                }
            }
        }
        Err(ReductoError::System(format!(
            "Retries exhausted for {}",
            endpoint
        )))
    }

    /// Upload document to Reducto with file modification cache and automatic retries
    pub async fn upload_document(&self, pdf_path: &Path) -> Result<String, ReductoError> {
        let mtime = fs::metadata(pdf_path).await.and_then(|m| m.modified()).ok();

        if let Some(mtime) = mtime {
            let cache = self.upload_cache.lock().await;
            if let Some((file_id, cached_mtime)) = cache.get(pdf_path) {
                if *cached_mtime == mtime {
                    tracing::debug!("[reducto] Reusing cached upload file_id for {:?}", pdf_path);
                    return Ok(file_id.clone());
                }
            }
        }

        let file_bytes = fs::read(pdf_path).await?;
        let file_name = pdf_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let max_retries = 2usize;
        let mut delay = std::time::Duration::from_millis(500);
        let mut last_err = None;

        for attempt in 0..=max_retries {
            let part = reqwest::multipart::Part::bytes(file_bytes.clone())
                .file_name(file_name.clone())
                .mime_str("application/pdf")
                .map_err(|e| ReductoError::System(format!("Invalid mime: {}", e)))?;

            let form = reqwest::multipart::Form::new().part("file", part);

            match self
                .raw_http
                .post(format!("{}/upload", REDUCTO_API_BASE))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .multipart(form)
                .send()
                .await
            {
                Ok(res) => {
                    let status = res.status();
                    if status.is_success() {
                        let body: UploadResponse = res.json().await?;
                        if let Some(mtime) = mtime {
                            let mut cache = self.upload_cache.lock().await;
                            cache.insert(pdf_path.to_path_buf(), (body.file_id.clone(), mtime));
                        }
                        return Ok(body.file_id);
                    }
                    if (status == StatusCode::TOO_MANY_REQUESTS
                        || status == StatusCode::SERVICE_UNAVAILABLE)
                        && attempt < max_retries
                    {
                        tracing::warn!(
                            "[reducto] Upload HTTP {} received. Retrying in {:?} (attempt {}/{})",
                            status,
                            delay,
                            attempt + 1,
                            max_retries
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    let body = res.text().await.unwrap_or_default();
                    return Err(ReductoError::Api(status, body));
                }
                Err(e) => {
                    if attempt < max_retries {
                        tracing::warn!(
                            "[reducto] Upload network error: {}. Retrying in {:?} (attempt {}/{})",
                            e,
                            delay,
                            attempt + 1,
                            max_retries
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    last_err = Some(ReductoError::Network(e));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| ReductoError::System("Upload retries exhausted".into())))
    }

    /// Convert documents into structured text, tables, and figures with layout-aware chunking
    pub async fn parse_document(&self, pdf_path: &Path) -> Result<serde_json::Value, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = ParseRequest {
            input: file_id,
            enhance: Some(EnhanceOptions {
                agentic: vec![AgenticScope {
                    scope: "table".into(),
                }],
            }),
            retrieval: Some(RetrievalOptions {
                chunking: ChunkingOptions {
                    chunk_mode: "variable".into(),
                },
            }),
            formatting: Some(FormattingOptions {
                table_output_format: "json".into(),
            }),
        };

        let parse_res: ReductoResponse = self.post_json_with_retry("/parse", &req).await?;

        let chunks = if parse_res.result.res_type == "url" {
            let url = parse_res.result.url.unwrap_or_default();
            let url_res = self.raw_http.get(&url).send().await?;
            url_res.json().await?
        } else {
            parse_res.result.chunks.unwrap_or(serde_json::Value::Null)
        };

        Ok(chunks)
    }

    /// Pull specific fields into JSON using a JSON Schema
    pub async fn extract_fields(
        &self,
        pdf_path: &Path,
        schema: serde_json::Value,
    ) -> Result<serde_json::Value, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = ExtractRequest {
            input: file_id,
            instructions: ExtractInstructions { schema },
        };

        let extract_res: ReductoExtractResponse =
            self.post_json_with_retry("/extract", &req).await?;
        if extract_res.result.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(extract_res.result[0].clone())
    }

    /// Divide documents into named sections using natural language descriptions
    pub async fn split_document(
        &self,
        pdf_path: &Path,
        split_description: &str,
    ) -> Result<serde_json::Value, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = SplitRequest {
            input: file_id,
            split_description: split_description.to_string(),
        };

        let split_res: ReductoResponse = self.post_json_with_retry("/split", &req).await?;
        Ok(split_res.result.sections.unwrap_or(serde_json::Value::Null))
    }

    /// Classify documents by type before processing
    pub async fn classify_document(
        &self,
        pdf_path: &Path,
        categories: Vec<String>,
    ) -> Result<String, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = ClassifyRequest {
            input: file_id,
            classification_schema: ClassifySchema { categories },
        };

        let classify_res: ReductoResponse = self.post_json_with_retry("/classify", &req).await?;
        Ok(classify_res.result.classification.unwrap_or_default())
    }

    fn parse_amount_clean(val: Option<&serde_json::Value>) -> Option<rust_decimal::Decimal> {
        let s = val?.as_str()?.trim();
        if s.is_empty() {
            return None;
        }
        let cleaned: String = s
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        cleaned.parse::<rust_decimal::Decimal>().ok()
    }

    /// Pull structured bank transactions directly into a canonical BankStatement using Reducto's POST /extract
    pub async fn extract_statement_transactions(
        &self,
        pdf_path: &Path,
    ) -> Result<crate::ai::document_ai::BankStatement, ReductoError> {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "bank_name": { "type": "string" },
                "account_number": { "type": "string" },
                "opening_balance": { "type": "string" },
                "closing_balance": { "type": "string" },
                "transactions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "date": { "type": "string" },
                            "description": { "type": "string" },
                            "debit": { "type": ["string", "null"] },
                            "credit": { "type": ["string", "null"] },
                            "amount": { "type": ["string", "null"] },
                            "running_balance": { "type": ["string", "null"] }
                        },
                        "required": ["date", "description"]
                    }
                }
            },
            "required": ["transactions"]
        });

        let json_val = self.extract_fields(pdf_path, schema).await?;
        let bank_name = json_val
            .get("bank_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let account_number = json_val
            .get("account_number")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let opening_balance = Self::parse_amount_clean(json_val.get("opening_balance"))
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let closing_balance = Self::parse_amount_clean(json_val.get("closing_balance"))
            .unwrap_or(rust_decimal::Decimal::ZERO);

        let mut transactions = Vec::new();
        if let Some(arr) = json_val.get("transactions").and_then(|v| v.as_array()) {
            for (idx, item) in arr.iter().enumerate() {
                let date = item
                    .get("date")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut debit = Self::parse_amount_clean(item.get("debit"));
                let mut credit = Self::parse_amount_clean(item.get("credit"));
                let running_balance = Self::parse_amount_clean(item.get("running_balance"));

                if debit.is_none() && credit.is_none() {
                    if let Some(amt) = Self::parse_amount_clean(item.get("amount")) {
                        if amt < rust_decimal::Decimal::ZERO {
                            credit = Some(-amt);
                        } else {
                            debit = Some(amt);
                        }
                    }
                }

                let raw_text = format!("{date} {description}");
                transactions.push(crate::engine::model::Transaction {
                    page: 0,
                    line_on_page: idx,
                    date,
                    raw_text,
                    debit,
                    credit,
                    running_balance,
                    bbox: None,
                    field_bboxes: crate::engine::model::FieldBboxes::default(),
                    provenance: crate::engine::model::Provenance::Reducto { confidence: 1.0 },
                    category: None,
                    canonical: Default::default(),
                });
            }
        }

        let mut stmt = crate::ai::document_ai::BankStatement {
            total_pages: 1,
            transactions,
            opening_balance,
            closing_balance,
            account_number,
            bank_name,
        };
        stmt.ensure_canonical_metadata();
        Ok(stmt)
    }

    /// Parse a statement PDF into a canonical BankStatement using Reducto.
    /// Prefers structured /extract, then falls back to parsing /parse markdown tables.
    pub async fn parse_statement(
        &self,
        pdf_path: &Path,
    ) -> Result<crate::ai::document_ai::BankStatement, ReductoError> {
        // 1. Direct structured extraction via POST /extract
        match self.extract_statement_transactions(pdf_path).await {
            Ok(stmt) if !stmt.transactions.is_empty() => {
                tracing::info!(
                    "[reducto] Successfully extracted {} transactions via structured /extract",
                    stmt.transactions.len()
                );
                return Ok(stmt);
            }
            Ok(_) => {
                tracing::warn!(
                    "[reducto] Structured /extract returned 0 transactions, falling back to /parse markdown"
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[reducto] Structured /extract failed: {}, falling back to /parse markdown",
                    e
                );
            }
        }

        // 2. Parse full document chunks/tables and convert markdown tables directly
        let chunks = self.parse_document(pdf_path).await?;
        let markdown = chunks.to_string();

        let mut statement = crate::ai::llamaparse::parse_markdown_to_statement_inner(&markdown)
            .map_err(|e| ReductoError::System(format!("Reducto markdown parsing failed: {e}")))?;

        for tx in &mut statement.transactions {
            tx.provenance = crate::engine::model::Provenance::Reducto { confidence: 0.95 };
        }
        statement.ensure_canonical_metadata();

        if statement.transactions.is_empty() {
            return Err(ReductoError::System(
                "Reducto extraction produced 0 transactions".into(),
            ));
        }

        Ok(statement)
    }

    /// Transfer-optimized parser alias ensuring high-fidelity extraction
    pub async fn parse_statement_for_transfer(
        &self,
        pdf_path: &Path,
    ) -> Result<crate::ai::document_ai::BankStatement, ReductoError> {
        self.parse_statement(pdf_path).await
    }

    /// Natural-language guided document editing using Reducto's POST /edit
    pub async fn edit_document(
        &self,
        pdf_path: &Path,
        edit_instructions: &str,
    ) -> Result<serde_json::Value, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        #[derive(serde::Serialize)]
        struct EditRequest {
            input: String,
            edit_instructions: String,
        }

        let req = EditRequest {
            input: file_id,
            edit_instructions: edit_instructions.to_string(),
        };

        let edit_res: ReductoResponse = self.post_json_with_retry("/edit", &req).await?;
        Ok(edit_res.result.chunks.unwrap_or(serde_json::Value::Null))
    }
}
