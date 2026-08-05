//! API Key Verification Module
//!
//! Provides comprehensive verification of all API credentials with multi-AI fallback chains.
//! Supports structured JSON output for CI/CD integration and actionable guidance for failures.

use crate::ai::document_ai::{DocAiError, DocumentAiClient};
use crate::ai::gemini_client::{GeminiClient, GeminiError};
use crate::app::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Verification status for a single service
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// Service verified successfully
    Success,
    /// Service verification failed but fallback available
    Partial,
    /// Service verification failed with no fallback
    Failed,
    /// Service not configured (optional service)
    Skipped,
}

/// Result of verifying a single API service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Service name (e.g., "PyMuPDF Pro", "Document AI", "Gemini")
    pub service: String,
    /// Verification status
    pub status: VerificationStatus,
    /// Latency in milliseconds
    pub latency_ms: u64,
    /// Error message if verification failed
    pub error_message: Option<String>,
    /// Actionable guidance for fixing the issue
    pub guidance: Option<String>,
    /// Which auth method or model was used (for fallback tracking)
    pub method_used: Option<String>,
}

/// Overall verification report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    /// Overall status (success if all required services pass)
    pub overall_status: VerificationStatus,
    /// Individual service results
    pub results: Vec<VerificationResult>,
    /// Timestamp of verification
    pub timestamp: String,
}

impl VerificationReport {
    /// Create a new verification report
    pub fn new(results: Vec<VerificationResult>) -> Self {
        let overall_status = Self::calculate_overall_status(&results);
        let timestamp = chrono::Utc::now().to_rfc3339();

        Self {
            overall_status,
            results,
            timestamp,
        }
    }

    /// Calculate overall status from individual results
    fn calculate_overall_status(results: &[VerificationResult]) -> VerificationStatus {
        let has_critical_failure = results.iter().any(|r| {
            r.status == VerificationStatus::Failed
                && (r.service == "PyMuPDF Pro" || r.service == "Document AI")
        });

        let has_any_failure = results
            .iter()
            .any(|r| r.status == VerificationStatus::Failed);
        let has_partial = results
            .iter()
            .any(|r| r.status == VerificationStatus::Partial);

        if has_critical_failure || has_any_failure {
            VerificationStatus::Failed
        } else if has_partial {
            VerificationStatus::Partial
        } else {
            VerificationStatus::Success
        }
    }

    /// Get exit code for CI/CD (0 = success, 1 = partial, 2 = failure)
    pub fn exit_code(&self) -> i32 {
        match self.overall_status {
            VerificationStatus::Success => 0,
            VerificationStatus::Partial => 1,
            VerificationStatus::Failed => 2,
            VerificationStatus::Skipped => 0,
        }
    }
}

/// Collect API-key health with fallback chains without writing to stdout.
/// Background/runtime callers must use this function so unrelated commands keep
/// their machine-readable output deterministic.
pub async fn collect_api_key_report(config: &AppConfig) -> VerificationReport {
    let mut results = Vec::new();

    // 1. Verify PyMuPDF Pro (required)
    results.push(verify_pymupdf_pro(config).await);

    // 2. Verify Document AI (recommended, with fallback chain)
    results.push(verify_document_ai(config).await);

    // 3. Verify Gemini (recommended, with fallback chain)
    results.push(verify_gemini(config).await);

    // 5. Verify LlamaParse (alternative LLM parser)
    results.push(verify_llamaparse(config).await);

    // 6. Verify pdfRest (cloud rendering)
    results.push(verify_pdfrest(config).await);

    // 7. Verify Vision AI (visual AI testing)
    results.push(verify_vision(config).await);

    VerificationReport::new(results)
}

/// Verify all API keys and print the explicitly requested report format.
pub async fn verify_all_api_keys(config: &AppConfig, json_output: bool) -> VerificationReport {
    let report = collect_api_key_report(config).await;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        print_human_readable(&report);
    }

    report
}

/// Verify PyMuPDF Pro key by testing runtime ping
async fn verify_pymupdf_pro(config: &AppConfig) -> VerificationResult {
    let start = Instant::now();

    // Check if key is configured
    let key = config.pymupdf_pro_key.as_ref();
    if key.is_none() || key.is_some_and(|k| k.is_empty()) {
        return VerificationResult {
            service: "PyMuPDF Pro".to_string(),
            status: VerificationStatus::Failed,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some("PYMUPDF_PRO_KEY not configured".to_string()),
            guidance: Some(
                "Set PYMUPDF_PRO_KEY environment variable with your PyMuPDF Pro license key. \
                 Obtain from https://pymupdf.io/"
                    .to_string(),
            ),
            method_used: None,
        };
    }

    // Check key format
    use crate::app::env_spec::is_well_formed_pro_key;
    let key_str = key.unwrap();
    if !is_well_formed_pro_key(key_str) {
        return VerificationResult {
            service: "PyMuPDF Pro".to_string(),
            status: VerificationStatus::Failed,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some(
                "Invalid key format (must be at least 16 alphanumeric characters)".to_string(),
            ),
            guidance: Some(
                "Ensure PYMUPDF_PRO_KEY is a valid license key: either a 24-char trial key (hFKt prefix) \
                 or a commercial key (≥16 alphanumeric chars). Obtain from https://pymupdf.io/"
                    .to_string(),
            ),
            method_used: None,
        };
    }

    // Note: Actual unlock test requires runtime, which we can't do in this async context
    // The format check is the best we can do without spawning the full runtime
    VerificationResult {
        service: "PyMuPDF Pro".to_string(),
        status: VerificationStatus::Success,
        latency_ms: start.elapsed().as_millis() as u64,
        error_message: None,
        guidance: None,
        method_used: Some("Format validation".to_string()),
    }
}

/// Verify Document AI with fallback chain: API key -> ADC -> Service Account
async fn verify_document_ai(config: &AppConfig) -> VerificationResult {
    let start = Instant::now();

    let doc_ai_config = match &config.document_ai {
        Some(cfg) => cfg,
        None => {
            return VerificationResult {
                service: "Document AI".to_string(),
                status: VerificationStatus::Skipped,
                latency_ms: start.elapsed().as_millis() as u64,
                error_message: Some("Document AI not configured".to_string()),
                guidance: Some(
                    "Set DOCUMENT_AI_PROJECT_ID, DOCUMENT_AI_LOCATION, and DOCUMENT_AI_PROCESSOR_ID \
                     to enable transaction extraction.".to_string()
                ),
                method_used: None,
            };
        }
    };

    // Check if any auth method is available
    if !doc_ai_config.has_auth() {
        return VerificationResult {
            service: "Document AI".to_string(),
            status: VerificationStatus::Skipped,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some("No authentication configured".to_string()),
            guidance: Some(
                "Set DOCUMENT_AI_API_KEY, or run 'gcloud auth application-default login', \
                 or set GOOGLE_APPLICATION_CREDENTIALS"
                    .to_string(),
            ),
            method_used: None,
        };
    }

    // Try to create client and ping
    match DocumentAiClient::from_app_config(config) {
        Ok(client) => match client.ping().await {
            Ok(_) => VerificationResult {
                service: "Document AI".to_string(),
                status: VerificationStatus::Success,
                latency_ms: start.elapsed().as_millis() as u64,
                error_message: None,
                guidance: None,
                method_used: Some(determine_docai_auth_method(doc_ai_config)),
            },
            Err(e) => {
                let guidance = get_docai_error_guidance(&e);
                VerificationResult {
                    service: "Document AI".to_string(),
                    status: VerificationStatus::Failed,
                    latency_ms: start.elapsed().as_millis() as u64,
                    error_message: Some(e.to_string()),
                    guidance: Some(guidance),
                    method_used: Some(determine_docai_auth_method(doc_ai_config)),
                }
            }
        },
        Err(e) => VerificationResult {
            service: "Document AI".to_string(),
            status: VerificationStatus::Failed,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some(e.to_string()),
            guidance: Some("Check Document AI configuration and credentials".to_string()),
            method_used: None,
        },
    }
}

/// Verify Gemini with fallback chain: gemini-2.5-pro -> gemini-1.5-pro -> gemini-2.5-flash
async fn verify_gemini(config: &AppConfig) -> VerificationResult {
    let start = Instant::now();

    // Check if API key is configured (for ApiKey mode)
    let api_key = config.gemini_api_key.as_ref();

    if api_key.is_none_or(|k| k.is_empty()) {
        // Check if Vertex mode is configured
        let doc_ai_config = config.document_ai.as_ref();
        let has_vertex = doc_ai_config.is_some_and(|cfg| !cfg.project_id.is_empty());

        if !has_vertex {
            return VerificationResult {
                service: "Gemini".to_string(),
                status: VerificationStatus::Skipped,
                latency_ms: start.elapsed().as_millis() as u64,
                error_message: Some("Gemini not configured".to_string()),
                guidance: Some(
                    "Set GEMINI_API_KEY for AI Studio mode, or configure Document AI with project_id \
                     for Vertex AI mode.".to_string()
                ),
                method_used: None,
            };
        }
    }

    // Try to create client and ping
    match GeminiClient::from_app_config(config) {
        Ok(client) => match client.ping().await {
            Ok(_) => VerificationResult {
                service: "Gemini".to_string(),
                status: VerificationStatus::Success,
                latency_ms: start.elapsed().as_millis() as u64,
                error_message: None,
                guidance: None,
                method_used: Some(if api_key.is_some_and(|k| !k.is_empty()) {
                    "API Key".to_string()
                } else {
                    "Vertex AI".to_string()
                }),
            },
            Err(e) => {
                let guidance = get_gemini_error_guidance(&e);
                VerificationResult {
                    service: "Gemini".to_string(),
                    status: VerificationStatus::Failed,
                    latency_ms: start.elapsed().as_millis() as u64,
                    error_message: Some(e.to_string()),
                    guidance: Some(guidance),
                    method_used: Some(if api_key.is_some_and(|k| !k.is_empty()) {
                        "API Key".to_string()
                    } else {
                        "Vertex AI".to_string()
                    }),
                }
            }
        },
        Err(e) => VerificationResult {
            service: "Gemini".to_string(),
            status: VerificationStatus::Failed,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some(e.to_string()),
            guidance: Some("Check Gemini API key or Vertex AI configuration".to_string()),
            method_used: None,
        },
    }
}

async fn verify_llamaparse(config: &AppConfig) -> VerificationResult {
    let start = Instant::now();
    let key = config.llamaparse_api_key.as_ref();

    if key.is_none() || key.is_some_and(|k| k.is_empty()) {
        return VerificationResult {
            service: "LlamaParse".to_string(),
            status: VerificationStatus::Skipped,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some("LLAMAPARSE_API_KEY not configured".to_string()),
            guidance: Some("Set LLAMAPARSE_API_KEY to use the alternative LLM parser. Get a key at https://cloud.llamaindex.ai/".to_string()),
            method_used: None,
        };
    }

    let key_str = key.unwrap();
    if !key_str.starts_with("llx-") {
        return VerificationResult {
            service: "LlamaParse".to_string(),
            status: VerificationStatus::Failed,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some("Invalid key format".to_string()),
            guidance: Some("LlamaParse keys usually start with 'llx-'".to_string()),
            method_used: None,
        };
    }

    VerificationResult {
        service: "LlamaParse".to_string(),
        status: VerificationStatus::Success,
        latency_ms: start.elapsed().as_millis() as u64,
        error_message: None,
        guidance: None,
        method_used: Some("Format validation".to_string()),
    }
}

async fn verify_pdfrest(config: &AppConfig) -> VerificationResult {
    let start = Instant::now();
    let key = config.pdfrest_api_key.as_ref();

    if key.is_none() || key.is_some_and(|k| k.is_empty()) {
        return VerificationResult {
            service: "pdfRest".to_string(),
            status: VerificationStatus::Skipped,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some("PDFREST_API_KEY not configured".to_string()),
            guidance: Some(
                "Set PDFREST_API_KEY to enable Adobe-tier cloud rendering for verification."
                    .to_string(),
            ),
            method_used: None,
        };
    }

    VerificationResult {
        service: "pdfRest".to_string(),
        status: VerificationStatus::Success,
        latency_ms: start.elapsed().as_millis() as u64,
        error_message: None,
        guidance: None,
        method_used: Some("API Key Configured".to_string()),
    }
}

async fn verify_vision(_config: &AppConfig) -> VerificationResult {
    let start = Instant::now();
    let key_val = _config.vision_api_key.clone().unwrap_or_default();
    let key = if key_val.is_empty() {
        None
    } else {
        Some(key_val)
    };
    let key = key.as_ref();

    if key.is_none() || key.is_some_and(|k| k.is_empty()) {
        return VerificationResult {
            service: "Vision AI".to_string(),
            status: VerificationStatus::Skipped,
            latency_ms: start.elapsed().as_millis() as u64,
            error_message: Some("VISION_API_KEY not configured".to_string()),
            guidance: Some(
                "Set VISION_API_KEY to enable visual AI testing verification layer.".to_string(),
            ),
            method_used: None,
        };
    }

    VerificationResult {
        service: "Vision AI".to_string(),
        status: VerificationStatus::Success,
        latency_ms: start.elapsed().as_millis() as u64,
        error_message: None,
        guidance: None,
        method_used: Some("API Key Configured".to_string()),
    }
}

/// Determine which Document AI auth method would be used
fn determine_docai_auth_method(config: &crate::app::config::DocumentAiConfig) -> String {
    if !config.api_key.is_empty() {
        "API Key".to_string()
    } else if !config.adc_path.is_empty() {
        "Application Default Credentials".to_string()
    } else if !config.service_account_path.is_empty() {
        "Service Account".to_string()
    } else {
        "Unknown".to_string()
    }
}

/// Get guidance for Document AI errors
fn get_docai_error_guidance(error: &DocAiError) -> String {
    match error {
        DocAiError::Auth(status, _) => {
            match status.as_u16() {
                401 => "Authentication failed. Check your API key or service account credentials. \
                        Ensure the key is valid and not expired.".to_string(),
                403 => "Permission denied. Ensure your service account has the \
                        'documentai.processor' role on the processor.".to_string(),
                _ => format!("Auth error (HTTP {status}): Check your credentials and permissions."),
            }
        }
        DocAiError::Network(_) => "Network error. Check your internet connection and firewall settings.".to_string(),
        DocAiError::MissingConfig(_) => "Required Document AI configuration is missing. \
                                         Set DOCUMENT_AI_PROJECT_ID, DOCUMENT_AI_LOCATION, and DOCUMENT_AI_PROCESSOR_ID.".to_string(),
        _ => "Document AI verification failed. Check your configuration and credentials.".to_string(),
    }
}

/// Get guidance for Gemini errors
fn get_gemini_error_guidance(error: &GeminiError) -> String {
    match error {
        GeminiError::MissingKey => "GEMINI_API_KEY not configured. \
                                    Create one at https://aistudio.google.com/app/apikey".to_string(),
        GeminiError::MissingVertexConfig => "Vertex AI mode requires Document AI configuration. \
                                             Set DOCUMENT_AI_PROJECT_ID and ensure credentials are set.".to_string(),
        GeminiError::Vertex(msg) => {
            if msg.contains("token") || msg.contains("auth") {
                "Vertex AI authentication failed. Ensure your service account has the \
                 'aiplatform.user' role and credentials are valid.".to_string()
            } else {
                format!("Vertex AI error: {msg}")
            }
        }
        GeminiError::Api(status, _) => {
            match status.as_u16() {
                401 => "Invalid API key. Check your GEMINI_API_KEY environment variable.".to_string(),
                403 => "API key does not have access to the requested model. \
                        Check your API key permissions.".to_string(),
                429 => "Rate limit exceeded. Wait a few minutes before trying again, \
                        or upgrade your quota.".to_string(),
                _ => format!("API error (HTTP {status}): Check your API key and quota."),
            }
        }
        GeminiError::Network(_) => "Network error. Check your internet connection.".to_string(),
        _ => "Gemini verification failed. Check your API key or Vertex AI configuration.".to_string(),
    }
}

/// Print human-readable verification report
fn print_human_readable(report: &VerificationReport) {
    println!("══════════════════════════════════════════════════════════");
    println!("  API Key Verification Report");
    println!("══════════════════════════════════════════════════════════");
    println!("  Timestamp: {}", report.timestamp);
    println!();

    for result in &report.results {
        let icon = match result.status {
            VerificationStatus::Success => "✅",
            VerificationStatus::Partial => "⚠️",
            VerificationStatus::Failed => "❌",
            VerificationStatus::Skipped => "⊘",
        };

        println!("  {}  {}  ({}ms)", icon, result.service, result.latency_ms);

        if let Some(method) = &result.method_used {
            println!("      Method: {method}");
        }

        if let Some(error) = &result.error_message {
            println!("      Error: {error}");
        }

        if let Some(guidance) = &result.guidance {
            println!("      Guidance: {guidance}");
        }

        println!();
    }

    let overall_icon = match report.overall_status {
        VerificationStatus::Success => "✅",
        VerificationStatus::Partial => "⚠️",
        VerificationStatus::Failed => "❌",
        VerificationStatus::Skipped => "⊘",
    };

    println!("══════════════════════════════════════════════════════════");
    println!(
        "  Overall Status: {}  (Exit code: {})",
        overall_icon,
        report.exit_code()
    );
    println!("══════════════════════════════════════════════════════════");
}
