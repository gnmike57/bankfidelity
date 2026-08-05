use std::env;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use serde::{Deserialize, Serialize};

use crate::app::env_spec::is_well_formed_pro_key;
use crate::error::{ConfigError, ConfigResult};
use zeroize::Zeroize;

/// Minimum passphrase length for security (16 characters)
const MIN_PASSPHRASE_LENGTH: usize = 16;

/// Minimum passphrase length for development mode
const DEV_PASSPHRASE_MIN_LENGTH: usize = 8;

pub static HTTP_CLIENT: OnceLock<ClientWithMiddleware> = OnceLock::new();

pub fn global_http_client() -> ClientWithMiddleware {
    HTTP_CLIENT.get_or_init(|| {
        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let reqwest_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build();

        let reqwest_client = match reqwest_client {
            Ok(client) => client,
            Err(e) => {
                tracing::error!("[config] Failed to build HTTP client: {}. Using default client as fallback.", e);
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(60))
                    .build()
                    .unwrap_or_else(|e| {
                        tracing::error!("[config] Failed to build fallback HTTP client: {}. This is critical.", e);
                        std::process::exit(1);
                    })
            }
        };

        ClientBuilder::new(reqwest_client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build()
    }).clone()
}

/// Availability of PyMuPDF Pro per-segment editing/rendering (Subsystem B),
/// derived solely from the `PYMUPDF_PRO_KEY` value the application read.
///
/// This status governs **only** the high-fidelity per-segment edit/render
/// path. It deliberately has **no bearing** on `lopdf` split/merge
/// (Subsystem A), which runs in every runtime environment regardless of key
/// state (see [`AppConfig::pro_editing_available`]).
///
/// # Offline expiry caveat
/// A Pro key's expiry can only be confirmed by PyMuPDF at unlock time. There
/// is no offline expiry check, so a present, well-formed key is reported as
/// [`ProKeyStatus::Available`]; absence or a malformed value is reported as
/// [`ProKeyStatus::Unavailable`]. A well-formed-but-expired key cannot be
/// distinguished here and will surface its failure later, at unlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProKeyStatus {
    /// A present, well-formed `PYMUPDF_PRO_KEY` was found; per-segment editing
    /// is expected to work (subject to unlock-time expiry verification).
    Available,
    /// No key, or a malformed key, was found; per-segment editing is
    /// unavailable. Splitting and merging remain available regardless.
    Unavailable,
}

impl ProKeyStatus {
    /// Returns `true` only when per-segment editing is available.
    pub fn is_available(&self) -> bool {
        matches!(self, ProKeyStatus::Available)
    }

    /// A human-readable, GUI/`serve`-friendly explanation of the status.
    pub fn reason(&self) -> &'static str {
        match self {
            ProKeyStatus::Available => {
                "Per-segment editing is available: a well-formed PYMUPDF_PRO_KEY was found \
                 (expiry is verified by PyMuPDF at unlock time)."
            }
            ProKeyStatus::Unavailable => {
                "Per-segment editing is unavailable: PYMUPDF_PRO_KEY is absent or malformed. \
                 Splitting and merging remain available."
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DocumentAiConfig {
    pub project_id: String,
    pub location: String,
    pub processor_id: String,
    /// Optional path to a Google Cloud Service Account JSON key (legacy auth).
    /// If empty, the client falls back to API-key auth (`api_key` field).
    pub service_account_path: String,
    /// Optional Document AI API key (Beta). Takes precedence over OAuth when set.
    pub api_key: String,
    /// Optional path to Application Default Credentials JSON (set by
    /// `gcloud auth application-default login`). Auto-detected from the
    /// platform's well-known location when not set explicitly.
    pub adc_path: String,
    /// GCS URI for batch process outputs (e.g. gs://my-bucket/outputs/).
    pub gcs_output_uri: String,
    /// Passphrase for encrypting the local Document AI cache
    pub passphrase: String,
}

/// How the Gemini calls authenticate.
///
/// `ApiKey` is the simplest (AI Studio `AIza...` key, default) and `Vertex`
/// is the enterprise option that authenticates with a Google Cloud service
/// account (or ADC) and calls the Vertex AI Gemini endpoint. Vertex keeps
/// data inside your GCP project and does not require an API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeminiAuthMode {
    /// AI Studio API key (`generativelanguage.googleapis.com?key=...`).
    #[default]
    ApiKey,
    /// Vertex AI (`{location}-aiplatform.googleapis.com`) authenticated via a
    /// Google Cloud service account / ADC token. No API key used.
    Vertex,
}

impl DocumentAiConfig {
    /// Returns true if the Document AI configuration has valid authentication.
    pub fn has_auth(&self) -> bool {
        !self.api_key.is_empty()
            || !self.adc_path.is_empty()
            || !self.service_account_path.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfEngineMode {
    /// Run both engines concurrently for read operations, preferring the
    /// primary (PyMuPDF) result when both succeed and transparently falling
    /// back to whichever engine survives if one fails.
    DualConcurrent,
    /// Primary-first (PyMuPDF), fall back to native on error (sequential).
    /// This is the default: PyMuPDF has the highest fidelity and always works
    /// out of the box; the native engine (pdfium) is used as a fallback when
    /// PyMuPDF is unavailable.
    #[default]
    PyMuPdfProPrimary,
    NativeOnly,
    /// Force PyMuPDF (highest fidelity edit-in-place).
    PyMuPdfOnly,
    /// Legacy persisted value. Reconstruction is not an edit-in-place fidelity engine
    /// and cannot be selected for the v1 editing workflow.
    TypstReconstruct,
}

impl PdfEngineMode {
    pub const fn is_fidelity_selectable(self) -> bool {
        !matches!(self, Self::TypstReconstruct)
    }
}

// ---------------------------------------------------------------------------
// Backend preference enums (persisted in AppSettings, used by runtime)
// ---------------------------------------------------------------------------

/// Which AI provider to use for balance analysis, completeness checks, and
/// vision validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderMode {
    /// Skip AI entirely - manual-only editing with no AI balance/vision calls.
    ManualOnly,
    /// Google Gemini via AI Studio API key.
    GeminiApiKey,
    /// Google Gemini via Vertex AI (enterprise, uses service-account / ADC).
    GeminiVertex,
    /// Groq API (extremely fast Llama 3 inference, free tier available).
    #[default]
    GroqApiKey,
    /// OpenRouter API (access to DeepSeek and hundreds of other models).
    OpenRouterApiKey,
    /// Mistral AI API (Mistral's native endpoint).
    MistralApiKey,
}

impl AiProviderMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::GeminiApiKey => "Gemini (API Key)",
            Self::GeminiVertex => "Gemini (Vertex AI)",
            Self::GroqApiKey => "Groq (Llama 3 / Fast)",
            Self::OpenRouterApiKey => "OpenRouter (DeepSeek)",
            Self::MistralApiKey => "Mistral AI",
            Self::ManualOnly => "Manual Only (No AI)",
        }
    }

    /// Token written to `.env` / `AI_PROVIDER` and read back by
    /// [`AppConfig::from_env`]. Matches the serde `rename_all = "snake_case"`
    /// representation of each variant.
    pub fn env_token(self) -> &'static str {
        match self {
            Self::GeminiApiKey => "gemini_api_key",
            Self::GeminiVertex => "gemini_vertex",
            Self::GroqApiKey => "groq_api_key",
            Self::OpenRouterApiKey => "openrouter_api_key",
            Self::MistralApiKey => "mistral_api_key",
            Self::ManualOnly => "manual_only",
        }
    }

    /// Tolerant parse from an environment variable string. Accepts the
    /// canonical serde snake_case tokens and convenient short aliases.
    /// Returns `ManualOnly` on empty/unknown input.
    pub fn from_env_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "gemini_api_key" | "gemini" => Self::GeminiApiKey,
            "gemini_vertex" | "vertex" | "vertex_ai" => Self::GeminiVertex,
            "groq_api_key" | "groq" => Self::GroqApiKey,
            "openrouter_api_key" | "openrouter" => Self::OpenRouterApiKey,
            "mistral_api_key" | "mistral" | "mistralai" => Self::MistralApiKey,
            "manual_only" | "manual" => Self::ManualOnly,
            _ => Self::ManualOnly,
        }
    }
}

/// Which document parser to use for extracting transactions from PDFs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentParserMode {
    /// LlamaParse (API-based document parser using LLMs for extraction).
    #[default]
    LlamaParse,
    /// Pure Rust heuristic parsing (regex + layout), highly accurate for standard banking formats.
    OfflineHeuristic,

    /// Local OCR via `ocrs` + `rten` (pure Rust, works offline on scanned
    /// documents, requires `--features ocr`).
    LocalOcrs,
    /// Google Document AI (highest accuracy on trained layouts, requires
    /// GCP credentials). First fallback when Mindee is unavailable.
    DocumentAi,
}

impl DocumentParserMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::DocumentAi => "Google Document AI",

            Self::LlamaParse => "LlamaParse",
            Self::OfflineHeuristic => "Offline Heuristic",

            Self::LocalOcrs => "Local OCR (not available for PDF workflow)",
        }
    }

    pub const fn is_v1_selectable(self) -> bool {
        !matches!(self, Self::LocalOcrs)
    }
}

/// Which renderer to use for verification (visual diff) of edited PDFs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMode {
    /// Local Pdfium rendering (default, fast, no network).
    #[default]
    LocalPdfium,
    /// pdfRest cloud rendering (Adobe-tier fidelity, requires API key).
    PdfRestCloud,
}

impl VerificationMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalPdfium => "Local (Pdfium)",
            Self::PdfRestCloud => "pdfRest (Cloud)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub gemini_api_key: Option<String>,
    pub pdfrest_api_key: Option<String>,
    pub lipi_api_key: Option<String>,
    pub groq_api_key: Option<String>,
    pub openrouter_api_key: Option<String>,
    pub mistral_api_key: Option<String>,
    pub mindee_api_key: Option<String>,
    pub vision_api_key: Option<String>,
    pub openrouter_model: String,
    pub mistral_model: String,
    pub ai_provider: AiProviderMode,
    pub document_ai: Option<DocumentAiConfig>,
    pub pymupdf_pro_key: Option<String>, // Changed to Option - must come from env
    pub passphrase: String,
    pub otel_endpoint: Option<String>,
    pub otel_service_name: String,
    pub log_dir: PathBuf,
    pub webhook_url: Option<String>,
    /// How Gemini authenticates: AI Studio API key (default) or Vertex AI
    /// (service-account / ADC token). Set in-app via the Credentials panel
    /// or by `GEMINI_AUTH_MODE=vertex` in the environment.
    pub gemini_auth_mode: GeminiAuthMode,
    /// Whether we're in development mode (relaxed security requirements)
    pub is_dev_mode: bool,
    /// Which PDF engine backend to use
    pub engine_mode: PdfEngineMode,
    pub llamaparse_api_key: Option<String>,
    /// Whether to prompt the user with a modal during semi-failures for manual fallback selection.
    pub interactive_fallbacks: bool,
    pub transfer_consensus_mode: bool,
    pub auto_match_dpi: bool,
}

impl Drop for AppConfig {
    fn drop(&mut self) {
        if let Some(key) = &mut self.gemini_api_key {
            key.zeroize();
        }
        if let Some(key) = &mut self.pdfrest_api_key {
            key.zeroize();
        }
        if let Some(key) = &mut self.lipi_api_key {
            key.zeroize();
        }
        if let Some(key) = &mut self.groq_api_key {
            key.zeroize();
        }
        if let Some(key) = &mut self.openrouter_api_key {
            key.zeroize();
        }
        if let Some(key) = &mut self.mistral_api_key {
            key.zeroize();
        }
        if let Some(key) = &mut self.mindee_api_key {
            key.zeroize();
        }
        if let Some(key) = &mut self.vision_api_key {
            key.zeroize();
        }
        if let Some(doc) = &mut self.document_ai {
            doc.api_key.zeroize();
            doc.service_account_path.zeroize();
        }
        if let Some(key) = &mut self.pymupdf_pro_key {
            key.zeroize();
        }
        self.passphrase.zeroize();
        if let Some(key) = &mut self.llamaparse_api_key {
            key.zeroize();
        }

        tracing::debug!("[config] AppConfig dropped, sensitive fields zeroized.");
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            gemini_api_key: None,
            pdfrest_api_key: None,
            lipi_api_key: None,
            groq_api_key: None,
            openrouter_api_key: None,
            mistral_api_key: None,
            mindee_api_key: None,
            vision_api_key: None,
            openrouter_model: "mistralai/mistral-nemo:free".to_string(),
            mistral_model: "mistral-large-latest".to_string(),
            ai_provider: AiProviderMode::default(),
            document_ai: None,

            pymupdf_pro_key: None,
            passphrase: "DEV_PASSPHRASE".into(),
            otel_endpoint: None,
            otel_service_name: "dual-core-pdf-pipeline".into(),
            log_dir: PathBuf::from("./logs"),
            webhook_url: None,
            gemini_auth_mode: GeminiAuthMode::ApiKey,
            is_dev_mode: cfg!(debug_assertions),
            engine_mode: PdfEngineMode::PyMuPdfProPrimary,
            llamaparse_api_key: None,
            interactive_fallbacks: true,
            transfer_consensus_mode: true,
            auto_match_dpi: true, // Force high fidelity font replication default
        }
    }
}

#[derive(Clone)]
pub struct ConfigSnapshot {
    generation: u64,
    config: Arc<AppConfig>,
}

impl ConfigSnapshot {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn config(&self) -> Arc<AppConfig> {
        self.config.clone()
    }
}

#[derive(Clone)]
pub struct ConfigManager {
    current: Arc<RwLock<ConfigSnapshot>>,
}

impl ConfigManager {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self {
            current: Arc::new(RwLock::new(ConfigSnapshot {
                generation: 0,
                config,
            })),
        }
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        self.current
            .read()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    pub fn replace(&self, config: AppConfig) -> ConfigSnapshot {
        let mut current = self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next = ConfigSnapshot {
            generation: current.generation.saturating_add(1),
            config: Arc::new(config),
        };
        *current = next.clone();
        next
    }

    pub fn reload_from_env(&self) -> ConfigResult<ConfigSnapshot> {
        let _ = dotenvy::dotenv_override();
        AppConfig::from_env().map(|config| self.replace(config))
    }
}

impl AppConfig {
    /// Loads configuration from environment variables.
    ///
    /// # Errors
    /// Returns `ConfigError` if required variables are missing or invalid.
    pub fn from_env() -> ConfigResult<Self> {
        let is_dev_mode = cfg!(debug_assertions);

        let clean_key = |key: Result<String, env::VarError>| -> Option<String> {
            key.ok()
                .map(|s| s.trim_matches('"').trim_matches('\'').trim().to_string())
                .filter(|s| !s.is_empty())
        };

        // Optional API keys
        let gemini_api_key = clean_key(env::var("GEMINI_API_KEY"));
        let groq_api_key = clean_key(env::var("GROQ_API_KEY"));
        let openrouter_api_key = clean_key(env::var("OPENROUTER_API_KEY"));
        let mistral_api_key = clean_key(env::var("MISTRAL_API_KEY"));
        let openrouter_model = clean_key(env::var("OPENROUTER_MODEL"))
            .unwrap_or_else(|| "mistralai/mistral-nemo:free".to_string());
        let mistral_model = clean_key(env::var("MISTRAL_MODEL"))
            .unwrap_or_else(|| "mistral-large-latest".to_string());
        let pdfrest_api_key = clean_key(env::var("PDFREST_API_KEY"));
        let lipi_api_key = clean_key(env::var("LIPI_API_KEY"));
        let mindee_api_key = clean_key(env::var("MINDEE_API_KEY"));
        let vision_api_key = clean_key(env::var("VISION_API_KEY"));

        let llamaparse_api_key = clean_key(env::var("LLAMAPARSE_API_KEY"));
        let webhook_url = clean_key(env::var("WEBHOOK_URL"));

        // Document AI configuration
        let proj = clean_key(env::var("DOCUMENT_AI_PROJECT_ID"));
        let loc = clean_key(env::var("DOCUMENT_AI_LOCATION"));
        let proc_id = clean_key(env::var("DOCUMENT_AI_PROCESSOR_ID"));
        let sa_path = clean_key(env::var("GOOGLE_APPLICATION_CREDENTIALS"));
        let api_key = clean_key(env::var("DOCUMENT_AI_API_KEY"));
        let adc_path = detect_adc_path();
        let gcs_output_uri = clean_key(env::var("DOCUMENT_AI_GCS_URI")).unwrap_or_default();

        let document_ai = match (proj, loc, proc_id) {
            (Some(project_id), Some(location), Some(processor_id))
                if api_key.is_some() || sa_path.is_some() || adc_path.is_some() =>
            {
                Some(DocumentAiConfig {
                    project_id,
                    location,
                    processor_id,
                    service_account_path: sa_path.unwrap_or_default(),
                    api_key: api_key.unwrap_or_default(),
                    adc_path: adc_path.unwrap_or_default(),
                    gcs_output_uri,
                    passphrase: String::new(), // Filled in by AppConfig later
                })
            }
            _ => None,
        };

        // PyMuPDF Pro key - required in production
        let pymupdf_pro_key = clean_key(env::var("PYMUPDF_PRO_KEY"));

        // Gemini auth mode: default API key, opt into Vertex via env.
        let gemini_auth_mode = match env::var("GEMINI_AUTH_MODE")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "vertex" | "vertex_ai" | "vertexai" => GeminiAuthMode::Vertex,
            _ => GeminiAuthMode::ApiKey,
        };

        // Passphrase - required
        let passphrase = env::var("DUAL_CORE_PASSPHRASE")
            .map(|s| s.trim_matches('"').trim_matches('\'').trim().to_string())
            .map_err(|_| ConfigError::MissingRequired("DUAL_CORE_PASSPHRASE".to_string()))?;

        // Validate passphrase length
        let min_length = if is_dev_mode {
            DEV_PASSPHRASE_MIN_LENGTH
        } else {
            MIN_PASSPHRASE_LENGTH
        };

        if passphrase.len() < min_length {
            return Err(ConfigError::invalid_value(
                "DUAL_CORE_PASSPHRASE",
                format!(
                    "must be at least {} characters (got {})",
                    min_length,
                    passphrase.len()
                ),
            ));
        }

        // Optional OTEL configuration
        let otel_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|s| !s.is_empty());
        let otel_service_name =
            env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "dual-core-pdf-pipeline".to_string());

        // Log directory - validate it can be created
        let log_dir = env::var("LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./logs"));

        // Try to create the log directory to catch permission issues early
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            return Err(ConfigError::invalid_value(
                "LOG_DIR",
                format!("cannot create directory: {e}"),
            ));
        }

        let mut doc_ai = document_ai;
        if let Some(ref mut d) = doc_ai {
            d.passphrase = passphrase.clone();
        }

        // AI provider selection: tolerant parse, mirrors GEMINI_AUTH_MODE pattern.
        let ai_provider =
            AiProviderMode::from_env_str(&env::var("AI_PROVIDER").unwrap_or_default());

        Ok(Self {
            gemini_api_key,
            groq_api_key,
            openrouter_api_key,
            mistral_api_key,
            mindee_api_key,
            vision_api_key,
            openrouter_model,
            mistral_model,
            ai_provider,
            pdfrest_api_key,
            lipi_api_key,
            document_ai: doc_ai,

            llamaparse_api_key,
            pymupdf_pro_key,
            passphrase,
            otel_endpoint,
            otel_service_name,
            log_dir,
            webhook_url,
            gemini_auth_mode,
            is_dev_mode,
            engine_mode: match env::var("PDF_ENGINE_MODE")
                .unwrap_or_default()
                .to_lowercase()
                .as_str()
            {
                "native" => PdfEngineMode::NativeOnly,
                "pymupdf" => PdfEngineMode::PyMuPdfOnly,
                "auto" => PdfEngineMode::PyMuPdfProPrimary,
                // Typst is a non-fidelity reconstruction export, never an edit engine.
                "typst" => PdfEngineMode::PyMuPdfProPrimary,
                "dual" | "dual_concurrent" => PdfEngineMode::DualConcurrent,
                _ => PdfEngineMode::PyMuPdfProPrimary,
            },
            interactive_fallbacks: env::var("INTERACTIVE_FALLBACKS")
                .map(|v| v.to_lowercase() != "false" && v != "0")
                .unwrap_or(true),
            transfer_consensus_mode: env::var("TRANSFER_CONSENSUS_MODE")
                .map(|v| v.to_lowercase() != "false" && v != "0")
                .unwrap_or(true),
            auto_match_dpi: env::var("AUTO_MATCH_DPI")
                .map(|v| v.to_lowercase() != "false" && v != "0")
                .unwrap_or(true), // Enforce high fidelity default
        })
    }

    /// Validates the configuration and returns errors for any missing required items.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.pymupdf_pro_key.is_none() && !self.is_dev_mode {
            errors.push("PYMUPDF_PRO_KEY environment variable is not set".to_string());
        }

        if self.passphrase.len() < MIN_PASSPHRASE_LENGTH && !self.is_dev_mode {
            errors.push(format!(
                "DUAL_CORE_PASSPHRASE must be at least {MIN_PASSPHRASE_LENGTH} characters"
            ));
        }

        errors
    }

    /// Reports whether PyMuPDF Pro per-segment editing/rendering (Subsystem B)
    /// is available, based on the `PYMUPDF_PRO_KEY` this config holds.
    ///
    /// A key is considered available when it is present and well-formed:
    /// either a 24-character trial key with the `hFKt` prefix, or a
    /// commercial license key of at least 16 characters without whitespace
    /// (PyMuPDFPro 1.28.0+ format; actual validation is at unlock time).
    /// Because expiry cannot be verified offline, a well-formed key is treated
    /// as available and any expiry failure surfaces later at PyMuPDF unlock
    /// time. Absence or a malformed value yields [`ProKeyStatus::Unavailable`]
    /// (Requirements 11.1, 11.3, 21.1, 21.2, 21.3, 21.6).
    ///
    /// # Subsystem isolation
    /// This status governs **only** per-segment editing/rendering. The
    /// `lopdf` split/merge engine (Subsystem A) does **not** consult this and
    /// runs regardless of key state in every runtime environment - local GUI,
    /// local `serve`, and the Railway `pdfsitch` deployment (Requirements
    /// 11.2, 21.5). Nothing in this method or its callers should be used to
    /// gate splitting or merging.
    pub fn pro_key_status(&self) -> ProKeyStatus {
        match self.pymupdf_pro_key.as_deref() {
            Some(key) if is_well_formed_pro_key(key) => ProKeyStatus::Available,
            _ => ProKeyStatus::Unavailable,
        }
    }

    /// Convenience boolean form of [`AppConfig::pro_key_status`]: `true` when
    /// per-segment editing is available.
    ///
    /// This MUST NOT be used to gate splitting or merging - those run
    /// regardless of Pro-key state (Requirements 11.2, 21.5).
    pub fn pro_editing_available(&self) -> bool {
        self.pro_key_status().is_available()
    }

    /// A human-readable reason describing the current Pro-key availability,
    /// suitable for GUI status display or a headless `serve` return value
    /// (Requirements 11.3, 21.6).
    pub fn pro_editing_status_reason(&self) -> &'static str {
        self.pro_key_status().reason()
    }

    /// Returns true if the application has a valid AI provider configured for
    /// balance analysis. Provider-aware: any configured AI backend counts
    /// (Gemini API key, Gemini Vertex, Groq, or OpenRouter).
    ///
    /// pdfRest (a rendering backend) is intentionally excluded — it is
    /// unrelated to balancing.
    pub fn has_ai_for_balancing(&self) -> bool {
        match self.ai_provider {
            AiProviderMode::ManualOnly => false,
            AiProviderMode::GeminiApiKey => self.gemini_api_key.is_some(),
            AiProviderMode::GeminiVertex => self
                .document_ai
                .as_ref()
                .map(|d| !d.service_account_path.is_empty() || !d.adc_path.is_empty())
                .unwrap_or(false),
            AiProviderMode::GroqApiKey => self.groq_api_key.is_some(),
            AiProviderMode::OpenRouterApiKey => self.openrouter_api_key.is_some(),
            AiProviderMode::MistralApiKey => self.mistral_api_key.is_some(),
        }
    }

    /// Returns true if the application has valid AI configuration for extraction.
    pub fn has_ai_for_extraction(&self) -> bool {
        self.document_ai.is_some() || self.llamaparse_api_key.is_some()
    }

    /// Detect which API backends have valid keys configured.
    ///
    /// Called at boot and after every `ReloadConfig` so the UI can grey-out
    /// unavailable options (with a message) and the runtime can skip them
    /// in fallback chains. This is a cheap, local-only check - it does NOT
    /// make network calls to verify the key is actually accepted by the
    /// remote service.
    pub fn detect_availability(&self) -> ApiAvailability {
        ApiAvailability {
            gemini_api_key: self.gemini_api_key.is_some(),
            groq_api_key: self.groq_api_key.is_some(),
            openrouter_api_key: self.openrouter_api_key.is_some(),
            mistral_api_key: self.mistral_api_key.is_some(),
            gemini_vertex: self
                .document_ai
                .as_ref()
                .map(|d| !d.service_account_path.is_empty() || !d.adc_path.is_empty())
                .unwrap_or(false),
            document_ai: self
                .document_ai
                .as_ref()
                .map(|d| d.has_auth())
                .unwrap_or(false),

            llamaparse: self.llamaparse_api_key.is_some(),
            pdfrest: self.pdfrest_api_key.is_some(),
            pymupdf_pro: self.pro_editing_available(),
            vision_ai: std::env::var("VISION_API_KEY").is_ok_and(|k| !k.is_empty()),
            ocr: cfg!(feature = "ocr")
                && std::path::Path::new("models/text-detection.rten").exists()
                && std::path::Path::new("models/text-recognition.rten").exists(),
        }
    }
}

// ---------------------------------------------------------------------------
// Boot-time API availability detection
// ---------------------------------------------------------------------------

/// Which API backends have valid keys configured. Computed at boot and
/// refreshed on every `ReloadConfig`. The UI uses this to grey-out
/// unavailable options with explanatory messages, and the runtime uses it
/// to skip unavailable backends in automatic fallback chains.
///
/// A `true` value means the key *exists and is non-empty* - it does NOT
/// guarantee the remote service will accept it (e.g. an expired or revoked
/// key still reads as `true` here). Actual acceptance is verified lazily
/// when the backend is first invoked.
#[derive(Debug, Clone, Default)]
pub struct ApiAvailability {
    /// `GEMINI_API_KEY` is set (AI Studio mode).
    pub gemini_api_key: bool,
    pub groq_api_key: bool,
    pub openrouter_api_key: bool,
    pub mistral_api_key: bool,
    /// A service-account or ADC path is configured for Vertex AI.
    pub gemini_vertex: bool,
    /// Google Document AI processor + auth are fully configured.
    pub document_ai: bool,
    /// `MINDEE_API_KEY` is set.

    /// `LLAMAPARSE_API_KEY` is set.
    pub llamaparse: bool,
    /// `PDFREST_API_KEY` is set.
    pub pdfrest: bool,
    /// `PYMUPDF_PRO_KEY` is set and well-formed.
    pub pymupdf_pro: bool,
    /// `VISION_API_KEY` is set.
    pub vision_ai: bool,
    /// Local OCR is available: `ocr` Cargo feature enabled AND
    /// `models/text-detection.rten` + `models/text-recognition.rten` present.
    pub ocr: bool,
}

impl ApiAvailability {
    pub fn disable_service(&mut self, service_name: &str) {
        match service_name.to_lowercase().as_str() {
            "gemini" => {
                self.gemini_api_key = false;
                self.gemini_vertex = false;
            }
            "groq" => self.groq_api_key = false,
            "openrouter" => self.openrouter_api_key = false,
            "mistral" => self.mistral_api_key = false,

            "llamaparse" => self.llamaparse = false,
            "document ai" | "document ai (vertex)" => self.document_ai = false,
            "vision ai" => self.vision_ai = false,
            "pdfrest" => self.pdfrest = false,
            _ => {}
        }
    }
    /// Human-readable reason why a specific backend is unavailable.
    /// Returns `None` when the backend IS available.
    pub fn unavailable_reason(&self, backend: &str) -> Option<&'static str> {
        match backend {
            "gemini_api_key" if !self.gemini_api_key => {
                Some("GEMINI_API_KEY not configured. Set it in Settings -> API Keys or .env.")
            }
            "groq_api_key" if !self.groq_api_key => {
                Some("GROQ_API_KEY not configured. Set it in Settings -> API Keys or .env.")
            }
            "gemini_vertex" if !self.gemini_vertex => {
                Some("Vertex AI requires a service account or ADC credentials. Configure in Settings -> API Keys.")
            }
            "document_ai" if !self.document_ai => {
                Some("Document AI requires project ID, processor ID, and auth credentials. Configure in Settings -> API Keys.")
            }

            "llamaparse" if !self.llamaparse => {
                Some("LLAMAPARSE_API_KEY not configured. Set it in Settings -> API Keys or .env.")
            }
            "pdfrest" if !self.pdfrest => {
                Some("PDFREST_API_KEY not configured. Set it in .env to enable cloud rendering.")
            }
            "openrouter_api_key" if !self.openrouter_api_key => {
                Some("OPENROUTER_API_KEY not configured. Set it in Settings -> API Keys or .env.")
            }
            "mistral_api_key" if !self.mistral_api_key => {
                Some("MISTRAL_API_KEY not configured. Set it in Settings -> API Keys or .env.")
            }
            "vision_ai" if !self.vision_ai => {
                Some("VISION_API_KEY not configured. Set it in Settings -> API Keys or .env.")
            }
            "pymupdf_pro" if !self.pymupdf_pro => {
                Some("PYMUPDF_PRO_KEY is missing or malformed. Per-segment editing is unavailable.")
            }
            "ocr" if !self.ocr => {
                if !cfg!(feature = "ocr") {
                    Some("Local OCR requires the 'ocr' Cargo feature. Rebuild with: cargo build --features ocr")
                } else {
                    Some("OCR model files not found. Download text-detection.rten and text-recognition.rten into the models/ directory.")
                }
            }
            _ => None,
        }
    }

    /// Log a summary of detected availability at boot time.
    pub fn log_summary(&self) {
        tracing::info!(
            gemini_api = self.gemini_api_key,
            gemini_vertex = self.gemini_vertex,
            mistral_api = self.mistral_api_key,
            document_ai = self.document_ai,
            llamaparse = self.llamaparse,
            pdfrest = self.pdfrest,
            pymupdf_pro = self.pymupdf_pro,
            vision_ai = self.vision_ai,
            ocr = self.ocr,
            "[boot] API availability detected"
        );
    }
}

/// Locate the Application Default Credentials file written by
/// `gcloud auth application-default login`. Returns `None` if no ADC file
/// can be found at any of the standard locations.
///
/// Priority order:
///  1. `GOOGLE_APPLICATION_CREDENTIALS_ADC` (custom override, opt-in)
///  2. `CLOUDSDK_CONFIG` env var (gcloud-supported override) +
///     `application_default_credentials.json`
///  3. Platform default:
///     - Windows:  `%APPDATA%\gcloud\application_default_credentials.json`
///     - Unix:     `$HOME/.config/gcloud/application_default_credentials.json`
fn detect_adc_path() -> Option<String> {
    if let Ok(p) = env::var("GOOGLE_APPLICATION_CREDENTIALS_ADC") {
        if !p.is_empty() && std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    if let Ok(cfg) = env::var("CLOUDSDK_CONFIG") {
        let candidate = PathBuf::from(cfg).join("application_default_credentials.json");
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    let candidate = if cfg!(windows) {
        env::var("APPDATA").ok().map(|d| {
            PathBuf::from(d)
                .join("gcloud")
                .join("application_default_credentials.json")
        })
    } else {
        env::var("HOME").ok().map(|d| {
            PathBuf::from(d)
                .join(".config")
                .join("gcloud")
                .join("application_default_credentials.json")
        })
    };
    candidate
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_adc_path_returns_string_or_none_without_panicking() {
        // Whatever the platform, this must not crash.
        let _ = detect_adc_path();
    }

    #[test]
    fn typst_reconstruction_is_legacy_only_and_not_fidelity_selectable() -> anyhow::Result<()> {
        let parsed: PdfEngineMode = serde_json::from_str("\"typst_reconstruct\"")?;
        assert_eq!(parsed, PdfEngineMode::TypstReconstruct);
        assert!(!parsed.is_fidelity_selectable());
        assert!(PdfEngineMode::PyMuPdfProPrimary.is_fidelity_selectable());
        assert!(PdfEngineMode::NativeOnly.is_fidelity_selectable());
        Ok(())
    }

    #[test]
    fn local_ocrs_is_legacy_only_and_not_v1_selectable() -> anyhow::Result<()> {
        let parsed: DocumentParserMode = serde_json::from_str("\"local_ocrs\"")?;
        assert_eq!(parsed, DocumentParserMode::LocalOcrs);
        assert!(!parsed.is_v1_selectable());
        assert!(DocumentParserMode::OfflineHeuristic.is_v1_selectable());
        assert!(parsed.label().contains("not available"));
        Ok(())
    }

    #[test]
    fn pro_editing_unavailable_when_key_absent() {
        let mut cfg = AppConfig::default();
        cfg.pymupdf_pro_key = None;

        assert_eq!(cfg.pro_key_status(), ProKeyStatus::Unavailable);
        assert!(!cfg.pro_editing_available());
        assert!(cfg.pro_editing_status_reason().contains("unavailable"));
    }

    #[test]
    fn pro_editing_available_with_well_formed_trial_key() {
        let mut cfg = AppConfig::default();
        cfg.pymupdf_pro_key = Some("hFKt4hca03GCFLAFLEGz5Bd3".to_string());

        assert_eq!(cfg.pro_key_status(), ProKeyStatus::Available);
        assert!(cfg.pro_editing_available());
    }

    #[test]
    fn pro_editing_unavailable_with_malformed_key() {
        let mut cfg = AppConfig::default();
        cfg.pymupdf_pro_key = Some("not-a-valid-key".to_string());

        assert_eq!(cfg.pro_key_status(), ProKeyStatus::Unavailable);
        assert!(!cfg.pro_editing_available());
    }

    #[test]
    fn pro_key_status_does_not_affect_validate_for_split_merge() {
        // A missing Pro key must never cause additional validation beyond the
        // existing dev-mode-gated PYMUPDF_PRO_KEY check; split/merge are not
        // gated by Pro-key state.
        let mut cfg = AppConfig::default();
        cfg.pymupdf_pro_key = None;
        cfg.is_dev_mode = true;

        // In dev mode the missing key is not reported as an error.
        assert!(!cfg.validate().iter().any(|e| e.contains("PYMUPDF_PRO_KEY")));
        // And availability is independent of validate().
        assert!(!cfg.pro_editing_available());
    }

    #[test]
    fn pro_key_status_serializes_to_json() -> anyhow::Result<()> {
        let json = serde_json::to_string(&ProKeyStatus::Available)?;
        assert!(json.contains("available"));
        Ok(())
    }

    #[test]
    fn ai_provider_mode_from_env_str() {
        use super::AiProviderMode;
        assert_eq!(
            AiProviderMode::from_env_str("gemini"),
            AiProviderMode::GeminiApiKey
        );
        assert_eq!(
            AiProviderMode::from_env_str("gemini_api_key"),
            AiProviderMode::GeminiApiKey
        );
        assert_eq!(
            AiProviderMode::from_env_str("vertex_ai"),
            AiProviderMode::GeminiVertex
        );
        assert_eq!(
            AiProviderMode::from_env_str("groq"),
            AiProviderMode::GroqApiKey
        );
        assert_eq!(
            AiProviderMode::from_env_str("openrouter_api_key"),
            AiProviderMode::OpenRouterApiKey
        );
        assert_eq!(
            AiProviderMode::from_env_str("manual_only"),
            AiProviderMode::ManualOnly
        );
        assert_eq!(AiProviderMode::from_env_str(""), AiProviderMode::ManualOnly);
        assert_eq!(
            AiProviderMode::from_env_str("unknown"),
            AiProviderMode::ManualOnly
        );
    }

    #[test]
    fn ai_provider_mode_env_token_round_trip() {
        use super::AiProviderMode;
        let modes = vec![
            AiProviderMode::GroqApiKey,
            AiProviderMode::OpenRouterApiKey,
            AiProviderMode::ManualOnly,
        ];
        for mode in modes {
            assert_eq!(AiProviderMode::from_env_str(mode.env_token()), mode);
        }
    }

    #[test]
    fn test_has_ai_for_balancing() {
        let mut cfg = AppConfig::default();
        cfg.ai_provider = super::AiProviderMode::ManualOnly;
        cfg.gemini_api_key = Some("test".into());
        cfg.groq_api_key = Some("test".into());

        assert!(!cfg.has_ai_for_balancing());

        // Gemini API Key requires gemini_api_key
        cfg.ai_provider = super::AiProviderMode::GeminiApiKey;
        cfg.gemini_api_key = None;
        assert!(!cfg.has_ai_for_balancing());
        cfg.gemini_api_key = Some("test".into());
        assert!(cfg.has_ai_for_balancing());

        // Groq API Key requires groq_api_key
        cfg.ai_provider = super::AiProviderMode::GroqApiKey;
        cfg.groq_api_key = None;
        assert!(!cfg.has_ai_for_balancing());
        cfg.groq_api_key = Some("test".into());
        assert!(cfg.has_ai_for_balancing());

        // OpenRouter API Key requires openrouter_api_key
        cfg.ai_provider = super::AiProviderMode::OpenRouterApiKey;
        cfg.openrouter_api_key = None;
        assert!(!cfg.has_ai_for_balancing());
        cfg.openrouter_api_key = Some("test".into());
        assert!(cfg.has_ai_for_balancing());

        // Gemini Vertex requires Document AI SA or ADC
        cfg.ai_provider = super::AiProviderMode::GeminiVertex;
        cfg.document_ai = None;
        assert!(!cfg.has_ai_for_balancing());
        cfg.document_ai = Some(DocumentAiConfig {
            project_id: "".into(),
            location: "".into(),
            processor_id: "".into(),
            service_account_path: "sa.json".into(),
            adc_path: "".into(),
            api_key: "".into(),
            gcs_output_uri: "".into(),
            passphrase: "".into(),
        });
        assert!(cfg.has_ai_for_balancing());

        // Independent of pdfRest
        cfg.pdfrest_api_key = Some("pdfrest".into());
        assert!(cfg.has_ai_for_balancing());
    }

    #[test]
    fn validate_reports_missing_pro_key_and_passphrase_in_production_mode() {
        let mut cfg = AppConfig::default();
        cfg.pymupdf_pro_key = None;
        cfg.passphrase = "short".into();
        cfg.is_dev_mode = false;

        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("PYMUPDF_PRO_KEY")));
        assert!(errors.iter().any(|e| e.contains("DUAL_CORE_PASSPHRASE")));
    }

    #[test]
    fn detect_availability_exposes_backend_state_and_helpful_reasons() {
        let mut cfg = AppConfig::default();
        cfg.gemini_api_key = Some("gemini".into());
        cfg.groq_api_key = None;
        cfg.openrouter_api_key = None;
        cfg.document_ai = Some(DocumentAiConfig {
            project_id: "proj".into(),
            location: "loc".into(),
            processor_id: "proc".into(),
            service_account_path: "sa.json".into(),
            adc_path: "".into(),
            api_key: "".into(),
            gcs_output_uri: "".into(),
            passphrase: "".into(),
        });
        cfg.llamaparse_api_key = Some("llama".into());
        cfg.pdfrest_api_key = Some("pdfrest".into());
        cfg.pymupdf_pro_key = Some("hFKt4hca03GCFLAFLEGz5Bd3".to_string());

        std::env::set_var("VISION_API_KEY", "test");
        let availability = cfg.detect_availability();
        assert!(availability.gemini_api_key);
        assert!(!availability.groq_api_key);
        assert!(availability.document_ai);
        assert!(availability.llamaparse);
        assert!(availability.pdfrest);
        assert!(availability.pymupdf_pro);
        assert!(availability.vision_ai);

        let reason = availability
            .unavailable_reason("groq_api_key")
            .expect("missing groq backend should produce a reason");
        assert!(reason.contains("GROQ"));
        assert!(availability.unavailable_reason("gemini_api_key").is_none());
    }

    #[test]
    fn unavailable_reason_exposes_openrouter_guidance_when_unconfigured() {
        let availability = ApiAvailability::default();
        let reason = availability
            .unavailable_reason("openrouter_api_key")
            .expect("missing openrouter backend should produce a reason");
        assert!(reason.contains("OPENROUTER"));
        assert!(reason.contains("Settings"));
    }

    #[test]
    fn unavailable_reason_exposes_vision_ai_guidance_when_unconfigured() {
        let availability = ApiAvailability::default();
        let reason = availability
            .unavailable_reason("vision_ai")
            .expect("missing vision backend should produce a reason");
        assert!(reason.contains("VISION_API_KEY"));
    }

    #[test]
    fn config_manager_advances_generation_and_preserves_old_snapshot() {
        let mut initial = AppConfig::default();
        initial.openrouter_model = "generation-0".into();
        let manager = ConfigManager::new(Arc::new(initial));
        let old = manager.snapshot();

        let mut replacement = AppConfig::default();
        replacement.openrouter_model = "generation-1".into();
        let current = manager.replace(replacement);

        assert_eq!(old.generation(), 0);
        assert_eq!(old.config().openrouter_model, "generation-0");
        assert_eq!(current.generation(), 1);
        assert_eq!(current.config().openrouter_model, "generation-1");
        assert_eq!(manager.snapshot().generation(), 1);
    }

    #[test]
    fn config_manager_readers_never_observe_mixed_generations() {
        let mut initial = AppConfig::default();
        initial.openrouter_model = "generation-0".into();
        let manager = ConfigManager::new(Arc::new(initial));
        let reader_manager = manager.clone();

        let reader = std::thread::spawn(move || {
            for _ in 0..10_000 {
                let snapshot = reader_manager.snapshot();
                assert_eq!(
                    snapshot.config().openrouter_model,
                    format!("generation-{}", snapshot.generation())
                );
            }
        });

        for generation in 1..=100 {
            let mut replacement = AppConfig::default();
            replacement.openrouter_model = format!("generation-{generation}");
            let snapshot = manager.replace(replacement);
            assert_eq!(snapshot.generation(), generation);
        }

        reader
            .join()
            .expect("configuration reader should not panic");
    }
}
