//! Extended HTTP API for BankFidelity
//!
//! This module extends the minimal health-check server with a full REST API
//! that exposes every job type as a POST endpoint. It enables:
//!
//! - Natural language chat control via `/chat`
//! - Direct job dispatch via `/job`
//! - API key management via `/keys`
//! - Status and diagnostics via `/status`
//!
//! ## Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET | /health | Liveness probe |
//! | GET | /readyz | Readiness probe |
//! | GET | /status | Full runtime status + capabilities |
//! | POST | /chat | Natural language command (returns job result) |
//! | POST | /job | Direct job dispatch by job type |
//! | POST | /keys | Update API keys at runtime |
//! | GET | /keys | List configured providers (no key values) |
//! | POST | /extract | Extract transactions from a PDF |
//! | POST | /verify | Run pixel-perfect fidelity check |
//! | POST | /transfer | Transfer transactions between PDFs |
//! | POST | /balance | Run Smart Balance Engine |
//! | POST | /dates | Adjust transaction dates |
//! | GET | /tools | List all available MCP tools |
//!
//! ## Chat API
//!
//! The `/chat` endpoint accepts a JSON body:
//! ```json
//! {
//!   "message": "Change the account holder name to John Smith",
//!   "pdf_path": "/path/to/statement.pdf",
//!   "provider": "gemini",
//!   "auto_apply": false
//! }
//! ```
//!
//! And returns:
//! ```json
//! {
//!   "intent": "ai_edit",
//!   "description": "AI edit via gemini — \"Change the account holder name to John Smith\"",
//!   "status": "queued",
//!   "job_id": "abc123"
//! }
//! ```

// This module is intentionally kept as documentation + type definitions.
// The actual HTTP routing is implemented in server.rs using the RuntimeChannel.
// The full implementation is in the Python MCP server (scripts/mcp_server.py)
// which provides immediate usability without requiring a Rust recompile.

/// Chat request body for the `/chat` endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct ChatRequest {
    /// Natural language instruction, e.g. "Change all dates to February"
    pub message: String,
    /// Path to the PDF to operate on (optional for non-document commands)
    pub pdf_path: Option<String>,
    /// AI provider to use: "gemini" | "mistral" | "local-llm" | "offline"
    #[serde(default = "default_provider")]
    pub provider: String,
    /// If true, apply changes immediately without confirmation
    #[serde(default)]
    pub auto_apply: bool,
}

fn default_provider() -> String { "gemini".to_string() }

/// Job dispatch request body for the `/job` endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct JobRequest {
    /// Job type: "extract_transactions" | "balance" | "verify" | "transfer" | etc.
    pub job: String,
    /// Path to the primary PDF
    pub path: Option<String>,
    /// Additional job-specific parameters
    #[serde(flatten)]
    pub params: serde_json::Value,
}

/// API key update request for the `/keys` endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct KeyUpdateRequest {
    /// Provider name: "gemini" | "mistral" | "llamaparse" | "pymupdf_pro" | "openrouter" | "pdfrest"
    pub provider: String,
    /// The new API key value
    pub api_key: String,
}

/// Standard API response envelope.
#[derive(Debug, serde::Serialize)]
pub struct ApiResponse<T: serde::Serialize> {
    pub status: String,
    pub data: Option<T>,
    pub error: Option<String>,
    pub job_id: Option<String>,
}

impl<T: serde::Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self { status: "ok".into(), data: Some(data), error: None, job_id: None }
    }
    pub fn err(msg: impl Into<String>) -> ApiResponse<serde_json::Value> {
        ApiResponse { status: "error".into(), data: None, error: Some(msg.into()), job_id: None }
    }
}
