//! Bank Statement Fidelity Editor - production GUI
//!
//! Layout (5-region):
//!   [ menu / status / actions / theme toggle ]
//!   [ left: nav + thumbnails ] [ central: canvas ] [ right: tools ]
//!   [ bottom: toasts / progress / status bar ]
//!
//! All long-running work runs through `Job`s on the runtime; the UI only
//! reads `JobResult`s and never blocks.

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::app::modals::AppModals;
use crate::app::runtime::{
    Job, JobId, JobResult, PythonJob, PythonJobResult, RuntimeClient, RuntimeSubmitError,
};
use crate::engine::history::ChangeHistory;
use crate::engine::verification::VerificationReport;
use egui_plot::PlotPoints;

// ---------------------------------------------------------------------------
// Theme palette (Catppuccin-inspired) + helpers
// ---------------------------------------------------------------------------

pub use crate::app::theme::{Palette, Theme};

// ---------------------------------------------------------------------------
// Persistent settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub recent_files: Vec<String>,
    #[serde(default)]
    pub dark_mode: bool, // legacy, kept for back-compat
    #[serde(default)]
    pub theme: Theme,
    pub auto_save: bool,
    pub default_dpi: f32,
    #[serde(default)]
    pub auto_match_dpi: bool,
    #[serde(default = "default_true")]
    pub transfer_consensus_mode: bool,
    pub use_pdfrest: bool,
    #[serde(default = "default_true")]
    pub use_vision_ai: bool,
    pub deep_font_replication: bool,
    #[serde(default)]
    pub show_welcome: bool,
    #[serde(default)]
    pub webhook_url: String,
    #[serde(default)]
    pub llamaparse_api_key: String,
    /// Master toggle for "3 Page Mode" - the DEFAULT operating mode.
    /// When true, opened PDFs are transparently split into <=3-page
    /// segments for Pro editing and re-merged on save. Defaults to TRUE,
    /// and a missing/absent stored value is also treated as true.
    #[serde(default = "default_true")]
    pub three_page_mode: bool,
    #[serde(default)]
    pub advanced_mode: bool,
    #[serde(default)]
    pub remote_engine_url: String,
    /// Backend preference: which AI provider to use for balance/vision.
    #[serde(default)]
    pub ai_provider: crate::app::config::AiProviderMode,
    /// Backend preference: which document parser to use for extraction.
    #[serde(default)]
    pub document_parser: crate::app::config::DocumentParserMode,
    /// Backend preference: which renderer for verification diffs.
    #[serde(default)]
    pub verification_renderer: crate::app::config::VerificationMode,
    /// Visual diff threshold (0.0–1.0). Lower = stricter fidelity gate.
    /// Default 0.02. The visual validation loop uses this as the
    /// tile-max score ceiling; any page-level tile above this value
    /// trips the "only intended changes" gate.
    #[serde(default = "default_visual_threshold")]
    pub visual_diff_threshold: f64,
    /// Maximum visual validation retry attempts before accepting
    /// the result even if the threshold is not met. Default 5.
    #[serde(default = "default_max_visual_attempts")]
    pub max_visual_attempts: u32,
    #[serde(default = "default_true")]
    pub interactive_fallbacks: bool,
}

/// serde default for `three_page_mode`. NOTE: a bare `#[serde(default)]`
/// resolves `bool` to `false`; the default for this feature must be `true`,
/// so we supply an explicit default function that returns `true` when no
/// stored value is present.
fn default_true() -> bool {
    true
}
fn default_visual_threshold() -> f64 {
    0.02
}
fn default_max_visual_attempts() -> u32 {
    5
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            recent_files: Vec::new(),
            dark_mode: true,
            theme: Theme::ForensicDark,
            auto_save: true,
            default_dpi: 300.0,
            auto_match_dpi: false,
            transfer_consensus_mode: true,
            use_pdfrest: false,
            use_vision_ai: true,
            deep_font_replication: false,
            show_welcome: true,
            webhook_url: String::new(),
            llamaparse_api_key: String::new(),
            three_page_mode: true,
            advanced_mode: false,
            remote_engine_url: String::new(),
            ai_provider: crate::app::config::AiProviderMode::default(),
            document_parser: crate::app::config::DocumentParserMode::default(),
            verification_renderer: crate::app::config::VerificationMode::default(),
            visual_diff_threshold: default_visual_threshold(),
            max_visual_attempts: default_max_visual_attempts(),
            interactive_fallbacks: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Toast / notification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Warn,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub expires_at: Instant,
    pub action_label: Option<String>,
    pub action_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Block returned by Python click-detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct TextBlock {
    pub page: usize,
    pub text: String,
    pub bbox: [f32; 4],
    #[serde(default)]
    pub font: String,
    #[serde(default)]
    pub size: f32,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ProgressState {
    pub label: String,
    pub fraction: f32,
    pub started_at: std::time::Instant,
}

#[derive(PartialEq)]
pub enum AppView {
    SingleDocument,
    BatchProcessing,
    AuditExplorer,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ActiveModal {
    #[default]
    None,
    DiscardDraftConfirm,
    WorkflowHitl,
    Settings,
    CommandPalette,
    Transfer,
    Feedback,
    DateAdjust,
    TransferTest,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveWorkflow {
    EditStatement,
    TransferTransactions,
    AgentCommand,
    AuditForensics,
    ChaosSandbox,
    Settings,
    ApiKeys,
}

pub struct MyApp {
    // Files
    pub input_path: String,
    pub output_path: String,
    pub current_pdf_path: PathBuf,
    previous_pdf_path: Option<PathBuf>,
    pub export_path: String,

    // Document state
    pub current_page: usize,
    pub total_pages: usize,
    pub history_state: ChangeHistory,

    // Batch Processing
    batch_folder_path: Option<PathBuf>,
    batch_files: Vec<PathBuf>,

    // View
    current_view: AppView,
    pub active_workflow: ActiveWorkflow,
    pub sidebar_expanded: bool,
    pub zoom_factor: f32,
    pan_offset: egui::Vec2,
    show_curtain: bool,
    curtain_ratio: f32,
    pub fit_to_view: bool,

    // Selection
    selected_block: Option<TextBlock>,
    last_click_pos: Option<egui::Pos2>,
    new_text: String,

    // Natural Language Editing
    pub natural_language_prompt: String,

    // Textures
    pub current_page_texture: Option<egui::TextureHandle>,
    before_texture: Option<egui::TextureHandle>,
    after_texture: Option<egui::TextureHandle>,
    pub transfer_source_texture: Option<egui::TextureHandle>,
    pub transfer_target_texture: Option<egui::TextureHandle>,
    pub current_page_dpi: f32,
    current_page_size_pts: Option<(f32, f32)>,

    // App / job state
    pub status: String,
    pub progress: Option<ProgressState>,
    last_warning: Option<String>,
    pub last_verification: Option<VerificationReport>,
    pub proposed_changes: Vec<(crate::engine::model::ProposedChange, bool)>,
    pub last_imbalance: Option<rust_decimal::Decimal>,
    pub in_flight: usize,
    pub active_workflow_job_id: Option<JobId>,
    pub ai_explanation: Option<String>,
    pub settings: AppSettings,
    toasts: VecDeque<Toast>,

    // Channels
    pub job_tx: RuntimeClient,
    pub job_rx: std::sync::mpsc::Receiver<JobResult>,
    pending_python: Option<tokio::sync::oneshot::Receiver<PythonJobResult>>,
    app_paths: crate::app::paths::AppPaths,
    run_workspace: Option<crate::app::paths::RunWorkspace>,

    // Render coalescing
    last_render_request: Option<(String, usize, u32)>,

    // Multi-stage workflow state
    pub workflow_stage: crate::engine::workflow::WorkflowStage,
    pub workflow_transactions: Vec<crate::engine::model::Transaction>,
    workflow_validation: Option<crate::engine::workflow::ParseValidation>,
    #[allow(dead_code)]
    workflow_df: Option<polars::frame::DataFrame>,
    #[allow(dead_code)]
    pub workflow_edits: Vec<crate::engine::workflow::UserEdit>,
    workflow_preview: Option<crate::engine::workflow::BalancePreview>,
    workflow_visual: Option<crate::engine::workflow::VisualAttempt>,
    workflow_outcome: Option<crate::engine::workflow::WorkflowOutcome>,
    pub last_runtime_activity: std::time::Instant,
    pub stuck_detection: Option<std::time::Instant>,
    #[allow(dead_code)]
    native_engine: Option<std::sync::Arc<dyn crate::pdf::PdfEngine>>,

    /// Stage 8.5: per-font breakdown for the loaded PDF, populated
    /// automatically when `JobResult::FontAnalysisReady` arrives.
    font_analysis: Option<crate::engine::font_analysis::FontAnalysis>,
    /// Stage 13 / Item #12: pending modal confirmations. Each entry is
    /// (title, body, on_confirm action).
    pub active_modal: ActiveModal,
    pub command_query: String,
    pub agent_autonomous_mode: bool,
    pub transfer_source_path: String,
    // Feedback modal state
    pub feedback_text: String,
    pub feedback_include_logs: bool,
    pub feedback_include_audit: bool,
    // Date Adjust dialog state
    pub date_adjust_shift_days: String,
    pub date_adjust_mode_shift: bool, // true = shift, false = remap
    pub date_adjust_from: String,
    pub date_adjust_to: String,
    // AI Confirmation dialog state
    pub pending_ai_confirmations: Vec<crate::engine::ai_confirm::AiConfirmation>,
    // Interactive Fallback state
    pub pending_interactive_fallback:
        Option<crate::engine::interactive_fallback::InteractiveFallbackRequest>,
    // Transfer Test dialog state
    pub transfer_test_paths: Vec<String>,
    pub transfer_test_report: Option<crate::engine::transfer_test_harness::TestHarnessReport>,
    /// Stage 12 / Item #3: history of cascade invocations during the
    /// current workflow attempt. Reset on a new workflow start; appended
    /// to whenever the runtime reports `JobResult::FontCascadeUsed`.
    font_cascade_reports: Vec<crate::engine::font_analysis::FontCascadeReport>,

    // Telemetry
    pub telemetry_cpu: f32,
    pub telemetry_ram_mb: u64,

    /// True when in-memory workflow state has changed since the last
    /// autosave to `audit/workflow.json`. Set whenever
    /// `workflow_validation`, `workflow_transactions` or `workflow_edits`
    /// is mutated; cleared after a successful save. Stage 5 / Item #9.
    workflow_dirty: bool,
    /// Last instant we wrote `audit/workflow.json`. Used to debounce - at
    /// most one save every 1.5s while edits are flying in.
    workflow_last_save: Option<Instant>,
    /// Cached `(input_path, sha256)` for the currently-open PDF so the
    /// autosave doesn't re-hash multi-MB files every 1.5s. Stage 6.
    workflow_input_hash: Option<(String, String)>,
    /// Per-cell text buffers for the inline edit table. Keyed by
    /// (page, line_on_page, field). Stage 5 / Item #6.
    workflow_cell_buffers:
        std::collections::HashMap<(usize, usize, crate::engine::workflow::EditField), String>,

    // Config (read-only)
    pub config: std::sync::Arc<crate::app::config::AppConfig>,

    // --- In-app API key / credentials editor (Settings -> API keys) ---
    /// Editable buffers, seeded from the current environment. Persisted to
    /// `.env` and hot-reloaded into the runtime via `Job::ReloadConfig`.
    pub edit_gemini_api_key: String,
    pub edit_docai_project_id: String,
    pub edit_docai_location: String,
    pub edit_docai_processor_id: String,
    /// Path to a Document AI service-account JSON key (best-practice auth).
    pub edit_docai_service_account: String,
    /// Optional Document AI API key (Beta), takes precedence over OAuth/SA.
    pub edit_docai_api_key: String,
    pub edit_pymupdf_pro_key: String,
    pub edit_llamaparse_api_key: String,
    pub edit_pdfrest_api_key: String,
    pub edit_vision_api_key: String,
    pub edit_groq_api_key: String,
    pub edit_openrouter_api_key: String,
    pub edit_openrouter_model: String,
    pub edit_mistral_api_key: String,
    pub edit_mistral_model: String,
    pub edit_lipi_api_key: String,
    pub edit_mindee_api_key: String,
    /// Gemini auth mode buffer: false = API key (default), true = Vertex AI
    /// (service-account / ADC). Persisted as `GEMINI_AUTH_MODE`.
    pub edit_gemini_use_vertex: bool,
    /// Which PDF engine backend the user wants to force (or Auto)
    pub edit_engine_mode: crate::app::config::PdfEngineMode,
    /// Latest credential/AI status reported by the runtime after a
    /// `Job::ReloadConfig` (document_ai_configured, gemini_configured,
    /// pro_editing_available). `None` until the first reload this session.
    config_status: Option<(bool, bool, bool)>,
    /// Boot-time (and reload-time) API availability snapshot. Drives the
    /// UI auto-exclusion of unavailable backends with explanatory messages.
    pub api_availability: crate::app::config::ApiAvailability,
    pub capability_registry: crate::app::capabilities::CapabilityRegistry,
    /// Result of the last `Job::ValidateCredentials` run. (Gemini, DocAI).
    pub credential_validation_status: Option<(Result<(), String>, Result<(), String>)>,
    /// True once the buffers have been seeded from the environment.
    #[allow(dead_code)]
    api_keys_seeded: bool,
    /// Latest real-time API health polling results.
    pub api_health: Option<Vec<crate::app::api_verification::VerificationResult>>,

    /// Proposed auto-fix for the last encountered error
    pub(crate) pending_autofix: Option<crate::app::error::AppError>,
    /// Selected parser version for Document AI
    pub selected_parser_version: String,

    // -- Document AI Version Manager --
    /// Cached list of available processor versions
    docai_versions: Vec<crate::ai::document_ai::ProcessorVersionInfo>,
    /// True while fetching versions from the API
    docai_versions_loading: bool,
    /// Whether to show the version management panel
    docai_training_status: Option<String>,
    /// Active long-running operation name (training, deploy, etc.)
    docai_active_operation: Option<String>,
    /// Streaming logs from the UFO process (public for E2E/status surfaces).
    pub ufo_logs: Vec<String>,
    /// Whether UFO is actively running (public for E2E and cancel UI state).
    pub is_ufo_running: bool,
    /// User clicked Cancel while UFO was running. Terminal `Error` /
    /// `UfoAutoEditResult` still frees `in_flight` once via
    /// `ends_gui_tracked_job`; this flag only suppresses post-cancel error UX.
    pub ufo_user_cancelled: bool,
}

impl MyApp {
    pub fn new<T: Into<RuntimeClient>>(
        job_tx: T,
        job_rx: std::sync::mpsc::Receiver<JobResult>,
        config: std::sync::Arc<crate::app::config::AppConfig>,
    ) -> Self {
        let job_tx = job_tx.into();
        let settings: AppSettings =
            confy::load("bank-statement-modifier", None).unwrap_or_default();
        let input_path = settings
            .recent_files
            .first()
            .cloned()
            .unwrap_or_else(|| "examples/sample.pdf".to_string());
        let app_paths = crate::app::paths::AppPaths::discover().unwrap_or_else(|error| {
            tracing::error!("[gui] platform application root unavailable: {error}");
            let fallback = crate::app::paths::AppPaths::with_root(
                std::env::temp_dir().join("BankStatementFidelityEditor"),
            );
            let _ = fallback.ensure();
            fallback
        });
        let capability_registry =
            crate::app::capabilities::CapabilityRegistry::probe(&config, &app_paths);
        let run_workspace = app_paths
            .create_run_workspace(std::path::Path::new(&input_path))
            .ok();
        let output_path = run_workspace
            .as_ref()
            .map(|workspace| workspace.output.join("edited.pdf"))
            .unwrap_or_else(|| app_paths.root().join("edited.pdf"));
        let export_path = run_workspace
            .as_ref()
            .map(|workspace| workspace.audit.join("history.json"))
            .unwrap_or_else(|| app_paths.audit_dir().join("history.json"));

        let app = Self {
            input_path: input_path.clone(),
            output_path: output_path.to_string_lossy().to_string(),
            current_pdf_path: PathBuf::new(),
            previous_pdf_path: None,
            export_path: export_path.to_string_lossy().to_string(),
            current_page: 0,
            total_pages: 0,
            history_state: ChangeHistory::new(),
            batch_folder_path: None,
            batch_files: Vec::new(),
            current_view: AppView::SingleDocument,
            active_workflow: ActiveWorkflow::EditStatement,
            sidebar_expanded: true,
            zoom_factor: 1.0,
            pan_offset: egui::Vec2::ZERO,
            show_curtain: false,
            curtain_ratio: 0.5,
            fit_to_view: true,
            selected_block: None,
            last_click_pos: None,
            new_text: String::new(),
            natural_language_prompt: String::new(),
            current_page_texture: None,
            before_texture: None,
            after_texture: None,
            transfer_source_texture: None,
            transfer_target_texture: None,
            current_page_dpi: settings.default_dpi,
            current_page_size_pts: None,
            status: "Ready".to_string(),
            progress: None,
            last_warning: None,
            last_verification: None,
            proposed_changes: Vec::new(),
            last_imbalance: None,
            in_flight: 0,
            active_workflow_job_id: None,
            ai_explanation: None,
            toasts: VecDeque::new(),
            job_tx,
            job_rx,
            ufo_logs: Vec::new(),
            is_ufo_running: false,
            ufo_user_cancelled: false,
            pending_python: None,
            app_paths,
            run_workspace,
            last_render_request: None,
            command_query: String::new(),
            agent_autonomous_mode: false,
            workflow_stage: crate::engine::workflow::WorkflowStage::Idle,
            workflow_transactions: Vec::new(),
            workflow_validation: None,
            workflow_df: None,
            workflow_edits: Vec::new(),
            workflow_preview: None,
            workflow_visual: None,
            workflow_outcome: None,
            last_runtime_activity: std::time::Instant::now(),
            stuck_detection: None,
            native_engine: None,
            font_analysis: None,
            font_cascade_reports: Vec::new(),
            active_modal: ActiveModal::None,
            transfer_source_path: String::new(),
            feedback_text: String::new(),
            feedback_include_logs: true,
            feedback_include_audit: true,
            date_adjust_shift_days: "0".to_string(),
            date_adjust_mode_shift: true,
            date_adjust_from: String::new(),
            date_adjust_to: String::new(),
            pending_ai_confirmations: Vec::new(),
            pending_interactive_fallback: None,
            transfer_test_paths: Vec::new(),
            transfer_test_report: None,
            telemetry_cpu: 0.0,
            telemetry_ram_mb: 0,
            workflow_dirty: false,
            workflow_last_save: None,
            workflow_input_hash: None,
            workflow_cell_buffers: std::collections::HashMap::new(),
            api_availability: config.detect_availability(),
            capability_registry,
            config: config.clone(),
            api_health: None,
            settings,
            // Seed API-key editor buffers from the current environment so the
            // Settings panel shows what's active. Values are masked in the UI.
            edit_gemini_api_key: config.gemini_api_key.clone().unwrap_or_default(),
            edit_docai_project_id: config
                .document_ai
                .as_ref()
                .map(|d| d.project_id.clone())
                .unwrap_or_default(),
            edit_docai_location: config
                .document_ai
                .as_ref()
                .map(|d| d.location.clone())
                .unwrap_or_else(|| "us".to_string()),
            edit_docai_processor_id: config
                .document_ai
                .as_ref()
                .map(|d| d.processor_id.clone())
                .unwrap_or_default(),
            edit_docai_service_account: config
                .document_ai
                .as_ref()
                .map(|d| d.service_account_path.clone())
                .unwrap_or_default(),
            edit_docai_api_key: config
                .document_ai
                .as_ref()
                .map(|d| d.api_key.clone())
                .unwrap_or_default(),
            edit_pymupdf_pro_key: config.pymupdf_pro_key.clone().unwrap_or_default(),
            edit_llamaparse_api_key: config.llamaparse_api_key.clone().unwrap_or_default(),
            edit_pdfrest_api_key: config.pdfrest_api_key.clone().unwrap_or_default(),
            edit_vision_api_key: config.vision_api_key.clone().unwrap_or_default(),
            edit_groq_api_key: config.groq_api_key.clone().unwrap_or_default(),
            edit_openrouter_api_key: config.openrouter_api_key.clone().unwrap_or_default(),
            edit_openrouter_model: config.openrouter_model.clone(),
            edit_mistral_api_key: config.mistral_api_key.clone().unwrap_or_default(),
            edit_mistral_model: config.mistral_model.clone(),
            edit_lipi_api_key: config.lipi_api_key.clone().unwrap_or_default(),
            edit_mindee_api_key: config.mindee_api_key.clone().unwrap_or_default(),
            edit_gemini_use_vertex: matches!(
                config.gemini_auth_mode,
                crate::app::config::GeminiAuthMode::Vertex
            ),
            edit_engine_mode: config.engine_mode,
            config_status: None,
            credential_validation_status: None,
            api_keys_seeded: true,
            pending_autofix: None,
            selected_parser_version: "pretrained-bankstatement-v5.0-2023-12-06".to_string(),
            docai_versions: Vec::new(),
            docai_versions_loading: false,
            docai_training_status: None,
            docai_active_operation: None,
        };
        // Log which API backends were detected at boot for diagnostics.
        app.api_availability.log_summary();

        // Seed USE_VISION_AI environment variable from the loaded AppSettings
        // so that the verification engine (running on the tokio runtime) respects
        // the GUI toggle on startup.
        std::env::set_var(
            "USE_VISION_AI",
            if app.settings.use_vision_ai { "1" } else { "0" },
        );

        // Seed AI_PROVIDER from persisted settings so the runtime AppConfig
        // snapshot picks it up on the initial ReloadConfig below.
        std::env::set_var("AI_PROVIDER", app.settings.ai_provider.env_key());
        std::env::set_var(
            "INTERACTIVE_FALLBACKS",
            if app.settings.interactive_fallbacks {
                "true"
            } else {
                "false"
            },
        );

        // Dispatch a one-time ReloadConfig so the runtime's config_holder
        // picks up the persisted provider + any env vars seeded above.
        // USE_VISION_AI is read live by verification, but ai_provider lives
        // in the config snapshot, so a reload is required.
        if let Err(e) = app.job_tx.send(crate::app::runtime::Job::ReloadConfig) {
            tracing::warn!("[gui] boot ReloadConfig failed (runtime may not be ready): {e}");
        }

        app
    }

    // -- helpers --------------------------------------------------------------

    /// Persist the in-app credential buffers to `.env`, apply them to the
    /// current process environment, and tell the runtime to hot-reload its
    /// `AppConfig` so subsequent Document AI / Gemini / Pro jobs use the new
    /// values without an application restart.
    ///
    /// Keys are upserted into `.env` (existing lines replaced in place,
    /// missing ones appended). Empty buffers remove the override from the live
    /// environment so a cleared field truly disables that credential.
    pub(crate) fn sync_credential_editors_from_config(
        &mut self,
        config: &crate::app::config::AppConfig,
    ) {
        self.edit_gemini_api_key = config.gemini_api_key.clone().unwrap_or_default();
        if let Some(document_ai) = &config.document_ai {
            self.edit_docai_project_id = document_ai.project_id.clone();
            self.edit_docai_location = document_ai.location.clone();
            self.edit_docai_processor_id = document_ai.processor_id.clone();
            self.edit_docai_service_account = document_ai.service_account_path.clone();
            self.edit_docai_api_key = document_ai.api_key.clone();
        } else {
            self.edit_docai_project_id.clear();
            self.edit_docai_location = "us".to_string();
            self.edit_docai_processor_id.clear();
            self.edit_docai_service_account.clear();
            self.edit_docai_api_key.clear();
        }
        self.edit_pymupdf_pro_key = config.pymupdf_pro_key.clone().unwrap_or_default();
        self.edit_gemini_use_vertex = matches!(
            config.gemini_auth_mode,
            crate::app::config::GeminiAuthMode::Vertex
        );
        self.edit_llamaparse_api_key = config.llamaparse_api_key.clone().unwrap_or_default();
        self.edit_pdfrest_api_key = config.pdfrest_api_key.clone().unwrap_or_default();
        self.edit_lipi_api_key = config.lipi_api_key.clone().unwrap_or_default();
        self.edit_vision_api_key = config.vision_api_key.clone().unwrap_or_default();
        self.edit_groq_api_key = config.groq_api_key.clone().unwrap_or_default();
        self.edit_openrouter_api_key = config.openrouter_api_key.clone().unwrap_or_default();
        self.edit_openrouter_model = config.openrouter_model.clone();
        self.edit_mistral_api_key = config.mistral_api_key.clone().unwrap_or_default();
        self.edit_mistral_model = config.mistral_model.clone();
        self.edit_mindee_api_key = config.mindee_api_key.clone().unwrap_or_default();
        self.edit_engine_mode = config.engine_mode;
        self.settings.ai_provider = config.ai_provider;
        self.settings.interactive_fallbacks = config.interactive_fallbacks;
    }

    pub fn save_credentials(&mut self) {
        // (env var name, value) pairs to upsert.
        let pairs: Vec<(&str, String)> = vec![
            (
                "GEMINI_API_KEY",
                self.edit_gemini_api_key.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_PROJECT_ID",
                self.edit_docai_project_id.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_LOCATION",
                self.edit_docai_location.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_PROCESSOR_ID",
                self.edit_docai_processor_id.trim().to_string(),
            ),
            (
                "GOOGLE_APPLICATION_CREDENTIALS",
                self.edit_docai_service_account.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_API_KEY",
                self.edit_docai_api_key.trim().to_string(),
            ),
            (
                "PYMUPDF_PRO_KEY",
                self.edit_pymupdf_pro_key.trim().to_string(),
            ),
            (
                "LLAMAPARSE_API_KEY",
                self.edit_llamaparse_api_key.trim().to_string(),
            ),
            (
                "PDFREST_API_KEY",
                self.edit_pdfrest_api_key.trim().to_string(),
            ),
            (
                "VISION_API_KEY",
                self.edit_vision_api_key.trim().to_string(),
            ),
            ("GROQ_API_KEY", self.edit_groq_api_key.trim().to_string()),
            (
                "OPENROUTER_API_KEY",
                self.edit_openrouter_api_key.trim().to_string(),
            ),
            (
                "MISTRAL_API_KEY",
                self.edit_mistral_api_key.trim().to_string(),
            ),
            (
                "MINDEE_API_KEY",
                self.edit_mindee_api_key.trim().to_string(),
            ),
            (
                "GEMINI_AUTH_MODE",
                if self.edit_gemini_use_vertex {
                    "vertex".to_string()
                } else {
                    "api_key".to_string()
                },
            ),
            (
                "PDF_ENGINE_MODE",
                match self.edit_engine_mode {
                    crate::app::config::PdfEngineMode::DualConcurrent => "dual".to_string(),
                    crate::app::config::PdfEngineMode::PyMuPdfProPrimary => "auto".to_string(),
                    crate::app::config::PdfEngineMode::NativeOnly => "native".to_string(),
                    crate::app::config::PdfEngineMode::PyMuPdfOnly => "pymupdf".to_string(),
                    crate::app::config::PdfEngineMode::TypstReconstruct => "typst".to_string(),
                },
            ),
            (
                "USE_VISION_AI",
                if self.settings.use_vision_ai {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            ),
            (
                "AI_PROVIDER",
                self.settings.ai_provider.env_key().to_string(),
            ),
            (
                "INTERACTIVE_FALLBACKS",
                if self.settings.interactive_fallbacks {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            ),
        ];

        // 1) Apply to the live process environment so from_env() sees them.
        for (k, v) in &pairs {
            if v.is_empty() {
                std::env::remove_var(k);
            } else {
                std::env::set_var(k, v);
            }
        }

        // 2) Upsert into .env so the change survives a restart.
        if let Err(e) = upsert_env_file(std::path::Path::new(".env"), &pairs) {
            tracing::warn!("[gui] failed to write .env: {}", e);
            self.toast(ToastKind::Error, format!("Could not write .env: {e}"));
            // Still attempt the live reload below - the in-memory env is set.
        }

        // 3) Ask the runtime to hot-reload AppConfig from the environment.
        if let Err(e) = self.job_tx.send(Job::ReloadConfig) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
        self.toast(ToastKind::Info, "Saving credentials and reloading...");
    }

    pub fn toast(&mut self, kind: ToastKind, msg: impl Into<String>) {
        self.toasts.push_back(Toast {
            kind,
            text: msg.into(),
            expires_at: Instant::now() + Duration::from_secs(6),
            action_label: None,
            action_id: None,
        });
        while self.toasts.len() > 5 {
            self.toasts.pop_front();
        }
    }

    fn activate_run_workspace(&mut self, document: &std::path::Path) {
        match self.app_paths.create_run_workspace(document) {
            Ok(workspace) => {
                self.output_path = workspace
                    .output
                    .join("edited.pdf")
                    .to_string_lossy()
                    .to_string();
                self.export_path = workspace
                    .audit
                    .join("history.json")
                    .to_string_lossy()
                    .to_string();
                self.run_workspace = Some(workspace);
            }
            Err(error) => {
                tracing::error!("[gui] failed to create document workspace: {error}");
                self.toast(
                    ToastKind::Error,
                    "Could not create an isolated document workspace",
                );
            }
        }
    }

    fn active_workflow_draft_path(&self) -> PathBuf {
        self.run_workspace
            .as_ref()
            .map(|workspace| workspace.drafts.join("workflow.json"))
            .unwrap_or_else(Self::workflow_draft_path)
    }

    pub(crate) fn discard_active_workflow_draft_quiet(&self) {
        let path = self.active_workflow_draft_path();
        if path.exists() {
            if let Err(error) = std::fs::remove_file(&path) {
                tracing::warn!("[gui] removing workflow draft failed: {error}");
            }
        }
    }

    fn dispatch_workflow_job(&mut self, job: Job) -> Result<JobId, RuntimeSubmitError> {
        let job_id = self.job_tx.send(job)?;
        self.active_workflow_job_id = Some(job_id);
        Ok(job_id)
    }

    pub(crate) fn cancel_active_workflow(&mut self) {
        let Some(job_id) = self.active_workflow_job_id else {
            self.toast(ToastKind::Warn, "No active workflow to cancel");
            return;
        };
        match self.job_tx.send(Job::Cancel { id: job_id }) {
            Ok(_) => {
                self.status = format!("Cancelling workflow job #{job_id}...");
                self.toast(ToastKind::Info, format!("Cancelling job #{job_id}"));
            }
            Err(error) => {
                tracing::error!("Runtime disconnected while cancelling job #{job_id}: {error}");
                self.toast(ToastKind::Error, "Could not send cancellation request");
            }
        }
    }

    fn apply_workflow_event(&mut self, event: crate::engine::workflow::WorkflowEvent) -> bool {
        let event_name = event.name();
        match self.workflow_stage.apply_event(event) {
            Ok(()) => {
                self.status = format!("Workflow: {}", self.workflow_stage.label());
                true
            }
            Err(error) => {
                let message = format!("Rejected workflow event '{event_name}': {error}");
                tracing::error!(
                    event = event_name,
                    state = ?self.workflow_stage.kind(),
                    "{message}"
                );
                self.status = message.clone();
                self.toast(ToastKind::Error, message);
                false
            }
        }
    }

    /// Pair edited PDFs with their corresponding originals for batch verification.
    ///
    /// Convention: an edited file is named `<base>_edited.pdf`. Its original is
    /// looked up as `<base>_original.pdf` first, falling back to a bare
    /// `<base>.pdf` in the same set. Returns `(original, edited)` pairs.
    #[allow(dead_code)]
    pub fn pair_originals_and_edited(files: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
        use std::collections::HashMap;
        let by_stem: HashMap<String, PathBuf> = files
            .iter()
            .filter_map(|p| {
                p.file_stem()
                    .map(|s| (s.to_string_lossy().to_string(), p.clone()))
            })
            .collect();

        let mut pairs = Vec::new();
        for (stem, edited) in &by_stem {
            if let Some(base) = stem.strip_suffix("_edited") {
                if let Some(original) = by_stem
                    .get(&format!("{base}_original"))
                    .or_else(|| by_stem.get(base))
                {
                    pairs.push((original.clone(), edited.clone()));
                }
            }
        }
        pairs.sort();
        pairs
    }

    #[allow(dead_code)]
    fn toast_with_action(
        &mut self,
        kind: ToastKind,
        msg: impl Into<String>,
        label: impl Into<String>,
        id: impl Into<String>,
    ) {
        self.toasts.push_back(Toast {
            kind,
            text: msg.into(),
            expires_at: Instant::now() + Duration::from_secs(12),
            action_label: Some(label.into()),
            action_id: Some(id.into()),
        });
        while self.toasts.len() > 5 {
            self.toasts.pop_front();
        }
    }

    pub fn request_render(&mut self, tag: &str) {
        // Only render if the page actually changed since the last request for
        // this tag. This drops bursts when the user clicks rapidly through
        // pages or zooms - preventing render queue blow-up.
        let key = (
            tag.to_string(),
            self.current_page,
            self.current_page_dpi as u32,
        );
        if self.last_render_request.as_ref() == Some(&key) && tag == "current" {
            // already requested with same parameters
            return;
        }
        self.last_render_request = Some(key);

        let path = if tag == "before" {
            self.previous_pdf_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&self.input_path))
        } else {
            self.current_pdf_path.clone()
        };
        if !path.exists() {
            tracing::warn!("[gui] cannot render {:?} (does not exist)", path);
            return;
        }
        if let Err(e) = self.job_tx.send(Job::RenderPage {
            path,
            page: self.current_page,
            dpi: self.current_page_dpi,
            tag: tag.to_string(),
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
    }

    fn update_recent_files(&mut self, path: String) {
        self.settings.recent_files.retain(|f| f != &path);
        self.settings.recent_files.insert(0, path);
        if self.settings.recent_files.len() > 10 {
            self.settings.recent_files.pop();
        }
        if let Err(e) = confy::store("bank-statement-modifier", None, &self.settings) {
            tracing::warn!("[gui] failed to persist settings: {}", e);
        }
    }

    fn load_texture_from_bytes(
        &self,
        ctx: &egui::Context,
        name: &str,
        bytes: &[u8],
    ) -> Option<egui::TextureHandle> {
        let image = match image::load_from_memory(bytes) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("[gui] failed to decode rendered PNG '{}': {}", name, e);
                return None;
            }
        };
        let image = image.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = image.as_flat_samples();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
        Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
    }
    /// G2: Build a real `.tar.gz` artifact bundle containing the input PDF,
    /// edited output PDF, audit log, and change history JSON.
    #[allow(dead_code)]
    pub fn build_artifact_bundle(
        input_path: &str,
        output_path: &std::path::Path,
        bundle_path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::fs::File;

        let gz_file = File::create(bundle_path)?;
        let enc = GzEncoder::new(gz_file, Compression::default());
        let mut ar = tar::Builder::new(enc);

        // Add input PDF if it exists
        let input = std::path::Path::new(input_path);
        if input.exists() {
            ar.append_path_with_name(
                input,
                format!(
                    "bundle/{}",
                    input.file_name().unwrap_or_default().to_string_lossy()
                ),
            )?;
        }

        // Add edited output PDF if it exists
        if output_path.exists() {
            ar.append_path_with_name(
                output_path,
                format!(
                    "bundle/{}",
                    output_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            )?;
        }

        // Add audit log if it exists
        let audit_dir = std::path::Path::new("audit");
        if audit_dir.exists() {
            for entry in std::fs::read_dir(audit_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    ar.append_path_with_name(
                        &path,
                        format!(
                            "bundle/audit/{}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                    )?;
                }
            }
        }

        // Add change history JSON if it exists
        let history_path = std::path::Path::new("audit/change_history.json");
        if history_path.exists() {
            ar.append_path_with_name(history_path, "bundle/change_history.json")?;
        }

        ar.into_inner()?.finish()?;
        Ok(())
    }

    pub fn export_to_excel(&mut self) {
        let result: Result<(), Box<dyn std::error::Error>> = (|| {
            let mut workbook = rust_xlsxwriter::Workbook::new();
            let worksheet = workbook.add_worksheet();
            worksheet.write_string(0, 0, "#")?;
            worksheet.write_string(0, 1, "Page")?;
            worksheet.write_string(0, 2, "Old Text")?;
            worksheet.write_string(0, 3, "New Text")?;
            worksheet.write_string(0, 4, "Reason")?;
            worksheet.write_string(0, 5, "Timestamp")?;
            for (i, rec) in self.history_state.get_history().iter().enumerate() {
                let row = (i + 1) as u32;
                worksheet.write_number(row, 0, (i + 1) as f64)?;
                worksheet.write_number(row, 1, (rec.page + 1) as f64)?;
                worksheet.write_string(row, 2, &rec.old_text)?;
                worksheet.write_string(row, 3, &rec.new_text)?;
                worksheet.write_string(row, 4, &rec.description)?;
                worksheet.write_string(row, 5, &rec.timestamp)?;
            }
            std::fs::create_dir_all("output")?;
            workbook.save("output/export.xlsx")?;
            Ok(())
        })();
        match result {
            Ok(_) => self.toast(ToastKind::Success, "Exported history to output/export.xlsx"),
            Err(e) => self.toast(ToastKind::Error, format!("Excel export failed: {e}")),
        }
    }

    pub fn fit_zoom_to_view(&mut self, available: egui::Vec2, tex_size: egui::Vec2) {
        if tex_size.x <= 0.0 || tex_size.y <= 0.0 {
            return;
        }
        let scale_x = available.x / tex_size.x;
        let scale_y = available.y / tex_size.y;
        self.zoom_factor = scale_x.min(scale_y).clamp(0.1, 5.0) * 0.95;
        self.pan_offset = egui::Vec2::ZERO;
    }

    pub fn balance_trend_points(&self) -> PlotPoints {
        // Real running-balance trend (no fake data).
        let pts: Vec<[f64; 2]> = self
            .history_state
            .get_history()
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.new_text
                    .replace(['$', ','], "")
                    .parse::<f64>()
                    .ok()
                    .map(|v| [i as f64, v])
            })
            .collect();
        if pts.is_empty() {
            PlotPoints::from(vec![[0.0, 0.0]])
        } else {
            PlotPoints::from(pts)
        }
    }
}

// ---------------------------------------------------------------------------
// eframe::App
// ---------------------------------------------------------------------------

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.headless_update(ctx);
    }
}

impl MyApp {
    pub fn headless_update(&mut self, ctx: &egui::Context) {
        // theme
        self.settings.theme.apply(ctx);

        // High-DPI auto-scaling (Stage 6)
        let target_dpi = self.settings.default_dpi * ctx.pixels_per_point();
        if (self.current_page_dpi - target_dpi).abs() > 1.0 {
            self.current_page_dpi = target_dpi;
            self.request_render("current");
        }

        if let Some(p) = &self.progress {
            let fade =
                ctx.animate_value_with_time(egui::Id::new("progress_overlay_fade"), 1.0, 0.3);

            egui::Area::new(egui::Id::new("progress_dialog"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(self.settings.theme.palette().surface.linear_multiply(0.95))
                        .inner_margin(egui::Margin::same(32.0))
                        .rounding(egui::Rounding::same(24.0))
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(0.0, 20.0),
                            blur: 40.0,
                            spread: 0.0,
                            color: egui::Color32::from_black_alpha((100.0 * fade) as u8),
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            self.settings.theme.palette().text.linear_multiply(0.1),
                        ))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add(
                                    egui::Spinner::new()
                                        .size(40.0)
                                        .color(self.settings.theme.palette().accent),
                                );
                                ui.add_space(24.0);
                                ui.label(
                                    egui::RichText::new(&p.label)
                                        .size(18.0)
                                        .strong()
                                        .color(self.settings.theme.palette().text),
                                );

                                ui.add_space(16.0);

                                let pct = (p.fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
                                let mut text = format!("{pct}%");

                                if p.fraction > 0.0 {
                                    let elapsed = p.started_at.elapsed().as_secs_f32();
                                    let eta = (elapsed / p.fraction) * (1.0 - p.fraction);
                                    if eta > 0.0 && eta.is_finite() {
                                        text = format!("{pct}% (ETA: {eta:.0}s)");
                                    }
                                }

                                ui.add(
                                    egui::ProgressBar::new(p.fraction.clamp(0.0, 1.0))
                                        .desired_width(320.0)
                                        .text(egui::RichText::new(text).size(14.0))
                                        .fill(self.settings.theme.palette().accent),
                                );
                            });
                        });
                });
        }

        // Stage 13 / Item #6: workflow shortcuts.
        //   Ctrl+1 -> Parse + AI validate
        //   Ctrl+2 -> Balance Out Preview
        //   Ctrl+3 -> Confirm and Render
        let want_parse =
            ctx.input(|i| i.modifiers.command_only() && i.key_pressed(egui::Key::Num1));
        let want_preview =
            ctx.input(|i| i.modifiers.command_only() && i.key_pressed(egui::Key::Num2));
        let want_confirm =
            ctx.input(|i| i.modifiers.command_only() && i.key_pressed(egui::Key::Num3));
        if want_parse && !self.input_path.is_empty() {
            if let Err(e) = self.dispatch_workflow_job(Job::WorkflowParseAndValidate {
                input: PathBuf::from(&self.input_path),
                version: Some(self.selected_parser_version.clone()),
                parser_mode: self.settings.document_parser,
                ai_provider: self.settings.ai_provider,
                ignore_offline_fallback: false,
            }) {
                tracing::error!("Runtime disconnected: {}", e);
            }
            self.in_flight += 1;
            self.workflow_edits.clear();
            self.workflow_preview = None;
            self.workflow_visual = None;
            self.workflow_outcome = None;
            self.font_cascade_reports.clear();
            self.workflow_dirty = true;
            self.toast(ToastKind::Info, "Parse triggered (Ctrl+1)");
        }
        if want_preview {
            if let Some(v) = &self.workflow_validation {
                if let Err(e) = self.dispatch_workflow_job(Job::WorkflowPreview {
                    original_transactions: self.workflow_transactions.clone(),
                    edits: self.workflow_edits.clone(),
                    opening_balance: v.opening_balance,
                    expected_closing: if v.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                        Some(v.closing_balance)
                    } else {
                        None
                    },
                }) {
                    tracing::error!("Runtime disconnected: {}", e);
                }
                self.in_flight += 1;
                self.toast(ToastKind::Info, "Preview triggered (Ctrl+2)");
            }
        }
        if want_confirm {
            if let Some(preview) = self.workflow_preview.clone() {
                if !preview.balanced {
                    self.toast(
                        ToastKind::Error,
                        format!(
                            "Render blocked: deterministic ledger is out of balance by ${:.2}",
                            preview.final_imbalance.abs()
                        ),
                    );
                } else {
                    let (kept, _) = crate::engine::workflow::prune_redundant_edits(
                        &self.workflow_edits,
                        &preview,
                    );
                    match self.dispatch_workflow_job(Job::WorkflowConfirmAndRender {
                        input: PathBuf::from(&self.input_path),
                        output: PathBuf::from(&self.output_path),
                        edits: kept,
                        original_transactions: self.workflow_transactions.clone(),
                        opening_balance: self
                            .workflow_validation
                            .as_ref()
                            .map(|validation| validation.opening_balance)
                            .unwrap_or_default(),
                        expected_closing: self.workflow_validation.as_ref().and_then(
                            |validation| {
                                if validation.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                                    Some(validation.closing_balance)
                                } else {
                                    None
                                }
                            },
                        ),
                        deep_font_replication: self.settings.deep_font_replication,
                        max_visual_attempts: self.settings.max_visual_attempts,
                        visual_threshold: self.settings.visual_diff_threshold,
                        ignore_font_coverage: false,
                        ignore_visual_fidelity: false,
                    }) {
                        Ok(_) => {
                            self.in_flight += 1;
                            self.toast(ToastKind::Info, "Confirm + Render triggered (Ctrl+3)");
                        }
                        Err(error) => tracing::error!("Runtime disconnected: {}", error),
                    }
                }
            }
        }

        // Stage 13 / Item #15: Ctrl+Shift+Z removes the last queued edit
        // (regular Ctrl+Z is reserved by egui::TextEdit for buffer undo).
        let want_undo_last_edit =
            ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Z));
        if want_undo_last_edit && !self.workflow_edits.is_empty() {
            let removed = self.workflow_edits.pop();
            if let Some(e) = removed {
                // Drop the matching cell-buffer entry so the table shows
                // the original value next frame.
                self.workflow_cell_buffers
                    .remove(&(e.page, e.line_on_page, e.field));
                self.workflow_dirty = true;
                self.toast(
                    ToastKind::Info,
                    format!(
                        "Undid last edit on P{} L{} ({} pending)",
                        e.page + 1,
                        e.line_on_page + 1,
                        self.workflow_edits.len()
                    ),
                );
            }
        }

        // Drag-and-drop support: open the first dropped PDF and tell the
        // user about additional drops. Stage 13 / Item #8.
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let dropped: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            let pdfs: Vec<PathBuf> = dropped
                .into_iter()
                .filter(|p| {
                    p.extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase())
                        == Some("pdf".into())
                })
                .collect();
            if let Some(first) = pdfs.first().cloned() {
                self.open_pdf(first);
                if pdfs.len() > 1 {
                    self.toast(
                        ToastKind::Warn,
                        format!(
                            "Opened the first PDF; ignored {} other(s). The app handles one statement at a time.",
                            pdfs.len() - 1
                        ),
                    );
                }
            }
        }
        // Visual hover-cue while dragging files
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            let p = self.settings.theme.palette();
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("dnd-overlay"),
            ));
            painter.rect_filled(
                screen,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 110),
            );
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "📥 Drop PDF to open",
                egui::FontId::proportional(28.0),
                p.accent,
            );
        }

        // ---- 1. Drain runtime results --------------------------------------
        // Single ownership of `in_flight` decrements for completed jobs:
        // use `ends_gui_tracked_job` (not only strict `is_terminal`) so success
        // payloads like PageRendered / TransactionsExtracted free the wait
        // slot exactly once. Handlers must not double-decrement.
        loop {
            match self.job_rx.try_recv() {
                Ok(res) => {
                    self.last_runtime_activity = std::time::Instant::now();
                    if res.ends_gui_tracked_job() && self.in_flight > 0 {
                        self.in_flight -= 1;
                    }
                    self.handle_job_result(ctx, res);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.status = "❌ Runtime worker disconnected".into();
                    self.in_flight = 0; // Bulletproof fix: reset on disconnect
                    break;
                }
            }
        }

        // ---- 1.1 Watchdog Stuck Detection ----------------------------------
        if self.in_flight > 0 {
            if self.last_runtime_activity.elapsed() > std::time::Duration::from_secs(30) {
                if self.stuck_detection.is_none() {
                    tracing::warn!(
                        "Watchdog: No activity from runtime for 30s. Triggering stuck detection."
                    );
                    self.stuck_detection = Some(std::time::Instant::now());
                }
            } else {
                self.stuck_detection = None;
            }
        } else {
            self.stuck_detection = None;
        }

        // ---- 1.5 Handle drag & drop ----------------------------------------
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                if let Some(file) = i.raw.dropped_files.first() {
                    if let Some(path) = &file.path {
                        if path.is_dir() {
                            self.toast(
                                ToastKind::Warn,
                                "Folder batch UI is not included in v1. Use the `extract-batch` CLI command for bounded batch processing.",
                            );
                        } else if path.is_file()
                            && path
                                .extension()
                                .and_then(|s| s.to_str())
                                .map(|s| s.to_lowercase())
                                == Some("pdf".to_string())
                        {
                            self.current_view = AppView::SingleDocument;
                            self.open_pdf(path.clone());
                        }
                    }
                }
            }
        });

        // Stage 5 / Item #9: autosave the workflow draft if anything has
        // changed since the last save (debounced to 1.5s inside the helper).
        self.autosave_workflow_draft();

        // ---- 2. Check pending Python click reply ---------------------------
        if let Some(rx) = self.pending_python.as_mut() {
            match rx.try_recv() {
                Ok(PythonJobResult::Json(json)) => {
                    if json.trim() == "null" {
                        self.toast(ToastKind::Info, "No text under that click.");
                    } else {
                        match serde_json::from_str::<TextBlock>(&json) {
                            Ok(b) => {
                                self.new_text = b.text.clone();
                                self.selected_block = Some(b);
                            }
                            Err(e) => {
                                self.toast(ToastKind::Warn, format!("Click parse failed: {e}"))
                            }
                        }
                    }
                    self.pending_python = None;
                }
                Ok(other) => {
                    tracing::debug!("[gui] click reply: {:?}", other);
                    self.pending_python = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                Err(_) => self.pending_python = None,
            }
        }

        // ---- 3. Collapsible Sidebar ----------------------------------------
        self.draw_sidebar(ctx);

        // ---- 4. Bottom status bar -----------------------------------------
        self.draw_status_bar(ctx);

        // ---- 5. Main Workspace Routing ------------------------------------
        match self.active_workflow {
            ActiveWorkflow::EditStatement => {
                self.draw_edit_statement_workflow(ctx);
            }
            ActiveWorkflow::TransferTransactions => {
                self.draw_transfer_workflow(ctx);
            }
            ActiveWorkflow::AgentCommand => {
                self.draw_agent_command_workflow(ctx);
            }
            ActiveWorkflow::AuditForensics => {
                self.draw_audit_explorer_view(ctx);
            }
            ActiveWorkflow::ChaosSandbox => {
                self.draw_chaos_sandbox_workflow(ctx);
            }
            ActiveWorkflow::Settings => {
                self.draw_settings_workflow(ctx);
            }
            ActiveWorkflow::ApiKeys => {
                self.draw_api_keys_workflow(ctx);
            }
        }

        // ---- 6. Toasts ----------------------------------------------------
        if let Some(action_id) = self.draw_toasts(ctx) {
            if action_id == "open_audit_explorer" {
                self.current_view = AppView::AuditExplorer;
            }
        }

        // ---- 6b. Modal confirmations -------------------------------------
        self.draw_modals(ctx);

        // ---- 7. Keyboard shortcuts ---------------------------------------
        self.handle_shortcuts(ctx);

        // Repaint while jobs are running so progress updates animate
        if self.in_flight > 0 || !self.toasts.is_empty() || self.pending_python.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
}

// ---------------------------------------------------------------------------
// UI sections
// ---------------------------------------------------------------------------

impl MyApp {
    fn handle_job_result(&mut self, ctx: &egui::Context, res: JobResult) {
        match res {
            JobResult::UfoLog(line) => {
                self.ufo_logs.push(line);
            }
            JobResult::UfoAutoEditResult(val) => {
                self.is_ufo_running = false;
                self.progress = None;
                // in_flight freed once in the drain loop via ends_gui_tracked_job
                if self.ufo_user_cancelled {
                    self.ufo_user_cancelled = false;
                    return;
                }

                let status = val["status"].as_str().unwrap_or("unknown");
                let task_id = val["task_id"].as_str().unwrap_or("unknown");
                if status == "success" {
                    self.toast(
                        ToastKind::Success,
                        format!("UFO Auto-Edit Complete (Task: {task_id})"),
                    );
                } else {
                    let message = val["message"]
                        .as_str()
                        .or_else(|| val["output"].as_str())
                        .unwrap_or("UFO task finished without success");
                    self.toast(
                        ToastKind::Error,
                        format!("UFO Auto-Edit failed (Task: {task_id}): {message}"),
                    );
                }
            }

            JobResult::McpRenderComplete { .. } => {
                self.progress = None;
            }
            JobResult::ImbalanceExplained { explanation } => {
                self.progress = None;
                self.ai_explanation = Some(explanation);
            }
            JobResult::WatchdogEvent(event) => {
                match event {
                    crate::app::watchdog::WatchdogEvent::StallDetected(_timeout) => {
                        self.stuck_detection = Some(std::time::Instant::now());
                    }
                    crate::app::watchdog::WatchdogEvent::FallbackTriggered => {
                        // The modal in modals.rs will handle the actual fallback triggering
                        // because it monitors stuck_detection. We can just leave it or force it.
                        self.stuck_detection =
                            Some(std::time::Instant::now() - std::time::Duration::from_secs(30));
                    }
                    crate::app::watchdog::WatchdogEvent::Recovered => {
                        self.stuck_detection = None;
                    }
                    crate::app::watchdog::WatchdogEvent::Telemetry { cpu_usage, ram_mb } => {
                        self.telemetry_cpu = cpu_usage;
                        self.telemetry_ram_mb = ram_mb;
                    }
                }
            }
            JobResult::ApiKeysVerified(report) => {
                use crate::app::api_verification::VerificationStatus;
                for res in &report.results {
                    if res.status == VerificationStatus::Failed {
                        let msg = res.error_message.as_deref().unwrap_or_default();
                        if msg.contains("429") {
                            self.toast(
                                ToastKind::Error,
                                format!(
                                    "{} quota exceeded (429). Temporarily disabled.",
                                    res.service
                                ),
                            );
                            self.api_availability.disable_service(&res.service);
                        } else if msg.contains("401") || msg.contains("403") {
                            self.toast(
                                ToastKind::Error,
                                format!("{} auth failed. Check your API key.", res.service),
                            );
                            self.api_availability.disable_service(&res.service);
                        }
                    }
                }
                self.api_health = Some(report.results);
            }
            JobResult::BugReportSubmitted => {
                self.toast(
                    ToastKind::Success,
                    "Bug report submitted successfully! Thank you.".to_string(),
                );
            }
            JobResult::DocumentLoaded { total_pages, .. } => {
                self.total_pages = total_pages;
                self.current_page = 0;
                self.current_pdf_path = PathBuf::from(&self.input_path);
                self.previous_pdf_path = None;
                self.update_recent_files(self.input_path.clone());
                self.status = format!("Loaded {total_pages} page(s)");
                self.toast(ToastKind::Success, format!("Loaded {total_pages} pages"));
                self.request_render("current");
                // Keep the original open_pdf in_flight wait open across the
                // auto-chained parse (DocumentLoaded is non-ending). Do not +1
                // again here or the counter permanently drifts.
                self.workflow_edits.clear();
                self.workflow_preview = None;
                self.workflow_visual = None;
                self.workflow_outcome = None;
                self.font_cascade_reports.clear();
                self.workflow_dirty = true;
                if let Err(e) = self.dispatch_workflow_job(Job::WorkflowParseAndValidate {
                    input: PathBuf::from(&self.input_path),
                    version: Some(self.selected_parser_version.clone()),
                    parser_mode: self.settings.document_parser,
                    ai_provider: self.settings.ai_provider,
                    ignore_offline_fallback: false,
                }) {
                    tracing::error!("Runtime disconnected: {}", e);
                    // Parse never started: free the open_pdf wait slot.
                    self.in_flight = self.in_flight.saturating_sub(1);
                }
            }
            JobResult::HistoryUpdated { history } => {
                self.history_state = history;
                let idx = self.history_state.current_index();
                self.previous_pdf_path = Some(self.current_pdf_path.clone());
                self.current_pdf_path = if idx > 0 {
                    self.history_state.get_history()[idx - 1]
                        .snapshot_path
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| PathBuf::from(&self.input_path))
                } else {
                    PathBuf::from(&self.input_path)
                };
                self.status = "History synchronized".into();
                self.request_render("current");
            }
            JobResult::PageRendered {
                png_bytes,
                tag,
                width_pts,
                height_pts,
                ..
            } => {
                let texture = self.load_texture_from_bytes(ctx, &tag, &png_bytes);
                match tag.as_str() {
                    "current" => {
                        self.current_page_texture = texture;
                        self.current_page_size_pts = Some((width_pts, height_pts));
                    }
                    "before" => self.before_texture = texture,
                    "after" => self.after_texture = texture,
                    "transfer_source" => self.transfer_source_texture = texture,
                    "transfer_target" => self.transfer_target_texture = texture,
                    _ => {}
                }
            }
            JobResult::ChangeApplied {
                record,
                requires_visual_review,
            } => {
                self.toast(
                    if requires_visual_review {
                        ToastKind::Warn
                    } else {
                        ToastKind::Success
                    },
                    format!("Edit applied: {} -> {}", record.old_text, record.new_text),
                );
                if requires_visual_review {
                    self.last_warning = Some("Review required: complex background.".into());
                }
                self.status = "Change applied".into();
                self.request_render("current");
                self.request_render("before");
                self.request_render("after");
            }
            JobResult::BalanceProposed { imbalance, changes } => {
                self.last_imbalance = Some(imbalance);
                self.proposed_changes = changes.into_iter().map(|c| (c, true)).collect();
                if self.proposed_changes.is_empty() {
                    self.status = "Statement is already perfectly balanced.".into();
                    self.toast(ToastKind::Success, "Statement is already balanced.");
                } else {
                    self.status = format!(
                        "Proposed {} adjustments for ${:.2} imbalance",
                        self.proposed_changes.len(),
                        imbalance
                    );
                    self.toast(
                        ToastKind::Info,
                        format!("{} adjustments proposed", self.proposed_changes.len()),
                    );
                }
            }
            JobResult::ProposedChangesApplied {
                changes_applied,
                failures,
            } => {
                if failures.is_empty() {
                    self.toast(
                        ToastKind::Success,
                        format!("Applied {changes_applied} changes"),
                    );
                } else {
                    self.toast(
                        ToastKind::Warn,
                        format!("Applied {changes_applied} ({} failures)", failures.len()),
                    );
                }
                // Statement may have changed on disk; refresh the views.
                self.request_render("current");
                self.request_render("before");
                self.request_render("after");
            }
            JobResult::ConfigReloaded {
                generation,
                config,
                document_ai_configured,
                gemini_configured,
                pro_editing_available,
            } => {
                self.config_status = Some((
                    document_ai_configured,
                    gemini_configured,
                    pro_editing_available,
                ));
                let mut parts = Vec::new();
                parts.push(format!(
                    "Document AI {}",
                    if document_ai_configured { "✓" } else { "✗" }
                ));
                parts.push(format!(
                    "Gemini {}",
                    if gemini_configured { "✓" } else { "✗" }
                ));
                parts.push(format!(
                    "Pro editing {}",
                    if pro_editing_available { "✓" } else { "✗" }
                ));
                let summary = parts.join(" Â· ");
                self.status = format!("Configuration generation {generation} applied: {summary}");
                // Refresh every GUI consumer from the exact immutable runtime
                // generation instead of independently re-reading process state.
                self.sync_credential_editors_from_config(&config);
                let fresh_avail = config.detect_availability();
                fresh_avail.log_summary();
                self.api_availability = fresh_avail;
                self.capability_registry =
                    crate::app::capabilities::CapabilityRegistry::probe(&config, &self.app_paths);
                self.config = config;
                self.toast(
                    if document_ai_configured && gemini_configured {
                        ToastKind::Success
                    } else {
                        ToastKind::Warn
                    },
                    format!("Credentials reloaded - {summary}"),
                );
            }
            JobResult::TransactionsExtracted(txs) => {
                self.toast(
                    ToastKind::Success,
                    format!("Extracted {} transactions", txs.len()),
                );
                self.workflow_transactions = txs;
                self.workflow_dirty = true;
                self.last_runtime_activity = std::time::Instant::now();
            }
            JobResult::NaturalLanguageEditReady(txs) => {
                self.toast(
                    ToastKind::Success,
                    format!("Applied AI edit to {} transactions", txs.len()),
                );
                self.workflow_transactions = txs;
                self.workflow_dirty = true;
                self.last_runtime_activity = std::time::Instant::now();
            }
            JobResult::CategorizationReady(txs) => {
                self.toast(
                    ToastKind::Success,
                    format!("Categorized {} transactions", txs.len()),
                );
                self.workflow_transactions = txs;
                self.workflow_dirty = true;
                self.last_runtime_activity = std::time::Instant::now();
            }
            JobResult::FontCompleted(_) => {
                self.toast(ToastKind::Success, "Font completion finished");
            }
            JobResult::ChangeHistoryExported { path } => {
                self.toast(
                    ToastKind::Success,
                    format!("History exported: {}", path.display()),
                );
            }
            JobResult::ReconstructComplete { output_path } => {
                self.toast(
                    ToastKind::Success,
                    format!(
                        "Typst Reconstruction Complete! Output saved to: {:?}",
                        output_path
                    ),
                );
            }
            JobResult::VerificationReport(report) => {
                self.last_verification = Some(report.clone());
                let passed = report.mandatory_local_pass();
                let failed_gates = report
                    .gates
                    .iter()
                    .filter(|gate| {
                        gate.mandatory
                            && gate.status
                                != crate::engine::verification::VerificationGateStatus::Passed
                    })
                    .map(|gate| gate.id.as_str())
                    .collect::<Vec<_>>();
                let summary = if passed {
                    "Verification PASS: every mandatory local gate passed".to_string()
                } else {
                    format!("Verification FAIL: {}", failed_gates.join(", "))
                };
                self.status = summary.clone();
                self.toast(
                    if passed {
                        ToastKind::Success
                    } else {
                        ToastKind::Error
                    },
                    summary,
                );
            }
            JobResult::Progress { label, fraction } => {
                if fraction >= 1.0 {
                    self.progress = None;
                } else {
                    let started_at = match &self.progress {
                        Some(p) if p.label == label => p.started_at,
                        _ => std::time::Instant::now(),
                    };
                    self.progress = Some(ProgressState {
                        label,
                        fraction,
                        started_at,
                    });
                }
            }
            JobResult::Error { job_label, message } => {
                if job_label == "ufo_dispatch" {
                    self.is_ufo_running = false;
                    // User cancel already toasted; process kill surfaces as Error.
                    // Free in_flight only via the drain ends_gui_tracked_job path.
                    if self.ufo_user_cancelled {
                        self.ufo_user_cancelled = false;
                        self.progress = None;
                        return;
                    }
                }
                // in_flight decremented once in the drain loop via ends_gui_tracked_job
                self.progress = None;
                if matches!(
                    job_label.as_str(),
                    "workflow_parse_and_validate"
                        | "workflow_preview"
                        | "workflow_confirm_and_render"
                        | "ai_fix_visual_fidelity"
                ) {
                    self.active_workflow_job_id = None;
                }

                if message.starts_with("__DISPATCH:") {
                    let parts: Vec<&str> = message.split(':').collect();
                    if parts.len() >= 2 {
                        match parts[1] {
                            "Undo" => {
                                let _ = self.job_tx.send(crate::app::runtime::Job::Undo);
                            }
                            "Redo" => {
                                let _ = self.job_tx.send(crate::app::runtime::Job::Redo);
                            }
                            "Balance" => {
                                let _ =
                                    self.job_tx
                                        .send(crate::app::runtime::Job::BalanceStatement {
                                            path: std::path::PathBuf::from(&self.input_path),
                                        });
                            }
                            "Verify" => {
                                let _ = self
                                    .job_tx
                                    .send(crate::app::runtime::Job::ValidateCredentials);
                            }
                            "Extract" => {
                                let _ = self.job_tx.send(
                                    crate::app::runtime::Job::ExtractTransactions {
                                        path: std::path::PathBuf::from(&self.input_path),
                                        parser_mode:
                                            crate::app::config::DocumentParserMode::LlamaParse,
                                    },
                                );
                            }
                            "Transfer" => {
                                // Transfer via GUI requires selecting source/target PDFs first
                            }
                            "AdjustDates" => {
                                let _ = self.job_tx.send(
                                    crate::app::runtime::Job::NaturalLanguageEdit {
                                        prompt: format!(
                                            "Shift dates by {}",
                                            parts.get(2).unwrap_or(&"0")
                                        ),
                                        transactions: self.workflow_transactions.clone(),
                                    },
                                );
                            }
                            "Categorize" => {
                                let _ = self.job_tx.send(
                                    crate::app::runtime::Job::CategorizeTransactions {
                                        transactions: self.workflow_transactions.clone(),
                                    },
                                );
                            }
                            "Doctor" => {
                                let _ = self
                                    .job_tx
                                    .send(crate::app::runtime::Job::ValidateCredentials);
                            }
                            "ReloadConfig" => {
                                let _ = self.job_tx.send(crate::app::runtime::Job::ReloadConfig);
                            }
                            "StressTest" => {
                                let _ =
                                    self.job_tx
                                        .send(crate::app::runtime::Job::RunTransferTests {
                                            statements: vec![],
                                            max_iterations: 1,
                                        });
                            }
                            _ => {}
                        }
                    }
                    return;
                }

                // Autofix interception for ALL errors
                let err = crate::app::error::AppError::parse_msg(&message)
                    .unwrap_or_else(|| crate::app::error::AppError::Unknown(message.clone()));

                match &err {
                    crate::app::error::AppError::ApiFailure(m) => {
                        let suggestion = err.suggested_action().unwrap_or("");
                        self.toast(
                            ToastKind::Error,
                            format!("API Error: {}\n{}", m, suggestion),
                        );
                    }
                    _ => {
                        self.pending_autofix = Some(err);
                    }
                }

                tracing::error!("[gui] runtime error in '{}': {}", job_label, message);

                // Write comprehensive error sink
                let dir = std::path::PathBuf::from("audit/error_reports");
                let _ = std::fs::create_dir_all(&dir);
                let filename = format!("report_{}.json", chrono::Utc::now().format("%Y%m%d%H%M%S"));
                let report = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "kind": "JobError",
                    "job_label": job_label,
                    "message": message,
                    "input_path": self.input_path,
                });
                let _ = std::fs::write(
                    dir.join(filename),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
            }
            JobResult::Pong => {
                self.toast(ToastKind::Info, "pong");
            }
            JobResult::FontAnalysisReady(analysis) => {
                let line = analysis.one_line_summary();
                let kind = if analysis.summary.all_fonts_covered {
                    ToastKind::Success
                } else {
                    ToastKind::Warn
                };
                self.toast(kind, line);
                self.font_analysis = Some(analysis);
            }
            JobResult::FontCascadeUsed(report) => {
                let summary = report.one_line_summary();
                let kind = if report.success {
                    ToastKind::Success
                } else {
                    ToastKind::Warn
                };
                self.toast(kind, summary);
                self.font_cascade_reports.push(report);
            }
            JobResult::Cancelled { id } => {
                self.progress = None;
                if self.active_workflow_job_id == Some(id) {
                    self.active_workflow_job_id = None;
                }
                self.toast(ToastKind::Info, format!("Cancelled job #{id}"));
                self.status = format!("Cancelled job #{id}");
            }
            JobResult::TimedOut { id, job_label } => {
                self.progress = None;
                if self.active_workflow_job_id == Some(id) {
                    self.active_workflow_job_id = None;
                }
                self.toast(
                    ToastKind::Error,
                    format!("{job_label} timed out (job #{id})"),
                );
                self.status = format!("Timed out: {job_label}");
            }

            // ---- Multi-stage workflow ----------------------------------
            JobResult::WorkflowStageChanged { stage } => {
                let opens_modal = matches!(
                    &stage,
                    crate::engine::workflow::WorkflowStage::VisualFidelityWarning { .. }
                        | crate::engine::workflow::WorkflowStage::ImbalanceCorrectionWarning { .. }
                        | crate::engine::workflow::WorkflowStage::FontCoverageWarning { .. }
                        | crate::engine::workflow::WorkflowStage::OfflineFallbackWarning
                );
                let event = crate::engine::workflow::WorkflowEvent::from_stage(stage);
                if self.apply_workflow_event(event) && opens_modal {
                    self.active_modal = ActiveModal::WorkflowHitl;
                }
            }
            JobResult::VisualAlternativesReady(images) => {
                let event = crate::engine::workflow::WorkflowEvent::ShowVisualComparison { images };
                if self.apply_workflow_event(event) {
                    self.active_modal = ActiveModal::WorkflowHitl;
                }
            }
            JobResult::WorkflowParseValidated {
                validation,
                transactions,
            } => {
                let count = validation.transactions_found;
                let score = validation.completeness_score;
                self.workflow_validation = Some(validation);
                self.workflow_transactions = transactions;
                // Stage 13 / Item #4: stale cell-buffer entries from a
                // prior parse can still appear in the inline edit table
                // because they are keyed by (page, line_on_page, field).
                // Re-parsing may produce new line_on_page indices for the
                // same transactions; clear the buffers so the table
                // re-initialises from the fresh values.
                self.workflow_cell_buffers.clear();
                self.workflow_edits.clear();
                self.workflow_dirty = true;
                self.toast(
                    if score >= 0.85 {
                        ToastKind::Success
                    } else {
                        ToastKind::Warn
                    },
                    format!(
                        "Parsed {count} transactions • completeness {:.0}%",
                        score * 100.0
                    ),
                );
            }
            JobResult::WorkflowPreviewBuilt(preview) => {
                let kind = if preview.balanced {
                    ToastKind::Success
                } else {
                    ToastKind::Warn
                };
                self.toast(
                    kind,
                    format!(
                        "Preview ready • {} rows will change • imbalance ${:.2}",
                        preview.rows.iter().filter(|r| r.will_change).count(),
                        preview.final_imbalance
                    ),
                );
                self.workflow_preview = Some(preview);
            }
            JobResult::WorkflowVisualAttempt(attempt) => {
                self.toast(
                    if attempt.passed() {
                        ToastKind::Success
                    } else {
                        ToastKind::Info
                    },
                    format!(
                        "Visual attempt {}/{} • diff {:.4}",
                        attempt.attempt, attempt.max_attempts, attempt.diff_score
                    ),
                );
                self.workflow_visual = Some(attempt);
            }
            JobResult::WorkflowComplete(outcome) => {
                self.progress = None;
                self.active_workflow_job_id = None;
                self.toast(ToastKind::Success, outcome.completion_summary.clone());
                self.workflow_outcome = Some(outcome);
                // Stage 6: workflow finished cleanly - clear the in-flight
                // edit queue and remove the autosaved draft so the next
                // session starts fresh. Resume-draft now correctly reports
                // "no draft to resume" until new edits accumulate.
                self.workflow_edits.clear();
                self.workflow_cell_buffers.clear();
                self.workflow_dirty = false;
                self.discard_active_workflow_draft_quiet();
            }
            JobResult::WorkflowFailed(failure) => {
                self.progress = None;
                self.active_workflow_job_id = None;
                let msg = match &failure {
                    crate::engine::workflow::WorkflowFailure::ParseFailed(s) => {
                        format!("Parse failed: {s}")
                    }
                    crate::engine::workflow::WorkflowFailure::Incomplete { score, .. } => {
                        format!("Parse rejected as incomplete (score {score:.2})")
                    }
                    crate::engine::workflow::WorkflowFailure::FontCoverageFailed {
                        missing_chars,
                    } => {
                        format!("Font coverage missing chars: {missing_chars:?}")
                    }
                    crate::engine::workflow::WorkflowFailure::VisualNotConverged {
                        last_score,
                        attempts,
                    } => {
                        format!("Visual didn't converge after {attempts} tries; last diff {last_score:.4}")
                    }
                    crate::engine::workflow::WorkflowFailure::FinalMathInvalid { imbalance } => {
                        format!("Final math invalid: imbalance ${imbalance:.2}")
                    }
                    crate::engine::workflow::WorkflowFailure::FidelityCheckFailed(s) => {
                        format!("AI Fidelity Check Failed: {s}")
                    }
                    crate::engine::workflow::WorkflowFailure::Other(s) => s.clone(),
                };

                // Autofix interception
                if let Some(err) = crate::app::error::AppError::parse_msg(&msg) {
                    if err.suggested_action().is_some() {
                        self.pending_autofix = Some(err);
                    }
                }

                self.toast(ToastKind::Error, &msg);
                self.apply_workflow_event(crate::engine::workflow::WorkflowEvent::Fail(failure));

                let dir = std::path::PathBuf::from("audit/error_reports");
                let _ = std::fs::create_dir_all(&dir);
                let filename = format!("report_{}.json", chrono::Utc::now().format("%Y%m%d%H%M%S"));
                let report = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "kind": "WorkflowFailed",
                    "message": msg,
                    "input_path": self.input_path,
                });
                let _ = std::fs::write(
                    dir.join(filename),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
            }
            JobResult::JobCompleted {
                job_label,
                disposition,
                artifact,
                message,
            } => {
                use crate::app::runtime::OperationDisposition;
                self.progress = None;
                let (kind, prefix) = match disposition {
                    OperationDisposition::Succeeded => (ToastKind::Success, "Completed"),
                    OperationDisposition::NoOp => (ToastKind::Info, "No changes"),
                    OperationDisposition::Partial => (ToastKind::Warn, "Partially completed"),
                    OperationDisposition::Failed => (ToastKind::Error, "Failed"),
                    OperationDisposition::Cancelled => (ToastKind::Warn, "Cancelled"),
                    OperationDisposition::TimedOut => (ToastKind::Error, "Timed out"),
                };
                let artifact_note = artifact
                    .as_ref()
                    .map(|path| format!(" -> {}", path.display()))
                    .unwrap_or_default();
                let status = format!("{prefix} [{job_label}]: {message}{artifact_note}");
                self.status = status.clone();
                self.toast(kind, status);
            }
            JobResult::TransferComplete(result) => {
                self.progress = None;
                let msg = format!(
                    "✓ Transfer complete: {} txns -> output, math: {}, visual: {} (AI Layout: {}), ({:.1}s)",
                    result.source_tx_count,
                    if result.math_verified { "✓" } else { "✗" },
                    if result.visual_verified { "✓" } else { "✗" },
                    if result.visual_proof_path.is_some() { "APPROVED" } else { "N/A" },
                    result.total_duration_secs,
                );
                self.status = msg.clone();
                self.toast(ToastKind::Success, &msg);

                // Auto-load the output PDF
                let output_path = result.output_path.clone();
                if output_path.exists() {
                    self.open_pdf(output_path);

                    // Auto-load the source PDF as a side-by-side (Curtain Diff) layer
                    if !self.transfer_source_path.is_empty()
                        && self
                            .job_tx
                            .send(Job::RenderPage {
                                path: std::path::PathBuf::from(self.transfer_source_path.clone()),
                                page: 0,
                                dpi: self.settings.default_dpi,
                                tag: "after".to_string(), // Reuse 'after' texture slot for side-by-side
                            })
                            .is_ok()
                    {
                        self.in_flight += 1;
                        self.show_curtain = true;
                        self.curtain_ratio = 0.5; // Split 50/50 down the middle
                        self.toast(ToastKind::Info, "Loading side-by-side comparison...");
                    }
                }
            }
            JobResult::TransferFailed { stage, message } => {
                self.progress = None;
                let msg = format!("Transfer failed at {stage}: {message}");
                self.status = msg.clone();
                self.toast(ToastKind::Error, &msg);

                // Write error report
                let dir = std::path::PathBuf::from("audit/error_reports");
                let _ = std::fs::create_dir_all(&dir);
                let filename = format!(
                    "transfer_{}.json",
                    chrono::Utc::now().format("%Y%m%d%H%M%S")
                );
                let report = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "kind": "TransferFailed",
                    "stage": stage,
                    "message": message,
                    "input_path": self.input_path,
                });
                let _ = std::fs::write(
                    dir.join(filename),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
            }
            JobResult::DatesAdjusted {
                records,
                output_path,
            } => {
                self.progress = None;
                let msg = format!(
                    "📅 Adjusted {} dates -> {}",
                    records.len(),
                    output_path.display()
                );
                self.status = msg.clone();
                self.toast(ToastKind::Success, &msg);
                // Auto-load the output
                if output_path.exists() {
                    self.open_pdf(output_path);
                }
            }
            JobResult::AiConfirmationNeeded(confirmation) => {
                self.pending_ai_confirmations.push(confirmation);
            }
            JobResult::InteractiveFallbackRequired(req) => {
                self.pending_interactive_fallback = Some(req);
            }
            JobResult::TransferTestsComplete(report) => {
                self.progress = None;
                let msg = report.summary();
                self.status = msg.clone();
                if report.all_passed() {
                    self.toast(ToastKind::Success, &msg);
                } else {
                    self.toast(ToastKind::Error, &msg);
                }
                self.transfer_test_report = Some(report);
            }

            // -- Document AI Version Management --
            JobResult::DocAiVersionsListed(versions) => {
                self.docai_versions = versions;
                self.docai_versions_loading = false;
                self.toast(
                    ToastKind::Info,
                    format!("Found {} processor versions", self.docai_versions.len()),
                );
            }
            JobResult::DocAiVersionOperationStarted {
                operation_name,
                description,
            } => {
                self.docai_training_status = Some(description.clone());
                self.docai_active_operation = Some(operation_name);
                self.toast(ToastKind::Info, &description);
            }
            JobResult::DocAiVersionError(msg) => {
                self.docai_versions_loading = false;
                self.docai_training_status = Some(format!("❌ {msg}"));
                self.toast(ToastKind::Error, &msg);
            }
            JobResult::NuclearFallbackRequired(msg) => {
                self.status = format!(
                    "Fidelity workflow stopped without replacing the document: {}",
                    msg
                );
                let toast_msg = self.status.clone();
                self.toast(ToastKind::Error, toast_msg);
            }
        }
    }
    fn draw_sidebar(&mut self, ctx: &egui::Context) {
        // Animate the sidebar width for a buttery smooth expansion
        let target_width = if self.sidebar_expanded { 240.0 } else { 70.0 };
        let width =
            ctx.animate_value_with_time(egui::Id::new("sidebar_width_anim"), target_width, 0.3);

        let frame = egui::Frame {
            inner_margin: egui::Margin::same(12.0),
            rounding: egui::Rounding {
                nw: 0.0,
                sw: 0.0,
                ne: 24.0,
                se: 24.0,
            },
            fill: ctx.style().visuals.window_fill.linear_multiply(0.95), // Slight translucency
            stroke: egui::Stroke::new(1.0, ctx.style().visuals.widgets.inactive.bg_fill),
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 8.0),
                blur: 16.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(80),
            },
            ..Default::default()
        };

        egui::SidePanel::left("sidebar")
            .frame(frame)
            .exact_width(width)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(16.0);

                // Toggle Button (Hamburger)
                ui.horizontal(|ui| {
                    let toggle_text = if self.sidebar_expanded {
                        "≡  Collapse"
                    } else {
                        "≡"
                    };
                    let btn =
                        egui::Button::new(egui::RichText::new(toggle_text).size(18.0).strong())
                            .frame(false)
                            .min_size(egui::vec2(ui.available_width(), 40.0));

                    if ui.add(btn).clicked() {
                        self.sidebar_expanded = !self.sidebar_expanded;
                    }
                });

                ui.add_space(32.0);

                let mut selected = self.active_workflow.clone();
                let workflows = [
                    (ActiveWorkflow::EditStatement, "📄", "Editor"),
                    (ActiveWorkflow::TransferTransactions, "⇄", "Transfer"),
                    (ActiveWorkflow::AgentCommand, "⌘", "Commands"),
                    (ActiveWorkflow::Settings, "⚙", "Settings"),
                    (ActiveWorkflow::ApiKeys, "🔑", "API Keys"),
                ];

                for (workflow, icon, text) in workflows {
                    let is_selected = self.active_workflow == workflow;

                    // Custom pill-shaped active state
                    let bg_color = if is_selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let text_color = if is_selected {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().text_color()
                    };

                    let btn_text = if width > 120.0 {
                        format!("{}  {}", icon, text)
                    } else {
                        icon.to_string()
                    };

                    let response = ui.allocate_rect(
                        egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), 48.0),
                        ),
                        egui::Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &btn_text)
                    });

                    // Hover animation
                    let hover_factor =
                        ctx.animate_bool(response.id.with("hover"), response.hovered());
                    let final_bg = if is_selected {
                        bg_color
                    } else {
                        ui.visuals()
                            .widgets
                            .hovered
                            .bg_fill
                            .linear_multiply(hover_factor)
                    };

                    // Draw the custom button
                    ui.painter().rect(
                        response.rect,
                        egui::Rounding::same(12.0),
                        final_bg,
                        egui::Stroke::NONE,
                    );

                    // Draw the text
                    let text_pos = response.rect.min
                        + egui::vec2(
                            if width > 120.0 {
                                16.0
                            } else {
                                (width - 24.0) / 2.0
                            },
                            14.0,
                        );
                    ui.painter().text(
                        text_pos,
                        egui::Align2::LEFT_TOP,
                        btn_text,
                        egui::FontId::new(16.0, egui::FontFamily::Proportional),
                        text_color,
                    );

                    if response.clicked() {
                        selected = workflow;
                    }

                    ui.advance_cursor_after_rect(response.rect);
                    ui.add_space(8.0);
                }

                self.active_workflow = selected;

                // Bottom anchored branding
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(16.0);
                    let opacity = ctx.animate_value_with_time(
                        egui::Id::new("sidebar_brand_anim"),
                        if self.sidebar_expanded { 1.0 } else { 0.0 },
                        0.2,
                    );
                    if opacity > 0.1 {
                        ui.label(
                            egui::RichText::new("Antigravity\nStatement Forensics")
                                .size(12.0)
                                .color(ui.visuals().text_color().linear_multiply(0.4 * opacity)),
                        );
                    }
                });
            });
    }

    fn draw_edit_statement_workflow(&mut self, ctx: &egui::Context) {
        let frame = egui::Frame {
            inner_margin: egui::Margin::same(16.0),
            fill: ctx.style().visuals.panel_fill,
            stroke: egui::Stroke::NONE,
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 8.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(40),
            },
            ..Default::default()
        };

        // Streaming Logs Drawer (if UFO is running or logs exist)
        if !self.ufo_logs.is_empty() {
            egui::TopBottomPanel::top("ufo_logs_panel")
                .resizable(true)
                .min_height(100.0)
                .max_height(300.0)
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::from_gray(30))
                        .inner_margin(8.0),
                )
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new("UFO Agent Logs")
                            .strong()
                            .color(egui::Color32::LIGHT_GREEN),
                    );
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let text = self.ufo_logs.join("\n");
                            ui.add(
                                egui::TextEdit::multiline(&mut text.as_str())
                                    .font(egui::TextStyle::Monospace)
                                    .text_color(egui::Color32::from_gray(200))
                                    .desired_width(f32::INFINITY)
                                    .interactive(false),
                            );
                        });
                });
        }

        // 1. Top Bar: Upload Dropzone & History Thumbnails
        egui::TopBottomPanel::top("edit_top_bar")
            .frame(frame)
            .exact_height(90.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Modern Upload Button
                    let upload_btn = ui.add_sized(
                        [160.0, 58.0],
                        egui::Button::new(
                            egui::RichText::new("📥  Upload Statement")
                                .size(15.0)
                                .strong(),
                        )
                        .fill(ui.visuals().selection.bg_fill),
                    );

                    if upload_btn.clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("PDF", &["pdf"])
                            .pick_file()
                        {
                            self.open_pdf(path);
                        }
                    }

                    ui.add_space(10.0);

                    // 🪄 📄 Auto-Edit (UFO) Button
                    let auto_edit_btn = ui.add_sized(
                        [180.0, 58.0],
                        egui::Button::new(
                            egui::RichText::new("🪄 📄 Auto-Edit (UFO)")
                                .size(15.0)
                                .strong()
                                .color(self.settings.theme.palette().bg),
                        )
                        .fill(self.settings.theme.palette().accent), // Use accent color for high visibility
                    );

                    if auto_edit_btn.clicked() {
                        if self.current_pdf_path.exists() {
                            let mut context = "BankFidelity State Context:\n".to_string();
                            context.push_str(&format!(
                                "Total Transactions loaded: {}\n",
                                self.workflow_transactions.len()
                            ));
                            if let Some(err) = &self.last_imbalance {
                                context.push_str(&format!(
                                    "Current Error: Statement is out of balance. Difference: {:.2}\n",
                                    err
                                ));
                            } else if let Some(verification) = &self.last_verification {
                                context.push_str(&format!(
                                    "Verification State: {verification:#?}\n"
                                ));
                            }

                            self.ufo_logs.clear();
                            match self.dispatch_workflow_job(Job::UfoAutoEdit {
                                path: self.current_pdf_path.clone(),
                                context,
                            }) {
                                Ok(_) => {
                                    self.is_ufo_running = true;
                                    self.in_flight += 1;
                                }
                                Err(e) => {
                                    self.is_ufo_running = false;
                                    tracing::error!("Failed to dispatch Auto-Edit: {e}");
                                    self.toast(
                                        ToastKind::Error,
                                        format!("Failed to start UFO Auto-Edit: {e}"),
                                    );
                                }
                            }
                        } else {
                            self.toast(ToastKind::Warn, "Please open a statement first.");
                        }
                    }

                    if self.is_ufo_running {
                        ui.add_space(10.0);
                        let cancel_btn = ui.add_sized(
                            [120.0, 58.0],
                            egui::Button::new(
                                egui::RichText::new("🛑 Cancel")
                                    .size(15.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(220, 50, 50)),
                        );

                        if cancel_btn.clicked() {
                            let _ = self.dispatch_workflow_job(Job::CancelUfo);
                            // Clear local busy flags immediately. Do NOT touch
                            // in_flight here: CancelUfo kills the process and
                            // the UfoAutoEdit spawn always emits Error or
                            // UfoAutoEditResult, which frees exactly one wait
                            // slot via ends_gui_tracked_job. Early decrement
                            // would double-count when that terminal result arrives.
                            self.is_ufo_running = false;
                            self.ufo_user_cancelled = true;
                            self.progress = None;
                            self.toast(ToastKind::Warn, "UFO Task Cancelled.");
                        }
                    }

                    ui.add_space(20.0);
                    let sep = egui::Separator::default().vertical().spacing(30.0);
                    ui.add(sep);

                    if self.current_pdf_path.exists() {
                        // Visual Progress Stepper (Stage 9)
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Workflow Progress")
                                    .color(ui.visuals().text_color().linear_multiply(0.6))
                                    .size(12.0),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                let steps =
                                    ["1. Load", "2. Edit & Diff", "3. Verify Math", "4. Export"];
                                // Determine current step based on state
                                let current_step = if self.last_verification.is_some() {
                                    3 // Validated, ready to export
                                } else if !self.workflow_transactions.is_empty() {
                                    2 // Edited/Balanced, pending validation
                                } else {
                                    1 // Loaded, pending edits
                                };

                                for (i, step) in steps.iter().enumerate() {
                                    let is_active = i <= current_step;
                                    let is_current = i == current_step;

                                    let color = if is_current {
                                        self.settings.theme.palette().accent
                                    } else if is_active {
                                        self.settings.theme.palette().success
                                    } else {
                                        self.settings.theme.palette().weak.linear_multiply(0.3)
                                    };

                                    let text = egui::RichText::new(*step).color(color).strong();
                                    ui.label(text);

                                    if i < steps.len() - 1 {
                                        ui.add_space(8.0);
                                        let line_color = if is_active {
                                            self.settings
                                                .theme
                                                .palette()
                                                .success
                                                .linear_multiply(0.5)
                                        } else {
                                            self.settings.theme.palette().weak.linear_multiply(0.1)
                                        };
                                        let (rect, _resp) = ui.allocate_exact_size(
                                            egui::vec2(40.0, 2.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().hline(
                                            rect.min.x..=rect.max.x,
                                            rect.center().y,
                                            egui::Stroke::new(2.0, line_color),
                                        );
                                        ui.add_space(8.0);
                                    }
                                }
                            });
                        });
                    } else {
                        // History Thumbnail Strip
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Recent Statements")
                                    .color(ui.visuals().text_color().linear_multiply(0.6))
                                    .size(12.0),
                            );
                            ui.add_space(4.0);
                            egui::ScrollArea::horizontal()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let recent = self.settings.recent_files.clone();
                                        if recent.is_empty() {
                                            ui.label(
                                                egui::RichText::new("No recent files")
                                                    .italics()
                                                    .color(
                                                        ui.visuals()
                                                            .text_color()
                                                            .linear_multiply(0.3),
                                                    ),
                                            );
                                        }
                                        for f in recent.into_iter().take(5) {
                                            let label = std::path::Path::new(&f)
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy();

                                            if ui
                                                .add_sized([120.0, 36.0], egui::Button::new(label))
                                                .clicked()
                                            {
                                                self.open_pdf(std::path::PathBuf::from(f));
                                            }
                                        }
                                    });
                                });
                        });
                    }

                    // Add Report Bug button to the far right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🐛 Submit Diagnostics").clicked() {
                            self.active_modal = ActiveModal::Feedback;
                        }
                    });
                });
            });

        // 2. Right Toolbox: The 5 specific e2e editing actions
        let right_frame = egui::Frame {
            inner_margin: egui::Margin::same(16.0),
            fill: ctx.style().visuals.window_fill.linear_multiply(0.98),
            stroke: egui::Stroke::new(1.0, ctx.style().visuals.widgets.inactive.bg_fill),
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 12.0),
                blur: 24.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(100),
            },
            ..Default::default()
        };

        egui::SidePanel::right("edit_toolbox")
            .frame(right_frame)
            .exact_width(340.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.heading(egui::RichText::new("Statement Forensics & Editing").strong());
                ui.add_space(24.0);

                if let Some(block) = self.selected_block.clone() {
                    egui::Frame::group(ui.style())
                        .fill(ui.visuals().faint_bg_color)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                egui::RichText::new("Properties")
                                    .color(ui.visuals().text_color().linear_multiply(0.6))
                                    .size(12.0),
                            );
                            ui.add_space(4.0);
                            ui.label(format!("Font: {}", block.font));
                            ui.label(format!("Size: {:.1} pt", block.size));
                        });

                    ui.add_space(16.0);

                    ui.label(
                        egui::RichText::new("Edit Content")
                            .color(ui.visuals().text_color().linear_multiply(0.6))
                            .size(12.0),
                    );
                    ui.add_space(4.0);

                    let text_edit = egui::TextEdit::multiline(&mut self.new_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(ui.available_width())
                        .margin(egui::vec2(12.0, 12.0));
                    ui.add(text_edit);

                    ui.add_space(20.0);

                    let btn_size = egui::vec2(ui.available_width(), 44.0);

                    if ui
                        .add_sized(
                            btn_size,
                            egui::Button::new(egui::RichText::new("Apply single edit").strong()),
                        )
                        .clicked()
                    {
                        let original = block.text.clone();
                        let new_text = self.new_text.clone();
                        if let Err(e) = self.job_tx.send(crate::app::runtime::Job::ApplyChange {
                            input: self.current_pdf_path.clone(),
                            output: std::path::PathBuf::from(&self.output_path),
                            page: self.current_page,
                            bbox: block.bbox,
                            old_text: original,
                            new_text,
                            description: "Manual Edit".to_string(),
                            deep_font_replication: self.settings.deep_font_replication,
                        }) {
                            tracing::error!("Runtime disconnected: {}", e);
                        }
                        self.in_flight += 1;
                        self.new_text.clear();
                        self.selected_block = None;
                    }
                    ui.add_space(5.0);
                    ui.add_space(5.0);
                    ui.separator();
                    ui.heading("Live Output Preview");
                    ui.add_space(10.0);
                    if let Some(tex) = &self.after_texture {
                        let max_size = ui.available_size() - egui::vec2(0.0, 20.0);
                        let tex_size = tex.size_vec2();
                        // Prevent scale from going unbounded, but fit it to the panel width
                        let scale = (ui.available_width() / tex_size.x)
                            .min(max_size.y / tex_size.y)
                            .min(1.0);
                        ui.add(egui::Image::new(tex).fit_to_exact_size(tex_size * scale));
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.weak("Preview will appear after an edit is applied.");
                        });
                    }
                    ui.add_space(5.0);
                    if ui
                        .add_sized(btn_size, egui::Button::new("Preview edits required"))
                        .clicked()
                    {
                        self.toast(ToastKind::Info, "Generating required edits proposal...");
                        let _ = self
                            .job_tx
                            .send(crate::app::runtime::Job::BalanceStatement {
                                path: std::path::PathBuf::from(&self.input_path),
                            });
                        self.in_flight += 1;
                    }
                    ui.add_space(5.0);
                    if ui
                        .add_sized(btn_size, egui::Button::new("Verify preview independently"))
                        .clicked()
                    {
                        self.toast(ToastKind::Info, "Running independent local verification...");
                        let intended_edits: Vec<crate::engine::verification::VerificationIntent> =
                            self.history_state
                                .get_history()
                                .iter()
                                .map(|record| crate::engine::verification::VerificationIntent {
                                    page: record.page,
                                    bbox: record.bbox,
                                    old_text: record.old_text.clone(),
                                    new_text: record.new_text.clone(),
                                })
                                .collect();
                        let _ = self.job_tx.send(crate::app::runtime::Job::Verify {
                            original: std::path::PathBuf::from(&self.input_path),
                            edited: std::path::PathBuf::from(&self.output_path),
                            output_dir: std::path::PathBuf::from("audit"),
                            intended_edits,
                            use_pdfrest: self.settings.verification_renderer
                                == crate::app::config::VerificationMode::PdfRestCloud,
                            pdfrest_key: self.config.pdfrest_api_key.clone(),
                            auto_match_dpi: self.settings.auto_match_dpi,
                        });
                        self.in_flight += 1;
                    }
                    ui.add_space(5.0);
                    if ui
                        .add_sized(
                            btn_size,
                            egui::Button::new("Perform * edits and perform complete balance out")
                                .fill(egui::Color32::from_rgb(0, 100, 0)),
                        )
                        .clicked()
                    {
                        self.toast(ToastKind::Info, "Executing full auto-balance editing...");
                        let _ = self
                            .job_tx
                            .send(crate::app::runtime::Job::BalanceStatement {
                                path: std::path::PathBuf::from(&self.input_path),
                            });
                        self.in_flight += 1;
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.weak("Click any text on the canvas to begin editing.");
                    });
                }
            });

        // 3. Central Panel: Context-aware zooming PDF canvas
        // This reuses the existing robust central panel rendering logic
        self.draw_central_panel(ctx);
    }

    fn draw_transfer_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("⇄ Cross-Ledger Migration Engine");
            ui.add_space(20.0);

            ui.columns(2, |columns| {
                // Source Dropzone
                columns[0].group(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Source Statement");
                        ui.label("Transactions will be extracted from this document.");
                        ui.add_space(10.0);
                        if ui
                            .add_sized([200.0, 80.0], egui::Button::new("📥 Upload Source\n(PDF)"))
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("PDF", &["pdf"])
                                .pick_file()
                            {
                                self.transfer_source_path = path.to_string_lossy().to_string();
                            }
                        }
                        if !self.transfer_source_path.is_empty() {
                            ui.add_space(5.0);
                            ui.colored_label(
                                egui::Color32::LIGHT_GREEN,
                                format!(
                                    "Selected: {}",
                                    std::path::Path::new(&self.transfer_source_path)
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                ),
                            );
                        }
                    });
                });

                // Target Dropzone
                columns[1].group(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Destination Ledger");
                        ui.label("Transactions will be injected into this document.");
                        ui.add_space(10.0);
                        if ui
                            .add_sized([200.0, 80.0], egui::Button::new("📥 Upload Target\n(PDF)"))
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("PDF", &["pdf"])
                                .pick_file()
                            {
                                self.input_path = path.to_string_lossy().to_string();
                            }
                        }
                        if !self.input_path.is_empty() && self.input_path != "examples/sample.pdf" {
                            ui.add_space(5.0);
                            ui.colored_label(
                                egui::Color32::LIGHT_GREEN,
                                format!(
                                    "Selected: {}",
                                    std::path::Path::new(&self.input_path)
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                ),
                            );
                        }
                    });
                });
            });

            ui.add_space(30.0);

            // Execute Transfer Action
            ui.vertical_centered(|ui| {
                let can_transfer = !self.transfer_source_path.is_empty()
                    && !self.input_path.is_empty()
                    && self.input_path != "examples/sample.pdf";

                let btn_id = ui.id().with("exec_transfer_btn");
                let hovered = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(btn_id))
                    .unwrap_or(false);
                let anim = ui.ctx().animate_bool_with_time(btn_id, hovered, 0.15);
                let expand_w = anim * 15.0;
                let expand_h = anim * 6.0;

                let bg_color = if can_transfer {
                    egui::Color32::from_rgb(0, 120 + (anim * 40.0) as u8, 0)
                } else {
                    egui::Color32::DARK_GRAY
                };

                let btn = egui::Button::new(
                    egui::RichText::new("⚡ Execute Complete Transfer").size(16.0 + (anim * 1.5)),
                )
                .min_size(egui::vec2(400.0 + expand_w, 60.0 + expand_h))
                .fill(bg_color);

                let response = ui.add_enabled(can_transfer, btn);
                if response.hovered() != hovered {
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(btn_id, response.hovered()));
                    ui.ctx().request_repaint();
                }

                if response.clicked() {
                    self.toast(
                        ToastKind::Info,
                        "Initiating Cross-Document Transaction Transfer...",
                    );
                    let _ = self
                        .job_tx
                        .send(crate::app::runtime::Job::ExtractTransactions {
                            path: std::path::PathBuf::from(&self.transfer_source_path),
                            parser_mode: self.settings.document_parser,
                        });
                    self.in_flight += 1;
                }

                if !can_transfer {
                    ui.add_space(5.0);
                    ui.weak("Please upload both a Source and Target statement to begin.");
                }
            });

            ui.add_space(30.0);
            ui.separator();
            ui.add_space(10.0);

            // Shared History Thumbnail Row
            ui.label("Recent Statements (Click to assign to Target):");
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let recent = self.settings.recent_files.clone();
                        for f in recent.into_iter().take(8) {
                            let label = std::path::Path::new(&f)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            if ui
                                .add_sized([120.0, 80.0], egui::Button::new(label))
                                .clicked()
                            {
                                self.input_path = f.clone();
                            }
                        }
                    });
                });
        });
    }

    fn draw_agent_command_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Command Center");
            ui.separator();
            ui.add_space(10.0);

            // Command input
            ui.horizontal(|ui| {
                ui.label("Instruction:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.command_query)
                        .desired_width(ui.available_width() - 80.0)
                        .hint_text("e.g. 'Change all transaction dates to 2026'"),
                );

                if (ui.button("Execute").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.command_query.is_empty() {
                    let prompt = std::mem::take(&mut self.command_query);
                    self.toast(ToastKind::Info, format!("Executing AI command: {}", prompt));
                    self.in_flight += 1;
                    let _ = self.job_tx.send(Job::NaturalLanguageEdit {
                        prompt,
                        transactions: self.workflow_transactions.clone(),
                    });
                }
            });

            ui.add_space(20.0);

            ui.group(|ui| {
                ui.heading("Manual commands only");
                ui.label("Background scraping and autonomous model training are not included in v1. Commands run only when you submit them here.");
            });
        });
    }

    fn draw_chaos_sandbox_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Controlled test runner unavailable");
            ui.label("The internal chaos suite is not exposed in the v1 application. No test has been started.");
        });
    }
    fn draw_settings_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("⚙️ System Configuration & Integrations");
            ui.separator();
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Theme:");
                        egui::ComboBox::from_id_salt("theme_selector")
                            .selected_text(format!("{:?}", self.settings.theme))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    Theme::ForensicDark,
                                    "Forensic Terminal (Dark)",
                                );
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    Theme::ForensicLight,
                                    "Laboratory (Light)",
                                );
                            });
                    });

                    ui.add_space(20.0);
                    self.draw_font_analysis_section(ui);
                    ui.add_space(20.0);
                    self.draw_workflow_section(ui);
                });
        });
    }

    fn draw_api_keys_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🔑 API Keys & Integration Management");
            ui.separator();
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.draw_api_keys_editor(ui);
                });
        });
    }

    fn draw_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            // Global progress: a labeled bar with percentage shown whenever a
            // job is running (explicit Progress updates) or any job is in
            // flight (spinner fallback for jobs that don't stream progress).
            if let Some(p) = self.progress.clone() {
                let pct = (p.fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
                let elapsed = p.started_at.elapsed();
                let eta_str = if p.fraction > 0.01 && p.fraction < 1.0 {
                    let total_est = elapsed.as_secs_f64() / (p.fraction as f64);
                    let remaining = total_est - elapsed.as_secs_f64();
                    if remaining > 60.0 * 90.0 {
                        String::from(" (long running)")
                    } else if remaining > 60.0 {
                        format!(" (ETA: {:.0}m {:.0}s)", remaining / 60.0, remaining % 60.0)
                    } else {
                        format!(" (ETA: {:.0}s)", remaining)
                    }
                } else {
                    String::new()
                };
                ui.add(
                    egui::ProgressBar::new(p.fraction.clamp(0.0, 1.0))
                        .desired_width(ui.available_width())
                        .text(format!("{} - {}%{}", p.label, pct, eta_str)),
                );
                ui.add_space(2.0);
            } else if self.in_flight > 0 {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.small(format!(
                        "Working... ({} task{} in progress)",
                        self.in_flight,
                        if self.in_flight == 1 { "" } else { "s" }
                    ));
                });
                ui.add_space(2.0);
            }
            ui.horizontal(|ui| {
                if self.settings.remote_engine_url.is_empty() {
                    ui.colored_label(egui::Color32::LIGHT_GREEN, "🟢 Local");
                } else {
                    ui.colored_label(
                        egui::Color32::LIGHT_BLUE,
                        format!("🔵 Remote ({})", self.settings.remote_engine_url),
                    );
                }
                ui.separator();
                ui.small(&self.status);
                ui.separator();
                if self.total_pages > 0 {
                    ui.small(format!(
                        "Page {}/{}",
                        self.current_page + 1,
                        self.total_pages
                    ));
                    ui.separator();
                }
                ui.small(format!("DPI: {:.0}", self.current_page_dpi));
                ui.separator();
                ui.small(format!("Zoom: {:.0}%", self.zoom_factor * 100.0));
                // Telemetry Footer (Aligned Right)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(w) = &self.last_warning {
                        ui.colored_label(egui::Color32::YELLOW, format!("⚠ {w}"));
                        ui.separator();
                    }

                    ui.small(format!("RAM: {} MB", self.telemetry_ram_mb));
                    ui.separator();

                    let cpu_color = if self.telemetry_cpu > 80.0 {
                        egui::Color32::from_rgb(255, 80, 80)
                    } else if self.telemetry_cpu > 40.0 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::LIGHT_GREEN
                    };
                    ui.colored_label(cpu_color, format!("CPU: {:.1}%", self.telemetry_cpu));
                    ui.separator();

                    let engine_state = if self.stuck_detection.is_some() {
                        "STALLED"
                    } else if self.in_flight > 0 {
                        "BUSY"
                    } else {
                        "IDLE"
                    };
                    let engine_color = match engine_state {
                        "STALLED" => egui::Color32::from_rgb(255, 80, 80),
                        "BUSY" => egui::Color32::YELLOW,
                        _ => egui::Color32::LIGHT_GREEN,
                    };
                    ui.colored_label(engine_color, format!("ENG: {}", engine_state));
                    ui.separator();

                    let mut all_healthy = true;
                    if let Some(health) = &self.api_health {
                        for res in health {
                            if res.status
                                == crate::app::api_verification::VerificationStatus::Failed
                            {
                                all_healthy = false;
                            }
                        }
                    }
                    if self.api_health.is_some() {
                        if all_healthy {
                            ui.colored_label(egui::Color32::LIGHT_GREEN, "API: HEALTHY");
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(255, 80, 80), "API: DEGRADED");
                        }
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "API: UNKNOWN");
                    }
                });
            });
        });
    }

    #[allow(dead_code)]
    fn draw_left_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("left_panel")
            .width_range(180.0..=300.0)
            .show(ctx, |ui| {
                ui.heading("Navigation");
                ui.horizontal(|ui| {
                    if ui.button("◀").clicked() && self.current_page > 0 {
                        self.current_page -= 1;
                        self.request_render("current");
                    }
                    ui.label(format!(
                        "{} / {}",
                        self.current_page + 1,
                        self.total_pages.max(1)
                    ));
                    if ui.button("▶").clicked() && self.current_page + 1 < self.total_pages {
                        self.current_page += 1;
                        self.request_render("current");
                    }
                });

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for i in 0..self.total_pages {
                            let selected = i == self.current_page;
                            if ui
                                .selectable_label(selected, format!("Page {}", i + 1))
                                .clicked()
                            {
                                self.current_page = i;
                                self.request_render("current");
                            }
                        }
                    });

                ui.separator();
                ui.heading("Precision Content Editor");
                if let Some(block) = self.selected_block.clone() {
                    ui.small(format!(
                        "Font: {}",
                        if block.font.is_empty() {
                            "(unknown)"
                        } else {
                            &block.font
                        }
                    ));
                    ui.small(format!("Size: {:.1}", block.size));
                    ui.add_enabled(
                        false,
                        egui::TextEdit::multiline(&mut block.text.clone()).desired_rows(2),
                    );
                    ui.text_edit_multiline(&mut self.new_text);
                    if self.settings.advanced_mode {
                        ui.checkbox(
                            &mut self.settings.deep_font_replication,
                            "Deep Font Replication (AI)",
                        );
                    }
                } else {
                    ui.weak("Click any text on the canvas to edit.");
                }
            });
    }

    /// Generate a safe output path that never overwrites the input.
    pub fn safe_output_path(input: &std::path::Path, suffix: &str) -> std::path::PathBuf {
        let stem = input.file_stem().unwrap_or_default().to_string_lossy();
        let ext = input.extension().unwrap_or_default().to_string_lossy();
        let parent = input.parent().unwrap_or(std::path::Path::new("."));
        let mut candidate = parent.join(format!("{stem}_{suffix}.{ext}"));
        let mut counter = 1u32;
        while candidate.exists() {
            candidate = parent.join(format!("{stem}_{suffix}_{counter}.{ext}"));
            counter += 1;
        }
        candidate
    }

    /// Settings -> API keys & credentials editor.
    ///
    /// Lets the user view/update the Gemini key, Document AI processor
    /// coordinates, the service-account JSON path (best-practice auth), an
    /// optional Document AI API key, and the PyMuPDF Pro key - then persist
    /// them to `.env`, push them into the process environment, and hot-reload
    /// the runtime config (`Job::ReloadConfig`) so they take effect with no
    /// restart.
    /// Stage 5 / Item #6 + #8: inline editable table of parsed transactions.
    /// Each numeric cell becomes a `TextEdit`; on change we upsert the
    /// matching `UserEdit` in `self.workflow_edits`. The "↶" button on each
    /// row reverts every queued edit on that row at once.
    ///
    /// Validation: an unparseable amount (anything `parse_money` can't read)
    /// gets a red border and is *not* committed to the queue, but the buffer
    /// keeps the user's keystrokes so they can fix it.
    fn draw_workflow_edit_table(&mut self, ui: &mut egui::Ui) {
        use crate::engine::workflow::{EditField, UserEdit};
        if self.workflow_transactions.is_empty() {
            return;
        }
        let palette = self.settings.theme.palette();

        ui.horizontal(|ui| {
            ui.label(format!(
                "📋 Inline edit ({} rows) - Tab to next field, ↶ reverts row",
                self.workflow_transactions.len()
            ));
            ui.add_space(8.0);
            if ui.button("🏷 Auto-Categorize").clicked() {
                if let Err(e) = self
                    .job_tx
                    .send(crate::app::runtime::Job::CategorizeTransactions {
                        transactions: self.workflow_transactions.clone(),
                    })
                {
                    tracing::error!("Runtime disconnected: {}", e);
                }
            }
        });

        // Snapshot what we need; the closure below mutates self.workflow_edits
        // and self.workflow_cell_buffers, so collect transaction copies first.
        let txs: Vec<crate::engine::model::Transaction> = self.workflow_transactions.clone();

        let mut cell_changes: Vec<(usize, usize, EditField, String, [f32; 4], String)> = Vec::new();
        let mut row_reverts: Vec<(usize, usize)> = Vec::new();

        egui::ScrollArea::both().auto_shrink([false, false])
            .max_height(220.0)
            .id_salt("workflow-edit-table")
            .show(ui, |ui| {
                use egui_extras::{Column, TableBuilder};
                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(28.0)) // P
                    .column(Column::auto().at_least(28.0)) // L
                    .column(Column::initial(78.0))         // Date
                    .column(Column::initial(150.0).at_least(100.0)) // Desc
                    .column(Column::initial(80.0))         // Category
                    .column(Column::initial(82.0))         // Debit
                    .column(Column::initial(82.0))         // Credit
                    .column(Column::initial(94.0))         // Balance
                    .column(Column::auto().at_least(28.0)) // Revert
                    .header(20.0, |mut header| {
                        for label in ["P", "#", "Date", "Description", "Category", "Debit", "Credit", "Balance", ""].iter() {
                            header.col(|ui| { ui.strong(*label); });
                        }
                    })
                    .body(|mut body| {
                        for tx in txs.iter() {
                            let key = (tx.page, tx.line_on_page);
                            let has_edit = self
                                .workflow_edits
                                .iter()
                                .any(|e| e.page == key.0 && e.line_on_page == key.1);

                            body.row(20.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(format!("{}", tx.page + 1));
                                });
                                row.col(|ui| {
                                    ui.label(format!("{}", tx.line_on_page + 1));
                                });

                                // Date - text field
                                row.col(|ui| {
                                    let buf = Self::cell_buffer(
                                        &mut self.workflow_cell_buffers,
                                        &self.workflow_edits,
                                        tx,
                                        EditField::Date,
                                        || tx.date.clone(),
                                    );
                                    if ui
                                        .add(egui::TextEdit::singleline(buf).desired_width(76.0))
                                        .changed()
                                    {
                                        cell_changes.push((
                                            tx.page,
                                            tx.line_on_page,
                                            EditField::Date,
                                            buf.clone(),
                                            Self::bbox_for_field(tx, EditField::Date),
                                            tx.date.clone(),
                                        ));
                                    }
                                });

                                // Description - text field
                                row.col(|ui| {
                                    let buf = Self::cell_buffer(
                                        &mut self.workflow_cell_buffers,
                                        &self.workflow_edits,
                                        tx,
                                        EditField::Description,
                                        || tx.raw_text.clone(),
                                    );
                                    if ui
                                        .add(egui::TextEdit::singleline(buf).desired_width(148.0))
                                        .changed()
                                    {
                                        cell_changes.push((
                                            tx.page,
                                            tx.line_on_page,
                                            EditField::Description,
                                            buf.clone(),
                                            Self::bbox_for_field(tx, EditField::Description),
                                            tx.raw_text.clone(),
                                        ));
                                    }
                                });

                                // Category - label
                                row.col(|ui| {
                                    let cat_text = tx.category.as_deref().unwrap_or("-");
                                    ui.label(egui::RichText::new(cat_text).color(palette.weak));
                                });

                                // Debit / Credit / Balance - money fields with red border on parse failure.
                                Self::money_cell(
                                    &mut row,
                                    &mut self.workflow_cell_buffers,
                                    &self.workflow_edits,
                                    tx,
                                    EditField::Debit,
                                    tx.debit,
                                    palette.warn,
                                    &mut cell_changes,
                                );
                                Self::money_cell(
                                    &mut row,
                                    &mut self.workflow_cell_buffers,
                                    &self.workflow_edits,
                                    tx,
                                    EditField::Credit,
                                    tx.credit,
                                    palette.warn,
                                    &mut cell_changes,
                                );
                                Self::money_cell(
                                    &mut row,
                                    &mut self.workflow_cell_buffers,
                                    &self.workflow_edits,
                                    tx,
                                    EditField::RunningBalance,
                                    tx.running_balance,
                                    palette.warn,
                                    &mut cell_changes,
                                );

                                // Revert column
                                row.col(|ui| {
                                    let label = if has_edit { "↶" } else { " " };
                                    if ui
                                        .add_enabled(
                                            has_edit,
                                            egui::Button::new(label).small(),
                                        )
                                        .on_hover_text("Revert all queued edits on this row")
                                        .clicked()
                                    {
                                        row_reverts.push((tx.page, tx.line_on_page));
                                    }
                                });
                            });
                        }
                    });
            });

        // Apply collected changes after the table render so we don't double-borrow self.
        for (page, line, field, new_text, bbox, old_text) in cell_changes {
            self.upsert_edit(UserEdit {
                page,
                line_on_page: line,
                bbox,
                old_text,
                new_text,
                field,
            });
        }
        if !row_reverts.is_empty() {
            for (page, line) in row_reverts {
                self.revert_row_edits(page, line);
            }
        }
    }

    /// Pick the per-field bbox for an edit. Falls back to the row-level
    /// bbox when the field-specific one isn't known (older parses, manual
    /// transactions). Stage 7.5 - without this, a debit edit would redact
    /// the entire row.
    fn bbox_for_field(
        tx: &crate::engine::model::Transaction,
        field: crate::engine::workflow::EditField,
    ) -> [f32; 4] {
        use crate::engine::workflow::EditField;
        let specific = match field {
            EditField::Date => tx.field_bboxes.date,
            EditField::Description => tx.field_bboxes.description,
            EditField::Debit => tx.field_bboxes.debit,
            EditField::Credit => tx.field_bboxes.credit,
            EditField::RunningBalance => tx.field_bboxes.running_balance,
        };
        specific.or(tx.bbox).unwrap_or([0.0; 4])
    }

    /// Get-or-init the per-cell text buffer. If the user has already queued
    /// an edit for this cell, the buffer reflects the queued new text;
    /// otherwise it starts from the parsed value.
    fn cell_buffer<'a>(
        buffers: &'a mut std::collections::HashMap<
            (usize, usize, crate::engine::workflow::EditField),
            String,
        >,
        edits: &[crate::engine::workflow::UserEdit],
        tx: &crate::engine::model::Transaction,
        field: crate::engine::workflow::EditField,
        default: impl FnOnce() -> String,
    ) -> &'a mut String {
        let key = (tx.page, tx.line_on_page, field);
        buffers.entry(key).or_insert_with(|| {
            edits
                .iter()
                .find(|e| {
                    e.page == tx.page && e.line_on_page == tx.line_on_page && e.field == field
                })
                .map(|e| e.new_text.clone())
                .unwrap_or_else(default)
        })
    }

    /// Render a single money cell (debit/credit/balance). Red border when
    /// the typed text isn't parseable.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn money_cell(
        row: &mut egui_extras::TableRow<'_, '_>,
        buffers: &mut std::collections::HashMap<
            (usize, usize, crate::engine::workflow::EditField),
            String,
        >,
        edits: &[crate::engine::workflow::UserEdit],
        tx: &crate::engine::model::Transaction,
        field: crate::engine::workflow::EditField,
        original: Option<rust_decimal::Decimal>,
        warn_color: egui::Color32,
        out: &mut Vec<(
            usize,
            usize,
            crate::engine::workflow::EditField,
            String,
            [f32; 4],
            String,
        )>,
    ) {
        row.col(|ui| {
            let buf = Self::cell_buffer(buffers, edits, tx, field, || {
                original.map(|v| format!("{v:.2}")).unwrap_or_default()
            });
            let valid = buf.trim().is_empty()
                || buf
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
                    .collect::<String>()
                    .parse::<f64>()
                    .is_ok();
            let mut edit = egui::TextEdit::singleline(buf).desired_width(80.0);
            if !valid {
                edit = edit.text_color(warn_color);
            }
            let resp = ui.add(edit);
            if resp.changed() {
                let old_text = original.map(|v| format!("{v:.2}")).unwrap_or_default();
                out.push((
                    tx.page,
                    tx.line_on_page,
                    field,
                    buf.clone(),
                    Self::bbox_for_field(tx, field),
                    old_text,
                ));
            }
        });
    }

    /// Insert or replace the edit on (page, line, field). When the new
    /// text equals the originally-parsed value, the edit is removed from
    /// the queue instead - typing a value back to its original is
    /// equivalent to no edit at all.
    fn upsert_edit(&mut self, mut edit: crate::engine::workflow::UserEdit) {
        // If the new text equals the original, drop any matching edit.
        let parsed_original = self
            .workflow_transactions
            .iter()
            .find(|t| t.page == edit.page && t.line_on_page == edit.line_on_page);
        let original_text = parsed_original
            .map(|t| match edit.field {
                crate::engine::workflow::EditField::Date => t.date.clone(),
                crate::engine::workflow::EditField::Description => t.raw_text.clone(),
                crate::engine::workflow::EditField::Debit => t
                    .debit
                    .map(|v| format!("{:.2}", v.round_dp(2)))
                    .unwrap_or_default(),
                crate::engine::workflow::EditField::Credit => t
                    .credit
                    .map(|v| format!("{:.2}", v.round_dp(2)))
                    .unwrap_or_default(),
                crate::engine::workflow::EditField::RunningBalance => t
                    .running_balance
                    .map(|v| format!("{:.2}", v.round_dp(2)))
                    .unwrap_or_default(),
            })
            .unwrap_or_default();

        // Use the original text we just looked up.
        if edit.old_text.is_empty() {
            edit.old_text = original_text.clone();
        }

        // No-op if the user typed back to the original - drop it.
        if edit.new_text == original_text {
            self.workflow_edits.retain(|e| {
                !(e.page == edit.page
                    && e.line_on_page == edit.line_on_page
                    && e.field == edit.field)
            });
            self.workflow_dirty = true;
            return;
        }

        if let Some(slot) = self.workflow_edits.iter_mut().find(|e| {
            e.page == edit.page && e.line_on_page == edit.line_on_page && e.field == edit.field
        }) {
            slot.new_text = edit.new_text;
            slot.bbox = edit.bbox;
        } else {
            self.workflow_edits.push(edit);
        }
        self.workflow_dirty = true;
    }

    /// Drop every queued edit on (page, line) and reset the cell buffers
    /// for that row so the table reflects the parsed values.
    fn revert_row_edits(&mut self, page: usize, line_on_page: usize) {
        let before = self.workflow_edits.len();
        self.workflow_edits
            .retain(|e| !(e.page == page && e.line_on_page == line_on_page));
        let removed = before.saturating_sub(self.workflow_edits.len());
        // Clear the cached cell buffers for this row so they re-init from
        // the parsed transaction next frame.
        self.workflow_cell_buffers
            .retain(|(p, l, _), _| !(*p == page && *l == line_on_page));
        if removed > 0 {
            self.workflow_dirty = true;
            self.toast(
                ToastKind::Info,
                format!(
                    "Reverted {} edit(s) on P{} L{}",
                    removed,
                    page + 1,
                    line_on_page + 1
                ),
            );
        }
    }

    /// Stage 8.5: per-font breakdown for the loaded PDF. Shows the user which
    /// fonts can be edited freely and which would need glyph creation, with
    /// an exact list of missing characters per font and the creation scope.
    pub fn draw_font_analysis_section(&mut self, ui: &mut egui::Ui) {
        let palette = self.settings.theme.palette();
        let analysis = match &self.font_analysis {
            Some(a) => a.clone(),
            None => {
                ui.collapsing("🔤 Font analysis", |ui| {
                    ui.label("Loading...");
                    if ui.button("Re-analyze").clicked() {
                        if let Err(e) = self.job_tx.send(Job::AnalyzeFonts {
                            path: PathBuf::from(&self.input_path),
                        }) {
                            tracing::error!("Runtime disconnected: {}", e);
                        }
                        self.in_flight += 1;
                    }
                });
                return;
            }
        };

        let header = if analysis.summary.all_fonts_covered {
            format!(
                "🔤 Font analysis - ✅ {} font(s), all covered",
                analysis.summary.total_fonts
            )
        } else {
            format!(
                "🔤 Font analysis - ⚠ {}/{} font(s) need attention",
                analysis.summary.fonts_needing_action, analysis.summary.total_fonts
            )
        };

        ui.collapsing(header, |ui| {
            // High-level summary line.
            let summary_color = if analysis.summary.all_fonts_covered {
                palette.success
            } else {
                palette.warn
            };
            ui.colored_label(summary_color, analysis.one_line_summary());

            if !analysis.summary.all_fonts_covered {
                ui.horizontal(|ui| {
                    if analysis.summary.missing_digit_count > 0 {
                        ui.colored_label(
                            palette.warn,
                            format!("Digits: {}", analysis.summary.missing_digit_count),
                        );
                    }
                    if analysis.summary.missing_letter_count > 0 {
                        ui.colored_label(
                            palette.warn,
                            format!("Letters: {}", analysis.summary.missing_letter_count),
                        );
                    }
                    if analysis.summary.missing_other_count > 0 {
                        ui.colored_label(
                            palette.warn,
                            format!("Other: {}", analysis.summary.missing_other_count),
                        );
                    }
                });
            }

            ui.separator();

            if ui.button("🔄 Re-analyze").clicked() {
                if let Err(e) = self.job_tx.send(Job::AnalyzeFonts {
                    path: PathBuf::from(&self.input_path),
                }) {
                    tracing::error!("Runtime disconnected: {}", e);
                }
                self.in_flight += 1;
            }

            ui.separator();

            // Per-font breakdown.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .id_salt("font-analysis-list")
                .max_height(280.0)
                .show(ui, |ui| {
                    for (i, font) in analysis.fonts.iter().enumerate() {
                        let needs_action = !font.missing_chars.is_empty();
                        let row_color = if needs_action {
                            palette.warn
                        } else {
                            palette.success
                        };
                        let role_label = match font.usage_role {
                            crate::engine::font_analysis::UsageRole::Digits => "digits",
                            crate::engine::font_analysis::UsageRole::Letters => "letters",
                            crate::engine::font_analysis::UsageRole::Mixed => "mixed",
                            crate::engine::font_analysis::UsageRole::Punctuation => "punct",
                            crate::engine::font_analysis::UsageRole::Other => "other",
                        };
                        let header = format!(
                            "{} {} • {} • {} use(s) on {} page(s)",
                            if needs_action { "⚠" } else { "✅" },
                            font.base_name,
                            role_label,
                            font.occurrences,
                            font.pages_used_on.len(),
                        );
                        let id = ui.make_persistent_id(("font-analysis", i));
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            id,
                            false,
                        )
                        .show_header(ui, |ui| {
                            ui.colored_label(row_color, header);
                        })
                        .body(|ui| {
                            ui.label(font.fidelity_impact.as_str());
                            ui.label(font.creation_scope.as_str());
                            ui.small(format!(
                                "Standard-14: {} • Subset: {}",
                                if font.is_standard_14 { "yes" } else { "no" },
                                if font.is_subset { "yes" } else { "no" },
                            ));
                            // Truncate the used-character preview at 80 chars
                            // so a font with hundreds of glyphs doesn't dominate
                            // the panel.
                            let used_preview: String = if font.characters_used.chars().count() > 80
                            {
                                let head: String = font.characters_used.chars().take(80).collect();
                                format!("{head}...")
                            } else {
                                font.characters_used.clone()
                            };
                            ui.small(format!("Used characters: {used_preview}"));
                            if !font.missing_chars.is_empty() {
                                let missing_str = font.missing_chars.join(" ");
                                ui.colored_label(palette.warn, format!("Missing: {missing_str}"));
                                let bd = &font.missing_breakdown;
                                if !bd.digits.is_empty() {
                                    ui.small(format!("  Digits: {}", bd.digits.join(" ")));
                                }
                                if !bd.letters.is_empty() {
                                    ui.small(format!("  Letters: {}", bd.letters.join(" ")));
                                }
                                if !bd.other.is_empty() {
                                    ui.small(format!("  Other: {}", bd.other.join(" ")));
                                }
                            }
                            ui.small(format!(
                                "Sizes: {:.1}-{:.1}pt • Pages: {}",
                                font.size_range[0],
                                font.size_range[1],
                                font.pages_used_on
                                    .iter()
                                    .map(|p| (p + 1).to_string())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            ));
                        });
                    }
                });
        });
    }

    pub fn draw_workflow_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("🤖 Workflow (AI parse -> preview -> render -> verify)", |ui| {
            let stage = self.workflow_stage.clone();
            let p = self.settings.theme.palette();

            // Step indicator. Stage 13 / Item #1: each label is hoverable
            // so the user can read what the step actually does, and the
            // active step gets a strong color so the indicator never looks
            // muted at idle.
            let step = stage.step_index();
            ui.horizontal(|ui| {
                let descriptions = [
                    "Run Document AI + Gemini completeness check",
                    "Edit values inline; queued edits go to Preview",
                    "Recompute every running balance with your edits",
                    "Apply edits to the PDF (binary-level redact-and-replace)",
                    "Render & compare; loop until visual match passes",
                    "Re-parse with Document AI to confirm math integrity",
                ];
                for (i, name) in [
                    "Parse", "Edit", "Preview", "Render", "Verify", "Confirm",
                ]
                .iter()
                .enumerate()
                {
                    let active_step = (i + 1) as u8;
                    let (color, label) = if active_step < step {
                        (p.success, format!("✓ {}. {name}", i + 1))
                    } else if active_step == step.min(6) {
                        (p.accent, format!("► {}. {name}", i + 1))
                    } else {
                        (p.weak, format!("{}. {name}", i + 1))
                    };
                    ui.colored_label(color, label).on_hover_text(descriptions[i]);
                }
            });
            ui.label(format!("Status: {}", stage.label()));

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Parser Version:");
                egui::ComboBox::from_id_salt("parser_version_select")
                    .selected_text(self.selected_parser_version.split('-').nth(2).unwrap_or(&self.selected_parser_version))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v5.0-2023-12-06".to_string(), "v5.0 (Default)");
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v4.0-2023-07-31".to_string(), "v4.0");
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v3.0-2022-05-16".to_string(), "v3.0");
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v2.0-2021-12-10".to_string(), "v2.0");
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v1.1-2021-08-13".to_string(), "v1.1");
                    });
                if ui.button("🔄 Parse").on_hover_text("Re-parse document with selected parser version").clicked() && !self.input_path.is_empty() {
                    if let Err(e) = self.dispatch_workflow_job(Job::WorkflowParseAndValidate {
                        input: PathBuf::from(&self.input_path),
                        version: Some(self.selected_parser_version.clone()),
                        parser_mode: self.settings.document_parser,
                        ai_provider: self.settings.ai_provider,
                        ignore_offline_fallback: false,
                    }) { tracing::error!("Runtime disconnected: {}", e); }
                    self.in_flight += 1;
                    self.workflow_edits.clear();
                    self.workflow_preview = None;
                    self.workflow_visual = None;
                    self.workflow_outcome = None;
                    self.font_cascade_reports.clear();
                    self.workflow_dirty = true;
                    self.toast(ToastKind::Info, "Parse triggered");
                }
            });

            ui.separator();



            if let Some(v) = &self.workflow_validation {
                ui.label(format!(
                    "Found {} txs • opening ${:.2} • closing ${:.2}",
                    v.transactions_found, v.opening_balance, v.closing_balance
                ));
                let bar_color = if v.is_acceptable() { p.success } else { p.warn };
                ui.colored_label(
                    bar_color,
                    format!("AI completeness: {:.0}%", v.completeness_score * 100.0),
                );
                if !v.completeness_notes.is_empty() {
                    ui.small(&v.completeness_notes);
                }
                if !v.missing_rows.is_empty() {
                    ui.colored_label(p.warn, format!("Possibly missing rows: {}", v.missing_rows.len()));
                    for m in v.missing_rows.iter().take(3) {
                        ui.small(format!("  • {m}"));
                    }
                }
            }

            ui.separator();

            // Stage 5 / Item #6 + #8: inline edit table with per-row revert.
            self.draw_workflow_edit_table(ui);

            ui.separator();

            // Natural Language Interface
            ui.label(egui::RichText::new("✨ Natural Language Edit (Beta)").strong());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.natural_language_prompt)
                    .hint_text("e.g. Change all Starbucks to Dunkin")
                    .desired_width(280.0));
                let can_edit = !self.natural_language_prompt.is_empty() && !self.workflow_transactions.is_empty();
                if ui.add_enabled(can_edit, egui::Button::new("Apply")).clicked() {
                    if let Err(e) = self.job_tx.send(crate::app::runtime::Job::NaturalLanguageEdit {
                        prompt: self.natural_language_prompt.clone(),
                        transactions: self.workflow_transactions.clone(),
                    }) {
                        tracing::error!("Runtime disconnected: {}", e);
                    }
                    self.in_flight += 1;
                    self.natural_language_prompt.clear();
                }
            });

            ui.separator();

            // Stage 3 button: balance preview
            let preview_enabled = self.workflow_validation.is_some();
            ui.label(format!("Pending edits queued: {}", self.workflow_edits.len()));
            if ui
                .add_enabled(preview_enabled, egui::Button::new("② Balance Out Preview"))
                .on_hover_text("Recompute every running balance with your edits and show the diff")
                .clicked()
            {
                if let Some(v) = &self.workflow_validation {
                    if let Err(e) = self.dispatch_workflow_job(Job::WorkflowPreview {
                        original_transactions: self.workflow_transactions.clone(),
                        edits: self.workflow_edits.clone(),
                        opening_balance: v.opening_balance,
                        expected_closing: if v.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                            Some(v.closing_balance)
                        } else {
                            None
                        },
                    }) { tracing::error!("Runtime disconnected: {}", e); }
                    self.in_flight += 1;
                }
            }

            if let Some(p) = &self.workflow_preview {
                let changed = p.rows.iter().filter(|r| r.will_change).count();
                let kind_color = if p.balanced { self.settings.theme.palette().success } else { self.settings.theme.palette().warn };
                ui.colored_label(
                    kind_color,
                    format!(
                        "{} row(s) will change • final imbalance ${:.2}",
                        changed, p.final_imbalance
                    ),
                );

                if !p.balanced && ui.button("🧠 Ask Local AI to Explain").clicked() {
                    let opening_balance = self
                        .workflow_validation
                        .as_ref()
                        .map(|v| {
                            v.opening_balance
                                .to_string()
                                .parse::<f64>()
                                .unwrap_or(0.0)
                        })
                        .unwrap_or(0.0);
                    let closing_balance = self
                        .workflow_validation
                        .as_ref()
                        .map(|v| {
                            v.closing_balance
                                .to_string()
                                .parse::<f64>()
                                .unwrap_or(0.0)
                        })
                        .unwrap_or(0.0);
                    let imbalance_f64 = p
                        .final_imbalance
                        .to_string()
                        .parse::<f64>()
                        .unwrap_or(0.0);

                    if let Err(e) = self.job_tx.send(crate::app::runtime::Job::ExplainImbalance {
                        transactions_json: serde_json::to_string(&self.workflow_transactions)
                            .unwrap_or_default(),
                        opening_balance,
                        closing_balance,
                        imbalance: imbalance_f64,
                    }) {
                        tracing::error!("Runtime disconnected: {}", e);
                    }
                    self.in_flight += 1;
                    self.ai_explanation = None;
                }

                let mut clear_explanation = false;
                if let Some(explanation) = &self.ai_explanation {
                    ui.add_space(4.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Local AI Forensics").strong().color(self.settings.theme.palette().success));
                        ui.label(explanation);
                        if ui.button("Dismiss").clicked() {
                            clear_explanation = true;
                        }
                    });
                }
                if clear_explanation {
                    self.ai_explanation = None;
                }

                if let Some(msg) = &p.auto_correction_message {
                    ui.small(msg);
                }
                // Compact diff list
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(120.0).show(ui, |ui| {
                    for r in p.rows.iter().filter(|r| r.will_change).take(20) {
                        // Char-aware truncation so multi-byte UTF-8 (CJK,
                        // accented Latin) doesn't panic on byte slicing.
                        let desc_short: String = if r.description.chars().count() > 24 {
                            let head: String = r.description.chars().take(24).collect();
                            format!("{head}...")
                        } else {
                            r.description.clone()
                        };
                        ui.small(format!(
                            "P{} L{} {} • bal {:?} -> {:?}",
                            r.page + 1,
                            r.line_on_page + 1,
                            desc_short,
                            r.old_running_balance,
                            r.new_running_balance,
                        ));
                    }
                });
            }

            ui.separator();

            // Stage 4-6: Quick (native) vs Deep (PyMuPDF) apply options.
            // Dual-engine safety: both engines stay loaded; if one fails the
            // other takes over. The user picks the fidelity tier per apply and
            // can re-run Deep if Quick doesn't suffice.
            let confirm_enabled = self.workflow_preview.is_some();
            let edit_count = self.workflow_edits.len().max(1);
            // Rough ETAs so the user can weigh speed vs fidelity. Native is ~1s
            // per edit; PyMuPDF Deep adds Pro per-segment work + deep font
            // replication overhead (~3s per edit plus a fixed warm-up).
            let _quick_eta = 2 + edit_count;
            let deep_eta = 5 + edit_count * 3;
            ui.label("3. Finalize Edits:");
            ui.horizontal(|ui| {
                let pro_btn_id = ui.id().with("pro_edit_btn");
                let hovered_pro = ui.ctx().data(|d| d.get_temp::<bool>(pro_btn_id)).unwrap_or(false);
                let anim_pro = ui.ctx().animate_bool_with_time(pro_btn_id, hovered_pro, 0.15);

                let pro_btn = egui::Button::new(
                    egui::RichText::new(format!("🎯 Perform Pro Edit • ~{deep_eta}s"))
                        .size(14.0 + anim_pro)
                ).fill(egui::Color32::from_rgb((20.0 * anim_pro) as u8, 80 + (anim_pro * 40.0) as u8, (40.0 * anim_pro) as u8));

                let resp_pro = ui.add_enabled(confirm_enabled, pro_btn)
                    .on_hover_text("High-fidelity PyMuPDF Pro apply with deep font replication.");

                if resp_pro.hovered() != hovered_pro {
                    ui.ctx().data_mut(|d| d.insert_temp(pro_btn_id, resp_pro.hovered()));
                    ui.ctx().request_repaint();
                }

                if resp_pro.clicked() {
                    self.dispatch_confirm_and_render(true, false);
                }

                let ai_btn_id = ui.id().with("verify_ai_btn");
                let hovered_ai = ui.ctx().data(|d| d.get_temp::<bool>(ai_btn_id)).unwrap_or(false);
                let anim_ai = ui.ctx().animate_bool_with_time(ai_btn_id, hovered_ai, 0.15);

                let ai_btn = egui::Button::new(
                    egui::RichText::new("🤖 Verify with AI")
                        .size(14.0 + anim_ai)
                ).fill(egui::Color32::from_rgb((80.0 * anim_ai) as u8, (30.0 * anim_ai) as u8, 120 + (anim_ai * 40.0) as u8));

                let resp_ai = ui.add_enabled(confirm_enabled, ai_btn)
                    .on_hover_text("Cross-check the edits and layout with Gemini Vision before finalizing.");

                if resp_ai.hovered() != hovered_ai {
                    ui.ctx().data_mut(|d| d.insert_temp(ai_btn_id, resp_ai.hovered()));
                    ui.ctx().request_repaint();
                }

                if resp_ai.clicked() {
                    self.toast(ToastKind::Info, "Dispatching AI verification...");
                    // We can dispatch a Job::Verify (or similar) here, but for now we'll trigger a background verification via AI.
                    let input = PathBuf::from(&self.input_path);
                    if let Err(e) = self.job_tx.send(crate::app::runtime::Job::AiCommand {
                        prompt: "Verify that all tabular edits align with the original styling and are mathematically sound.".to_string(),
                        path: input,
                    }) {
                        tracing::error!("Failed to dispatch AI verify: {}", e);
                    }
                }
            });
            ui.small(format!(
                "All edits are applied instantly via Native Rust. Use Pro Edit for final polish. {} edit(s) applied.",
                self.workflow_edits.len()
            ));

            if let Some(va) = &self.workflow_visual {
                let palette = self.settings.theme.palette();
                let c = if va.passed() { palette.success } else { palette.warn };
                ui.colored_label(
                    c,
                    format!(
                        "Visual {}/{} • diff {:.4} • intended-only {}",
                        va.attempt,
                        va.max_attempts,
                        va.diff_score,
                        if va.only_intended { "✓" } else { "✗" }
                    ),
                );
            }
            if let Some(o) = &self.workflow_outcome {
                ui.colored_label(self.settings.theme.palette().success, &o.completion_summary);
                ui.small(format!("Final PDF: {}", o.final_pdf.display()));
                ui.small(format!(
                    "Re-parsed transactions: {} • final imbalance ${:.2}",
                    o.transactions_re_parsed, o.final_imbalance
                ));
            }

            if let crate::engine::workflow::WorkflowStage::FontCoverageWarning { missing_chars } = &self.workflow_stage {
                ui.separator();
                let palette = self.settings.theme.palette();
                ui.colored_label(palette.warn, "Font Coverage Block");
                ui.label(format!("The replacement requires characters absent from the selected embedded or supplied font:\n{:?}", missing_chars));
                ui.label("Automatic typeface substitution is disabled because it would not preserve statement fidelity. Change the replacement text or supply a reviewed font that covers every character.");
                if ui.button("Return to Edit Review").clicked() {
                    let preview = self.workflow_preview.clone().unwrap_or_default();
                    self.apply_workflow_event(
                        crate::engine::workflow::WorkflowEvent::ResumePreview(preview),
                    );
                }
            }

            // Stage 12 / Item #3: surface cascade results so the user can
            // see exactly which tier(s) closed any font-coverage gap.
            if !self.font_cascade_reports.is_empty() {
                ui.separator();
                ui.label("🔧 Font cascade history:");
                let palette = self.settings.theme.palette();
                for report in &self.font_cascade_reports {
                    let color = if report.success { palette.success } else { palette.warn };
                    ui.colored_label(
                        color,
                        format!(
                            "Attempt {} on '{}': {}",
                            report.workflow_attempt,
                            report.original_font,
                            report.one_line_summary()
                        ),
                    );
                    if !report.synthesised.is_empty() {
                        ui.small(format!("  composite: {}", report.synthesised.join(", ")));
                    }
                    if !report.donor_extended.is_empty() {
                        ui.small(format!("  donor:     {}", report.donor_extended.join(", ")));
                    }
                    if !report.ai_extended.is_empty() {
                        ui.small(format!("  AI donor:  {}", report.ai_extended.join(", ")));
                    }
                    if !report.still_missing.is_empty() {
                        ui.colored_label(
                            palette.warn,
                            format!("  still missing: {}", report.still_missing.join(", ")),
                        );
                    }
                }
            }
        });
    }

    /// Apply the queued workflow edits to the PDF and render-validate them.
    ///
    /// `deep` selects the Deep fidelity tier (PyMuPDF Pro per-segment edit +
    /// deep font replication); when `false` the Quick (native) tier runs. Both
    /// tiers share the redundant-edit pruning so the apply loop stays tight, and
    /// both run under the dual-engine safety net so a single engine failure
    /// falls back to the other rather than aborting the edit.
    pub(crate) fn dispatch_confirm_and_render(&mut self, deep: bool, ignore_font_coverage: bool) {
        // Stage 2 / Item #7: drop edits whose typed value already matches the
        // cascade. Reduces visual noise (extra redactions) and shortens the
        // apply loop.
        let edits_to_apply = if let Some(p) = &self.workflow_preview {
            let (kept, dropped) =
                crate::engine::workflow::prune_redundant_edits(&self.workflow_edits, p);
            if !dropped.is_empty() {
                self.toast(
                    ToastKind::Info,
                    format!("Pruned {} redundant edit(s)", dropped.len()),
                );
            }
            kept
        } else {
            self.workflow_edits.clone()
        };
        self.toast(
            ToastKind::Info,
            if deep {
                "Applying with Deep (PyMuPDF) fidelity..."
            } else {
                "Applying with Quick (Native) fidelity..."
            },
        );
        if let Err(e) = self.dispatch_workflow_job(Job::WorkflowConfirmAndRender {
            input: PathBuf::from(&self.input_path),
            output: PathBuf::from(&self.output_path),
            edits: edits_to_apply,
            original_transactions: self.workflow_transactions.clone(),
            opening_balance: self
                .workflow_validation
                .as_ref()
                .map(|v| v.opening_balance)
                .unwrap_or_default(),
            expected_closing: self.workflow_validation.as_ref().and_then(|v| {
                if v.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                    Some(v.closing_balance)
                } else {
                    None
                }
            }),
            deep_font_replication: deep,
            max_visual_attempts: self.settings.max_visual_attempts,
            visual_threshold: self.settings.visual_diff_threshold,
            ignore_font_coverage,
            ignore_visual_fidelity: false,
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
    }

    #[allow(dead_code)]
    fn draw_batch_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Batch Processing Dashboard");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("📂 Select Directory").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.batch_folder_path = Some(path.clone());
                        self.batch_files.clear();
                        if let Ok(entries) = std::fs::read_dir(&path) {
                            for entry in entries.filter_map(|e| e.ok()) {
                                let p = entry.path();
                                if p.is_file() && p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) == Some("pdf".to_string()) {
                                    self.batch_files.push(p);
                                }
                            }
                        }
                    }
                }
                if let Some(path) = &self.batch_folder_path {
                    ui.label(format!("Selected: {}", path.display()));
                } else {
                    ui.label("Drag and drop a folder of statements here, or click to select a directory.");
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let has_files = !self.batch_files.is_empty();
                if ui.add_enabled(has_files, egui::Button::new("Extract All to JSON")).clicked() {
                    for file in &self.batch_files {
                        if let Err(e) = self.job_tx.send(Job::ExtractTransactions {
                            path: file.clone(),
                            parser_mode: self.settings.document_parser,
                        }) { tracing::error!("Runtime disconnected: {}", e); }
                        self.in_flight += 1;
                    }
                    self.toast(ToastKind::Info, format!("Queued {} extraction jobs", self.batch_files.len()));
                }
                if ui.add_enabled(has_files, egui::Button::new("Auto-Balance All")).clicked() {
                    for file in &self.batch_files {
                        let output = file.with_file_name(format!("{}_balanced.pdf", file.file_stem().unwrap_or_default().to_string_lossy()));
                        if let Err(e) = self.job_tx.send(Job::BalanceAndApplyAll {
                            input: file.clone(),
                            output,
                            auto_apply: true,
                        }) { tracing::error!("Runtime disconnected: {}", e); }
                        self.in_flight += 1;
                    }
                    self.toast(ToastKind::Info, format!("Queued {} balancing jobs", self.batch_files.len()));
                }
                if ui.add_enabled(has_files, egui::Button::new("Verify All against Originals")).clicked() {
                    let pairs = Self::pair_originals_and_edited(&self.batch_files);
                    if pairs.is_empty() {
                        self.toast(ToastKind::Warn, "No paired _original/_edited PDFs found in folder.");
                    } else {
                        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                        for (original, edited) in &pairs {
                            let stem = edited.file_stem().unwrap_or_default().to_string_lossy().to_string();
                            if let Err(e) = self.job_tx.send(Job::Verify {
                                original: original.clone(),
                                edited: edited.clone(),
                                output_dir: PathBuf::from("audit/verify/batch").join(&timestamp).join(&stem),
                                intended_edits: Vec::new(),
                                use_pdfrest: self.settings.verification_renderer == crate::app::config::VerificationMode::PdfRestCloud,
                                pdfrest_key: self.config.pdfrest_api_key.clone(),
                                auto_match_dpi: self.settings.auto_match_dpi,
                            }) { tracing::error!("Runtime disconnected: {}", e); }
                            self.in_flight += 1;
                        }
                        self.toast(ToastKind::Info, format!("Queued {} verification job(s)", pairs.len()));
                    }
                }
            });

            ui.add_space(10.0);

            if !self.batch_files.is_empty() {
                ui.heading(format!("{} PDF(s) found", self.batch_files.len()));
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for file in &self.batch_files {
                        ui.label(file.file_name().unwrap_or_default().to_string_lossy());
                    }
                });
            }
        });
    }

    #[allow(dead_code)]
    fn draw_audit_explorer_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Audit Explorer unavailable in v1");
            ui.label("Use the immutable exported audit and verification evidence. No simulated explorer is provided.");
        });
    }

    fn draw_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Toolbar above canvas
            ui.horizontal(|ui| {
                if ui.button("🔍-").clicked() {
                    self.zoom_factor = (self.zoom_factor * 0.85).clamp(0.1, 5.0);
                    self.fit_to_view = false;
                }
                if ui.button("🔍+").clicked() {
                    self.zoom_factor = (self.zoom_factor * 1.15).clamp(0.1, 5.0);
                    self.fit_to_view = false;
                }
                if ui.button("Fit").clicked() {
                    self.fit_to_view = true;
                }
                if ui.button("100%").clicked() {
                    self.zoom_factor = 1.0;
                    self.pan_offset = egui::Vec2::ZERO;
                    self.fit_to_view = false;
                }
                ui.separator();
                ui.checkbox(&mut self.show_curtain, "Curtain Diff");
                if self.show_curtain {
                    ui.add(egui::Slider::new(&mut self.curtain_ratio, 0.0..=1.0).text("split"));
                }
            });

            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                let (response, painter) = ui.allocate_painter(
                    ui.available_size(),
                    egui::Sense::drag().union(egui::Sense::click()),
                );

                // Zoom - Ctrl+wheel
                let zoom_scroll = ui.input(|i| {
                    if i.modifiers.command {
                        i.smooth_scroll_delta.y
                    } else {
                        0.0
                    }
                });
                if zoom_scroll != 0.0 {
                    self.zoom_factor = (self.zoom_factor + zoom_scroll * 0.002).clamp(0.1, 5.0);
                    self.fit_to_view = false;
                }

                // Pan - any drag (primary, middle, etc.)
                if response.dragged() {
                    self.pan_offset += response.drag_delta();
                    self.fit_to_view = false;
                }

                if let Some(texture) = self.current_page_texture.clone() {
                    let tex_size = texture.size_vec2();
                    if self.fit_to_view {
                        self.fit_zoom_to_view(response.rect.size(), tex_size);
                    }
                    let size = tex_size * self.zoom_factor;
                    let center = response.rect.center() + self.pan_offset;
                    let rect = egui::Rect::from_center_size(center, size);

                    painter.image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );

                    // Curtain diff: paint the "after" texture clipped to ratio
                    if self.show_curtain {
                        if let Some(after) = self.after_texture.clone() {
                            let split_x = rect.min.x + rect.width() * self.curtain_ratio;
                            let after_rect =
                                egui::Rect::from_min_max(egui::pos2(split_x, rect.min.y), rect.max);
                            let uv_min = egui::pos2(self.curtain_ratio, 0.0);
                            painter.image(
                                after.id(),
                                after_rect,
                                egui::Rect::from_min_max(uv_min, egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                            painter.line_segment(
                                [
                                    egui::pos2(split_x, rect.min.y),
                                    egui::pos2(split_x, rect.max.y),
                                ],
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 0)),
                            );
                        }

                        // Hotspot Highlighting: draw red/green boxes around modified regions
                        if let Some((w, h)) = self.current_page_size_pts {
                            for edit in &self.workflow_edits {
                                if edit.page == self.current_page {
                                    let [x0, y0, x1, y1] = edit.bbox;
                                    let sx0 = rect.min.x + (x0 / w) * size.x;
                                    let sy0 = rect.min.y + (y0 / h) * size.y;
                                    let sx1 = rect.min.x + (x1 / w) * size.x;
                                    let sy1 = rect.min.y + (y1 / h) * size.y;

                                    let item_rect = egui::Rect::from_min_max(
                                        egui::pos2(sx0, sy0),
                                        egui::pos2(sx1, sy1),
                                    );

                                    let split_x = rect.min.x + rect.width() * self.curtain_ratio;

                                    if sx0 < split_x {
                                        let mut r = item_rect;
                                        r.max.x = r.max.x.min(split_x);
                                        painter.rect_stroke(r, 2.0, egui::Stroke::new(2.0, egui::Color32::from_rgba_premultiplied(255, 50, 50, 150)));
                                    }

                                    if sx1 > split_x {
                                        let mut r = item_rect;
                                        r.min.x = r.min.x.max(split_x);
                                        painter.rect_stroke(r, 2.0, egui::Stroke::new(2.0, egui::Color32::from_rgba_premultiplied(50, 255, 50, 150)));
                                    }

                                    // Phase 2 - Stage 1: Smart Alignment Guides (on hover)
                                    if let Some(pos) = response.hover_pos() {
                                        if item_rect.contains(pos) {
                                            // Draw crosshair alignment lines matching Figma's smart guides
                                            let p = self.settings.theme.palette();
                                            let guide_color = p.accent.linear_multiply(0.4);

                                            // Horizontal guide through center
                                            painter.hline(response.rect.min.x..=response.rect.max.x, item_rect.center().y, egui::Stroke::new(1.0, guide_color));

                                            // Vertical guide through center
                                            painter.vline(item_rect.center().x, response.rect.min.y..=response.rect.max.y, egui::Stroke::new(1.0, guide_color));

                                            // Highlight the bounds
                                            painter.rect_stroke(item_rect, 0.0, egui::Stroke::new(2.0, p.accent));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Click -> resolve text block via Python
                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            self.last_click_pos = Some(pos);
                            let relative = pos - rect.min;
                            let (x, y) = if let Some((w, h)) = self.current_page_size_pts {
                                (relative.x * w / size.x, relative.y * h / size.y)
                            } else {
                                (relative.x / self.zoom_factor, relative.y / self.zoom_factor)
                            };
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            if self
                                .job_tx
                                .send(Job::Python(
                                    PythonJob::FindTextBlockAtClick {
                                        pdf_path: self
                                            .current_pdf_path
                                            .to_string_lossy()
                                            .to_string(),
                                        page_num: self.current_page,
                                        x,
                                        y,
                                    },
                                    tx,
                                ))
                                .is_ok()
                            {
                                self.pending_python = Some(rx);
                                self.in_flight += 1;
                            }
                        }
                    }

                    // Selected bbox highlight
                    if let Some(block) = &self.selected_block {
                        if block.page == self.current_page {
                            let (sx, sy) = if let Some((w, h)) = self.current_page_size_pts {
                                (size.x / w, size.y / h)
                            } else {
                                (self.zoom_factor, self.zoom_factor)
                            };
                            let min = rect.min + egui::vec2(block.bbox[0] * sx, block.bbox[1] * sy);
                            let max = rect.min + egui::vec2(block.bbox[2] * sx, block.bbox[3] * sy);
                            painter.rect_stroke(
                                egui::Rect::from_min_max(min, max),
                                4.0,
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 200, 0)),
                            );
                        }
                    }

                    // Stage 5 / Item #20: live diff overlay during preview.
                    // Translucent yellow over each `will_change` bbox on the
                    // current page; tooltip shows old -> new.
                    if let Some(preview) = self.workflow_preview.clone() {
                        let (sx, sy) = if let Some((w, h)) = self.current_page_size_pts {
                            (size.x / w, size.y / h)
                        } else {
                            (self.zoom_factor, self.zoom_factor)
                        };
                        let mouse = response.hover_pos();
                        for prow in preview
                            .rows
                            .iter()
                            .filter(|r| r.will_change && r.page == self.current_page)
                        {
                            // Find the underlying transaction so we can use its bbox.
                            let Some(tx) = self.workflow_transactions.iter().find(|t| {
                                t.page == prow.page && t.line_on_page == prow.line_on_page
                            }) else {
                                continue;
                            };
                            let Some(bbox) = tx.bbox else {
                                continue;
                            };
                            let min = rect.min + egui::vec2(bbox[0] * sx, bbox[1] * sy);
                            let max = rect.min + egui::vec2(bbox[2] * sx, bbox[3] * sy);
                            let cell = egui::Rect::from_min_max(min, max);
                            // Translucent yellow fill + amber border.
                            painter.rect_filled(
                                cell,
                                2.0,
                                egui::Color32::from_rgba_unmultiplied(255, 220, 0, 70),
                            );
                            painter.rect_stroke(
                                cell,
                                2.0,
                                egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgb(220, 180, 0),
                                ),
                            );
                            // Hover tooltip with the diff text (Phase 2 - Stage 2: Advanced Hover Cards)
                            if let Some(m) = mouse {
                                if cell.contains(m) {
                                    let old_str = prow
                                        .old_running_balance
                                        .map(|v| format!("{v:.2}"))
                                        .unwrap_or_else(|| "-".into());
                                    let new_str = prow
                                        .new_running_balance
                                        .map(|v| format!("{v:.2}"))
                                        .unwrap_or_else(|| "-".into());

                                    let cell_resp = ui.allocate_rect(cell, egui::Sense::hover());
                                    cell_resp.on_hover_ui(|ui| {
                                        let p = self.settings.theme.palette();
                                        egui::Frame::none()
                                            .inner_margin(egui::vec2(12.0, 10.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new("🔍 Math Correction").color(p.accent).strong());
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("P{} L{}", prow.page + 1, prow.line_on_page + 1)).weak().size(12.0));
                                                    });
                                                });
                                                ui.add_space(6.0);

                                                // Display old vs new with clear coloring
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new(old_str).color(p.warn).strikethrough());
                                                    ui.label(egui::RichText::new(" ➔ ").color(p.weak));
                                                    ui.label(egui::RichText::new(new_str).color(p.success).strong());
                                                });

                                                ui.add_space(6.0);
                                                ui.small(egui::RichText::new("Balance automatically re-calculated by Engine").color(p.text.linear_multiply(0.7)));
                                            });
                                    });
                                }
                            }
                        }
                    }

                    // Minimap overlay (Phase 2 - Stage 1)
                    if self.zoom_factor > 1.05 {
                        let minimap_w = 120.0;
                        let minimap_h = minimap_w * (tex_size.y / tex_size.x);
                        let minimap_size = egui::vec2(minimap_w, minimap_h);

                        let minimap_rect = egui::Rect::from_min_size(
                            response.rect.max - minimap_size - egui::vec2(24.0, 24.0),
                            minimap_size,
                        );

                        // Background
                        painter.rect_filled(minimap_rect, 4.0, egui::Color32::from_black_alpha(180));
                        painter.rect_stroke(minimap_rect, 4.0, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(30)));

                        // Render full page texture scaled down
                        painter.image(
                            texture.id(),
                            minimap_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );

                        // Indicator box showing the visible portion
                        let vis_min_uv = (response.rect.min - rect.min) / rect.size();
                        let vis_max_uv = (response.rect.max - rect.min) / rect.size();

                        let clamped_min = vis_min_uv.clamp(egui::vec2(0.0, 0.0), egui::vec2(1.0, 1.0));
                        let clamped_max = vis_max_uv.clamp(egui::vec2(0.0, 0.0), egui::vec2(1.0, 1.0));

                        let ind_rect = egui::Rect::from_min_max(
                            minimap_rect.min + clamped_min * minimap_rect.size(),
                            minimap_rect.min + clamped_max * minimap_rect.size(),
                        );

                        painter.rect_filled(ind_rect, 2.0, self.settings.theme.palette().accent.linear_multiply(0.2));
                        painter.rect_stroke(ind_rect, 2.0, egui::Stroke::new(1.5, self.settings.theme.palette().accent));
                    }
                } else {
                    // Welcome / empty placeholder
                    self.draw_empty_canvas(ui, response.rect, &painter);
                }
            });
            let mut dock = egui::Area::new(egui::Id::new("floating_action_dock")).order(egui::Order::Foreground);
            if self.selected_block.is_some() {
                if let Some(pos) = self.last_click_pos {
                    dock = dock.current_pos(pos + egui::vec2(20.0, 20.0));
                } else {
                    dock = dock.anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0));
                }
            } else {
                dock = dock.anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0));
            }

            dock.show(ctx, |ui| {
                if self.selected_block.is_some() || !self.proposed_changes.is_empty() {
                    let p = self.settings.theme.palette();
                    egui::Frame::window(ui.style())
                        .fill(p.surface.linear_multiply(0.85))
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(0.0, 10.0),
                            blur: 30.0,
                            spread: 0.0,
                            color: egui::Color32::from_black_alpha(80),
                        })
                        .rounding(16.0)
                        .stroke(egui::Stroke::new(1.0, p.text.linear_multiply(0.1)))
                        .inner_margin(egui::Margin::symmetric(20.0, 16.0))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                // Primary Contextual Action
                                if self.selected_block.is_some() {
                                    ui.horizontal(|ui| {
                                        let apply_btn = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("🎯 Apply Single Edit")
                                                    .color(p.bg).strong()
                                            )
                                            .fill(p.accent)
                                            .rounding(8.0)
                                            .min_size(egui::vec2(160.0, 36.0))
                                        );

                                        if apply_btn.on_hover_text("Replace the selected text and instantly verify math + fidelity.").clicked() {
                                            if let Some(block) = self.selected_block.clone() {
                                                let input = if self.current_pdf_path.exists() {
                                                    self.current_pdf_path.clone()
                                                } else {
                                                    std::path::PathBuf::from(&self.input_path)
                                                };
                                                let edit = crate::engine::workflow::UserEdit {
                                                    page: self.current_page,
                                                    line_on_page: 0,
                                                    bbox: block.bbox,
                                                    old_text: block.text.clone(),
                                                    new_text: self.new_text.clone(),
                                                    field: crate::engine::workflow::EditField::Description,
                                                };
                                                if let Err(e) = self.dispatch_workflow_job(Job::WorkflowConfirmAndRender {
                                                    input,
                                                    output: std::path::PathBuf::from(&self.output_path),
                                                    edits: vec![edit],
                                                    original_transactions: self.workflow_transactions.clone(),
                                                    opening_balance: self.workflow_validation.as_ref().map(|v| v.opening_balance).unwrap_or_default(),
                                                    expected_closing: self.workflow_validation.as_ref().and_then(|v| {
                                                        if v.closing_balance.abs() > rust_decimal::Decimal::ZERO { Some(v.closing_balance) } else { None }
                                                    }),
                                                    deep_font_replication: self.settings.deep_font_replication,
                                                    max_visual_attempts: self.settings.max_visual_attempts.min(3),
                                                    visual_threshold: self.settings.visual_diff_threshold.max(0.05),
                                                    ignore_font_coverage: false,
                                                    ignore_visual_fidelity: false,
                                                }) { tracing::error!("Runtime disconnected: {}", e); }
                                                self.in_flight += 1;
                                            }
                                        }

                                        ui.add_space(8.0);

                                    });

                                    ui.add_space(12.0);
                                    let mut rect = ui.min_rect();
                                    rect.max.y = rect.min.y + 1.0;
                                    ui.painter().rect_filled(rect, 0.0, p.text.linear_multiply(0.05));
                                    ui.add_space(12.0);
                                }

                                // Global Action Row
                                ui.horizontal(|ui| {
                                    let adjust_btn = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("⚖ Auto-Balance Statement")
                                                .color(p.panel).strong()
                                        )
                                        .fill(p.success)
                                        .rounding(8.0)
                                        .min_size(egui::vec2(200.0, 36.0))
                                    );
                                    if adjust_btn.on_hover_text("Computes minimal adjustments for the entire statement and applies them automatically.").clicked() {
                                        let input = if self.current_pdf_path.exists() { self.current_pdf_path.clone() } else { std::path::PathBuf::from(&self.input_path) };
                                        if input.as_os_str().is_empty() || !input.exists() {
                                            self.toast(ToastKind::Error, "Open a PDF first.");
                                        } else {
                                            if let Err(e) = self.job_tx.send(Job::BalanceAndApplyAll {
                                                input, output: std::path::PathBuf::from(&self.output_path), auto_apply: true,
                                            }) { tracing::error!("Runtime disconnected: {}", e); }
                                            self.in_flight += 1;
                                            self.status = "Auto-balancing entire statement...".into();
                                            self.toast(ToastKind::Info, "Auto-balancing entire statement...");
                                        }
                                    }

                                    ui.add_space(8.0);
                                    if ui.add(egui::Button::new(egui::RichText::new("📅 Dates").color(p.text)).fill(p.bg).rounding(8.0).min_size(egui::vec2(80.0, 36.0))).on_hover_text("Adjust all transaction dates").clicked() {
                                        self.active_modal = ActiveModal::DateAdjust;
                                    }

                                    ui.add_space(8.0);
                                    if ui.add(egui::Button::new(egui::RichText::new("🔄 Transfer").color(p.text)).fill(p.bg).rounding(8.0).min_size(egui::vec2(90.0, 36.0))).on_hover_text("Transfer from another PDF").clicked() {
                                        self.active_modal = ActiveModal::Transfer;
                                    }
                                });
                            });
                        });
                }
            });
        });
    }

    fn draw_empty_canvas(&mut self, ui: &mut egui::Ui, rect: egui::Rect, painter: &egui::Painter) {
        let p = self.settings.theme.palette();

        // --- 1. Background Gradient ---
        painter.rect_filled(rect, 0.0, p.bg);

        // Ambient glows in the background
        let center = rect.center();
        let glow_radius = 400.0;
        let glow_color = p.accent.linear_multiply(0.05);

        painter.circle_filled(center + egui::vec2(-200.0, -150.0), glow_radius, glow_color);
        painter.circle_filled(
            center + egui::vec2(250.0, 200.0),
            glow_radius * 0.8,
            p.success.linear_multiply(0.03),
        );

        // --- 2. Asynchronous Loading State ---
        if self.current_pdf_path.exists() {
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        egui::Frame::none()
                            .inner_margin(egui::Margin::same(32.0))
                            .rounding(egui::Rounding::same(24.0))
                            .fill(p.surface.linear_multiply(0.8))
                            .shadow(egui::epaint::Shadow {
                                offset: egui::vec2(0.0, 12.0),
                                blur: 24.0,
                                spread: 0.0,
                                color: egui::Color32::from_black_alpha(40),
                            })
                            .stroke(egui::Stroke::new(1.0, p.surface.linear_multiply(0.5)))
                            .show(ui, |ui| {
                                // Phase 2 - Stage 2: Shimmering Skeleton Loader
                                let time = ui.input(|i| i.time);
                                let alpha = ((time * 4.0).sin() as f32 * 0.3 + 0.7) * 0.15;
                                let skel_color = p.text.linear_multiply(alpha);

                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(200.0, 80.0),
                                    egui::Sense::hover(),
                                );
                                let painter = ui.painter();
                                painter.rect_filled(
                                    egui::Rect::from_min_size(rect.min, egui::vec2(200.0, 24.0)),
                                    4.0,
                                    skel_color,
                                );
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        rect.min + egui::vec2(0.0, 40.0),
                                        egui::vec2(160.0, 14.0),
                                    ),
                                    4.0,
                                    skel_color,
                                );
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        rect.min + egui::vec2(0.0, 64.0),
                                        egui::vec2(120.0, 14.0),
                                    ),
                                    4.0,
                                    skel_color,
                                );
                                ui.ctx().request_repaint();

                                ui.add_space(16.0);
                                ui.label(
                                    egui::RichText::new("Rendering document...")
                                        .color(p.text)
                                        .size(18.0)
                                        .strong(),
                                );
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("Applying AI vision and structure mapping")
                                        .color(p.weak)
                                        .size(14.0),
                                );
                            });
                    });
                });
            });
            return;
        }

        // --- 3. Welcome Glass Panel ---
        let panel_width = 460.0;
        let panel_height = 420.0;
        let panel_rect =
            egui::Rect::from_center_size(center, egui::vec2(panel_width, panel_height));

        // Glassmorphism effect
        painter.rect(
            panel_rect,
            egui::Rounding::same(24.0),
            p.surface.linear_multiply(0.85),
            egui::Stroke::new(1.5, p.text.linear_multiply(0.1)),
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(panel_rect), |ui| {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                // Icon
                ui.label(egui::RichText::new("✨").size(48.0));
                ui.add_space(16.0);

                // Title
                ui.label(
                    egui::RichText::new("Antigravity Statement Forensics")
                        .size(26.0)
                        .strong()
                        .color(p.text),
                );

                ui.add_space(8.0);

                // Subtitle
                ui.label(
                    egui::RichText::new(
                        "High-Fidelity Ledger Processing & AI-Driven Reconciliation",
                    )
                    .size(14.0)
                    .color(p.weak),
                );

                ui.add_space(40.0);

                // Primary Action Button
                let btn = egui::Button::new(
                    egui::RichText::new("⊕ Initialize Workspace Session")
                        .size(16.0)
                        .strong()
                        .color(p.bg),
                )
                .min_size(egui::vec2(320.0, 52.0))
                .rounding(egui::Rounding::same(12.0))
                .fill(p.accent);

                if ui
                    .add(btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PDF", &["pdf"])
                        .pick_file()
                    {
                        self.open_pdf(path);
                    }
                }

                ui.add_space(16.0);

                ui.label(
                    egui::RichText::new("or drag and drop a PDF file here")
                        .size(13.0)
                        .italics()
                        .color(p.weak.linear_multiply(0.7)),
                );

                ui.add_space(30.0);

                ui.horizontal(|ui| {
                    ui.add_space(70.0); // Center the secondary actions
                    if ui
                        .button(
                            egui::RichText::new("⟲ Restore Previous Session")
                                .size(13.0)
                                .color(p.text),
                        )
                        .clicked()
                    {
                        let auto = std::path::PathBuf::from("audit").join("history.json");
                        if auto.exists() {
                            if let Err(e) =
                                self.job_tx.send(crate::app::runtime::Job::LoadHistory {
                                    input: auto.clone(),
                                })
                            {
                                tracing::error!("Runtime disconnected: {}", e);
                            }
                            self.in_flight += 1;
                            self.toast(
                                ToastKind::Info,
                                format!("Resuming from {}", auto.display()),
                            );
                        } else {
                            self.toast(ToastKind::Warn, "No previous session found.");
                        }
                    }
                    ui.add_space(10.0);
                    if ui
                        .button(
                            egui::RichText::new("📝 Recover Editing Draft")
                                .size(13.0)
                                .color(p.text),
                        )
                        .clicked()
                    {
                        self.resume_workflow_draft();
                    }
                });
            });
        });
    }

    /// Stage 13 / Item #12: confirmation modals.
    fn draw_toasts(&mut self, ctx: &egui::Context) -> Option<String> {
        let mut clicked_id = None;
        // Drop expired
        let now = Instant::now();
        self.toasts.retain(|t| t.expires_at > now);
        if self.toasts.is_empty() {
            return None;
        }

        egui::Area::new("toasts".into())
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -32.0))
            .show(ctx, |ui| {
                ui.vertical_centered_justified(|ui| {
                    let p = self.settings.theme.palette();
                    for toast in self.toasts.iter().rev().take(5) {
                        let bg = match toast.kind {
                            ToastKind::Info => p.info,
                            ToastKind::Warn => p.warn,
                            ToastKind::Error => p.error,
                            ToastKind::Success => p.success,
                        };
                        let icon = match toast.kind {
                            ToastKind::Info => "ℹ",
                            ToastKind::Warn => "⚠",
                            ToastKind::Error => "✗",
                            ToastKind::Success => "✓",
                        };

                        // Remaining lifetime based alpha
                        let remaining = toast.expires_at.saturating_duration_since(now);
                        let base_alpha = (remaining.as_millis() as f32 / 6000.0).clamp(0.0, 1.0);

                        // Slide-in / slide-out animation
                        let anim_id = egui::Id::new("toast").with(&toast.text);
                        let target = if base_alpha > 0.05 { 1.0 } else { 0.0 };
                        let slide = ctx.animate_value_with_time(anim_id, target, 0.3);

                        let final_alpha = (base_alpha.min(slide) * 230.0) as u8;
                        if final_alpha == 0 {
                            continue;
                        }

                        let bg = egui::Color32::from_rgba_unmultiplied(
                            bg.r(),
                            bg.g(),
                            bg.b(),
                            final_alpha,
                        );
                        let fg = egui::Color32::from_white_alpha((255.0 * slide) as u8);

                        ui.horizontal(|ui| {
                            // Pushes the toast from the right to slide it in
                            ui.add_space((1.0 - slide) * 300.0);

                            egui::Frame::none()
                                .fill(bg)
                                .rounding(10.0)
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_white_alpha((40.0 * slide) as u8),
                                ))
                                .inner_margin(egui::vec2(12.0, 8.0))
                                .shadow(egui::epaint::Shadow {
                                    offset: egui::vec2(0.0, 4.0 * slide),
                                    blur: 12.0 * slide,
                                    spread: 0.0,
                                    color: egui::Color32::from_black_alpha((60.0 * slide) as u8),
                                })
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(fg, icon);
                                        ui.colored_label(fg, &toast.text);
                                        if let Some(label) = &toast.action_label {
                                            ui.add_space(8.0);
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new(label).color(fg),
                                                    )
                                                    .fill(egui::Color32::from_black_alpha(100)),
                                                )
                                                .clicked()
                                            {
                                                clicked_id = toast.action_id.clone();
                                            }
                                        }
                                    });
                                });
                        });
                        ui.add_space(6.0 * slide);
                    }
                });
            });

        if let Some(id) = &clicked_id {
            self.toasts.retain(|t| t.action_id.as_ref() != Some(id));
        }

        clicked_id
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        // Read individual shortcut states instead of cloning the entire InputState.
        let ctrl_o = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O));
        let ctrl_z = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z));
        let ctrl_y = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Y));
        let ctrl_s = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));
        let page_down = ctx
            .input(|i| i.key_pressed(egui::Key::PageDown) || i.key_pressed(egui::Key::ArrowRight));
        let page_up =
            ctx.input(|i| i.key_pressed(egui::Key::PageUp) || i.key_pressed(egui::Key::ArrowLeft));
        let zoom_in =
            ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
        let zoom_out = ctx.input(|i| i.key_pressed(egui::Key::Minus));
        let zoom_reset = ctx.input(|i| i.key_pressed(egui::Key::Num0));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        if escape {
            self.active_modal = ActiveModal::None;
            self.active_modal = ActiveModal::None;
            self.active_modal = ActiveModal::None;
            self.active_modal = ActiveModal::None;
        }

        if ctrl_o {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .pick_file()
            {
                self.open_pdf(path);
            }
        }
        if ctrl_z {
            if let Err(e) = self.job_tx.send(Job::Undo) {
                tracing::error!("Runtime disconnected: {}", e);
            }
        }
        if ctrl_y {
            if let Err(e) = self.job_tx.send(Job::Redo) {
                tracing::error!("Runtime disconnected: {}", e);
            }
        }
        if ctrl_s {
            if let Err(e) = self.job_tx.send(Job::ExportChangeHistory {
                output: PathBuf::from(&self.export_path),
            }) {
                tracing::error!("Runtime disconnected: {}", e);
            }
        }
        if page_down && self.current_page + 1 < self.total_pages {
            self.current_page += 1;
            self.request_render("current");
        }
        if page_up && self.current_page > 0 {
            self.current_page -= 1;
            self.request_render("current");
        }
        if zoom_in {
            self.zoom_factor = (self.zoom_factor * 1.15).clamp(0.1, 5.0);
            self.fit_to_view = false;
        }
        if zoom_out {
            self.zoom_factor = (self.zoom_factor * 0.85).clamp(0.1, 5.0);
            self.fit_to_view = false;
        }
        if zoom_reset {
            self.zoom_factor = 1.0;
            self.pan_offset = egui::Vec2::ZERO;
            self.fit_to_view = false;
        }
    }

    pub fn open_pdf(&mut self, path: PathBuf) {
        if !path.exists() {
            self.toast(
                ToastKind::Error,
                format!("File not found: {}", path.display()),
            );
            return;
        }
        self.input_path = path.to_string_lossy().to_string();
        self.activate_run_workspace(&path);
        self.current_pdf_path = path.clone();
        self.previous_pdf_path = None;
        self.history_state = ChangeHistory::new();
        self.proposed_changes.clear();
        self.last_imbalance = None;
        self.last_verification = None;
        self.last_warning = None;
        self.selected_block = None;
        // Stage 6: opening a new PDF invalidates any cached hash and any
        // in-flight workflow buffers - those belong to the previous file.
        self.workflow_input_hash = None;
        self.workflow_cell_buffers.clear();
        // Stage 8.5: clear the font analysis; the runtime will produce a
        // fresh one for the new PDF.
        self.font_analysis = None;
        if let Err(e) = self.job_tx.send(Job::LoadDocument {
            path: self.current_pdf_path.clone(),
            three_page_mode: self.settings.three_page_mode,
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
    }

    /// Path of the on-disk autosave for the current workflow. One file per
    /// session - overwritten as edits change. Stage 5 / Item #9.
    pub fn workflow_draft_path() -> PathBuf {
        crate::app::paths::AppPaths::discover()
            .map(|paths| paths.audit_dir().join("workflow.json"))
            .unwrap_or_else(|_| PathBuf::from("audit").join("workflow.json"))
    }

    /// Delete the on-disk draft if it exists. Used after a successful
    /// `WorkflowComplete` and from the "Discard draft" menu. Errors are
    /// logged but never surfaced - the file may legitimately be missing.
    pub fn discard_workflow_draft_quiet() {
        let path = Self::workflow_draft_path();
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("[gui] removing workflow draft failed: {}", e);
            }
        }
    }

    /// Persist the current workflow state to `audit/workflow.json` if there
    /// is anything worth saving (validation has been done, OR there are
    /// queued edits) and the dirty flag is set. Debounced to at most one
    /// write per 1.5s. Failures are logged but never raised - losing an
    /// autosave is non-fatal.
    fn autosave_workflow_draft(&mut self) {
        if !self.workflow_dirty {
            return;
        }
        // Nothing to save until a parse has produced a baseline.
        if self.workflow_validation.is_none() && self.workflow_edits.is_empty() {
            self.workflow_dirty = false;
            return;
        }
        // Debounce: 1.5s between writes.
        if let Some(t) = self.workflow_last_save {
            if t.elapsed() < Duration::from_millis(1500) {
                return;
            }
        }
        let pdf = PathBuf::from(&self.input_path);
        if !pdf.exists() {
            self.workflow_dirty = false;
            return;
        }
        // Cache the PDF SHA-256 once per (input_path, file change) so the
        // autosave doesn't re-read multi-MB files every 1.5s. The cache key
        // is just the path; if the user opens a new PDF the cache is
        // cleared in `open_pdf`. Stage 6.
        let hash = match &self.workflow_input_hash {
            Some((cached_path, h)) if cached_path == &self.input_path => h.clone(),
            _ => {
                let bytes = match std::fs::read(&pdf) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("[gui] reading PDF for hash failed: {}", e);
                        self.workflow_dirty = false;
                        return;
                    }
                };
                let h = crate::engine::workflow::sha256_hex_of(&bytes);
                self.workflow_input_hash = Some((self.input_path.clone(), h.clone()));
                h
            }
        };
        let draft = crate::engine::workflow::WorkflowDraft::new_with_hash(
            &pdf,
            hash,
            self.workflow_validation.clone(),
            self.workflow_transactions.clone(),
            self.workflow_edits.clone(),
        );
        let path = self.active_workflow_draft_path();
        match draft.save_to_file(&path) {
            Ok(()) => {
                tracing::debug!("[gui] saved workflow draft to {}", path.display());
                self.workflow_dirty = false;
                self.workflow_last_save = Some(Instant::now());
            }
            Err(e) => {
                tracing::warn!("[gui] saving workflow draft failed: {}", e);
                // Leave dirty=true so we retry next frame.
            }
        }
    }

    /// Resume a workflow from `audit/workflow.json`, restoring validation,
    /// transactions and queued edits. Verifies the on-disk PDF still
    /// hashes to what the draft expects; if not, surfaces a warning toast
    /// but proceeds - the user might intentionally be loading a draft
    /// against a manually-saved copy.
    fn resume_workflow_draft(&mut self) {
        let path = self.active_workflow_draft_path();
        if !path.exists() {
            self.toast(ToastKind::Warn, "No workflow draft to resume.");
            return;
        }
        let draft = match crate::engine::workflow::WorkflowDraft::load_from_file(&path) {
            Ok(d) => d,
            Err(e) => {
                self.toast(ToastKind::Error, format!("Could not load draft: {e}"));
                return;
            }
        };

        // Stage 13 / Item #11: when the original PDF is missing, prompt the
        // user to locate it instead of orphaning the draft. If they
        // cancel, leave the draft untouched so they can retry later.
        let mut pdf_path = PathBuf::from(&draft.input_path);
        if !pdf_path.exists() {
            self.toast(
                ToastKind::Warn,
                format!("PDF missing: {} - please pick the file", pdf_path.display()),
            );
            match rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_title("Locate the PDF this draft was saved against")
                .pick_file()
            {
                Some(picked) => {
                    pdf_path = picked;
                }
                None => {
                    self.toast(
                        ToastKind::Info,
                        "Resume cancelled - draft kept; pick the PDF later.",
                    );
                    return;
                }
            }
        }

        let same = draft.matches_pdf(&pdf_path);
        // Restore session state.
        self.input_path = pdf_path.to_string_lossy().to_string();
        self.activate_run_workspace(&pdf_path);
        self.current_pdf_path = pdf_path.clone();
        self.workflow_validation = draft.validation.clone();
        self.workflow_transactions = draft.transactions.clone();
        self.workflow_edits = draft.edits.clone();
        self.workflow_preview = None;
        self.workflow_visual = None;
        self.workflow_outcome = None;
        self.apply_workflow_event(crate::engine::workflow::WorkflowEvent::Reset);
        if let Some(validation) = draft.validation.clone() {
            self.apply_workflow_event(crate::engine::workflow::WorkflowEvent::RestoreEditing(
                validation,
            ));
        }
        self.workflow_dirty = false;

        // Trigger a render of the PDF.
        if let Err(e) = self.job_tx.send(Job::LoadDocument {
            path: pdf_path.clone(),
            three_page_mode: self.settings.three_page_mode,
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;

        if same {
            self.toast(
                ToastKind::Success,
                format!(
                    "Resumed workflow draft - {} edits queued",
                    draft.edits.len()
                ),
            );
        } else {
            self.toast(
                ToastKind::Warn,
                "Draft loaded but the PDF has changed since it was saved.",
            );
        }
    }
}

/// Upsert `pairs` (env var name -> value) into a dotenv file at `path`.
///
/// Existing `KEY=...` lines are replaced in place (preserving order and
/// unrelated lines/comments); keys not present are appended. A key whose
/// value is empty is written as `KEY=` so the file documents that it was
/// intentionally cleared (the live process env already had it removed by the
/// caller). Creates the file if it does not exist.
fn upsert_env_file(path: &std::path::Path, pairs: &[(&str, String)]) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();

    // Track which keys we've already written so leftovers get appended.
    let mut remaining: std::collections::HashMap<&str, &String> =
        pairs.iter().map(|(k, v)| (*k, v)).collect();

    let mut out_lines: Vec<String> = Vec::new();
    for line in existing.lines() {
        let trimmed = line.trim_start();
        // Leave comments and blank lines untouched.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out_lines.push(line.to_string());
            continue;
        }
        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            if let Some(val) = remaining.remove(key) {
                out_lines.push(format!("{key}={val}"));
                continue;
            }
        }
        out_lines.push(line.to_string());
    }

    // Append any keys that weren't already present.
    if !remaining.is_empty() {
        // Deterministic order for the appended block.
        let mut appended: Vec<(&str, &String)> = pairs
            .iter()
            .filter(|(k, _)| remaining.contains_key(*k))
            .map(|(k, v)| (*k, v))
            .collect();
        appended.dedup_by(|a, b| a.0 == b.0);
        for (k, v) in appended {
            out_lines.push(format!("{k}={v}"));
        }
    }

    let mut contents = out_lines.join("\n");
    contents.push('\n');
    std::fs::write(path, contents)
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "Inter-Regular".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "Inter-Bold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/Inter-Bold.ttf"
        ))),
    );

    if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        prop.insert(0, "Inter-Regular".to_owned());
    }

    if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        mono.push("Inter-Regular".to_owned());
    }

    fonts.families.insert(
        egui::FontFamily::Name("Bold".into()),
        vec!["Inter-Bold".to_owned(), "Inter-Regular".to_owned()],
    );

    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn load_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../../assets/icon.png"))
        .expect("Failed to open icon path")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

pub fn run_gui(
    job_tx: RuntimeClient,
    job_rx: std::sync::mpsc::Receiver<JobResult>,
    config: std::sync::Arc<crate::app::config::AppConfig>,
) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 640.0])
            .with_title("Bank Statement Fidelity Editor v0.5.0")
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Bank Statement Fidelity Editor",
        options,
        Box::new(move |cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(MyApp::new(job_tx, job_rx, config.clone())))
        }),
    )
}
