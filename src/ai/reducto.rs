use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::app::config::AppConfig;
use tokio::fs;

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

        Ok(Self {
            raw_http,
            api_key,
        })
    }

    pub async fn upload_document(&self, pdf_path: &Path) -> Result<String, ReductoError> {
        let file_bytes = fs::read(pdf_path).await?;
        let file_name = pdf_path.file_name().unwrap_or_default().to_string_lossy().to_string();

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.clone())
            .mime_str("application/pdf")
            .map_err(|e| ReductoError::System(format!("Invalid mime: {}", e)))?;

        let form = reqwest::multipart::Form::new().part("file", part);

        let res = self.raw_http
            .post(format!("{}/upload", REDUCTO_API_BASE))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(ReductoError::Api(res.status(), res.text().await?));
        }

        let body: UploadResponse = res.json().await?;
        Ok(body.file_id)
    }

    /// Convert documents into structured text, tables, and figures with layout-aware chunking
    pub async fn parse_document(&self, pdf_path: &Path) -> Result<serde_json::Value, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = ParseRequest {
            input: file_id,
            enhance: Some(EnhanceOptions {
                agentic: vec![AgenticScope { scope: "table".into() }],
            }),
            retrieval: Some(RetrievalOptions {
                chunking: ChunkingOptions { chunk_mode: "variable".into() }
            }),
            formatting: Some(FormattingOptions {
                table_output_format: "json".into(),
            }),
        };

        let res = self.raw_http
            .post(format!("{}/parse", REDUCTO_API_BASE))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(ReductoError::Api(res.status(), res.text().await?));
        }

        let parse_res: ReductoResponse = res.json().await?;

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
    pub async fn extract_fields(&self, pdf_path: &Path, schema: serde_json::Value) -> Result<serde_json::Value, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = ExtractRequest {
            input: file_id,
            instructions: ExtractInstructions { schema },
        };

        let res = self.raw_http
            .post(format!("{}/extract", REDUCTO_API_BASE))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(ReductoError::Api(res.status(), res.text().await?));
        }

        let extract_res: ReductoExtractResponse = res.json().await?;
        if extract_res.result.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(extract_res.result[0].clone())
    }

    /// Divide documents into named sections using natural language descriptions
    pub async fn split_document(&self, pdf_path: &Path, split_description: &str) -> Result<serde_json::Value, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = SplitRequest {
            input: file_id,
            split_description: split_description.to_string(),
        };

        let res = self.raw_http
            .post(format!("{}/split", REDUCTO_API_BASE))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(ReductoError::Api(res.status(), res.text().await?));
        }

        let split_res: ReductoResponse = res.json().await?;
        Ok(split_res.result.sections.unwrap_or(serde_json::Value::Null))
    }

    /// Classify documents by type before processing
    pub async fn classify_document(&self, pdf_path: &Path, categories: Vec<String>) -> Result<String, ReductoError> {
        let file_id = self.upload_document(pdf_path).await?;

        let req = ClassifyRequest {
            input: file_id,
            classification_schema: ClassifySchema { categories },
        };

        let res = self.raw_http
            .post(format!("{}/classify", REDUCTO_API_BASE))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(ReductoError::Api(res.status(), res.text().await?));
        }

        let classify_res: ReductoResponse = res.json().await?;
        Ok(classify_res.result.classification.unwrap_or_default())
    }

    pub async fn parse_statement(&self, pdf_path: &Path) -> Result<crate::ai::document_ai::BankStatement, ReductoError> {
        let chunks = self.parse_document(pdf_path).await?;
        let markdown = chunks.to_string(); 
        
        let mut statement = crate::ai::llamaparse::LlamaParseClient::from_app_config(&crate::app::config::AppConfig::default())
            .unwrap()
            .parse_markdown_to_statement(&markdown)
            .map_err(|e| ReductoError::System(e.to_string()))?;
        statement.ensure_canonical_metadata();
        Ok(statement)
    }

    pub async fn parse_statement_for_transfer(&self, pdf_path: &Path) -> Result<crate::ai::document_ai::BankStatement, ReductoError> {
        self.parse_statement(pdf_path).await
    }

    pub async fn edit_document(&self, pdf_path: &Path, edit_instructions: &str) -> Result<serde_json::Value, ReductoError> {
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

        let res = self.raw_http
            .post(format!("{}/edit", "https://platform.reducto.ai"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(ReductoError::Api(res.status(), res.text().await?));
        }

        let edit_res: ReductoResponse = res.json().await?;
        Ok(edit_res.result.chunks.unwrap_or(serde_json::Value::Null))
    }
}
