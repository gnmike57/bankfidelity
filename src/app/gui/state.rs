//! Application state and core domain types for the GUI.
#![allow(unused_imports)]

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::app::runtime::{
    Job, JobId, JobResult, PythonJob, PythonJobResult, RuntimeClient, RuntimeSubmitError,
};
use crate::engine::history::ChangeHistory;
use crate::engine::verification::VerificationReport;
use egui_plot::PlotPoints;

pub use crate::app::theme::{Palette, Theme};
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
            document_parser: crate::app::config::DocumentParserMode::from_env(),
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
    pub previous_pdf_path: Option<PathBuf>,
    pub export_path: String,

    // Document state
    pub current_page: usize,
    pub total_pages: usize,
    pub history_state: ChangeHistory,

    // Batch Processing
    pub batch_folder_path: Option<PathBuf>,
    pub batch_files: Vec<PathBuf>,

    // View
    pub current_view: AppView,
    pub active_workflow: ActiveWorkflow,
    pub sidebar_expanded: bool,
    pub zoom_factor: f32,
    pub pan_offset: egui::Vec2,
    pub show_curtain: bool,
    pub curtain_ratio: f32,
    pub fit_to_view: bool,

    // Selection
    pub selected_block: Option<TextBlock>,
    pub last_click_pos: Option<egui::Pos2>,
    pub new_text: String,

    // Natural Language Editing
    pub natural_language_prompt: String,

    // Textures
    pub current_page_texture: Option<egui::TextureHandle>,
    pub before_texture: Option<egui::TextureHandle>,
    pub after_texture: Option<egui::TextureHandle>,
    pub transfer_source_texture: Option<egui::TextureHandle>,
    pub transfer_target_texture: Option<egui::TextureHandle>,
    pub current_page_dpi: f32,
    pub current_page_size_pts: Option<(f32, f32)>,

    // App / job state
    pub status: String,
    pub progress: Option<ProgressState>,
    pub last_warning: Option<String>,
    pub last_verification: Option<VerificationReport>,
    pub proposed_changes: Vec<(crate::engine::model::ProposedChange, bool)>,
    pub last_imbalance: Option<rust_decimal::Decimal>,
    pub in_flight: usize,
    pub active_workflow_job_id: Option<JobId>,
    pub ai_explanation: Option<String>,
    pub settings: AppSettings,
    pub toasts: VecDeque<Toast>,

    // Channels
    pub job_tx: RuntimeClient,
    pub job_rx: std::sync::mpsc::Receiver<JobResult>,
    pub pending_python: Option<tokio::sync::oneshot::Receiver<PythonJobResult>>,
    pub app_paths: crate::app::paths::AppPaths,
    pub run_workspace: Option<crate::app::paths::RunWorkspace>,

    // Render coalescing
    pub last_render_request: Option<(String, usize, u32)>,

    // Multi-stage workflow state
    pub workflow_stage: crate::engine::workflow::WorkflowStage,
    pub workflow_transactions: Vec<crate::engine::model::Transaction>,
    pub workflow_validation: Option<crate::engine::workflow::ParseValidation>,
    #[allow(dead_code)]
    pub workflow_df: Option<polars::frame::DataFrame>,
    #[allow(dead_code)]
    pub workflow_edits: Vec<crate::engine::workflow::UserEdit>,
    pub workflow_preview: Option<crate::engine::workflow::BalancePreview>,
    pub workflow_visual: Option<crate::engine::workflow::VisualAttempt>,
    pub workflow_outcome: Option<crate::engine::workflow::WorkflowOutcome>,
    pub last_runtime_activity: std::time::Instant,
    pub stuck_detection: Option<std::time::Instant>,
    #[allow(dead_code)]
    pub native_engine: Option<std::sync::Arc<dyn crate::pdf::PdfEngine>>,

    /// Stage 8.5: per-font breakdown for the loaded PDF, populated
    /// automatically when `JobResult::FontAnalysisReady` arrives.
    pub font_analysis: Option<crate::engine::font_analysis::FontAnalysis>,
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
    pub font_cascade_reports: Vec<crate::engine::font_analysis::FontCascadeReport>,

    // Telemetry
    pub telemetry_cpu: f32,
    pub telemetry_ram_mb: u64,

    /// True when in-memory workflow state has changed since the last
    /// autosave to `audit/workflow.json`. Set whenever
    /// `workflow_validation`, `workflow_transactions` or `workflow_edits`
    /// is mutated; cleared after a successful save. Stage 5 / Item #9.
    pub workflow_dirty: bool,
    /// Last instant we wrote `audit/workflow.json`. Used to debounce - at
    /// most one save every 1.5s while edits are flying in.
    pub workflow_last_save: Option<Instant>,
    /// Cached `(input_path, sha256)` for the currently-open PDF so the
    /// autosave doesn't re-hash multi-MB files every 1.5s. Stage 6.
    pub workflow_input_hash: Option<(String, String)>,
    /// Per-cell text buffers for the inline edit table. Keyed by
    /// (page, line_on_page, field). Stage 5 / Item #6.
    pub workflow_cell_buffers:
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
    pub config_status: Option<(bool, bool, bool)>,
    /// Boot-time (and reload-time) API availability snapshot. Drives the
    /// UI auto-exclusion of unavailable backends with explanatory messages.
    pub api_availability: crate::app::config::ApiAvailability,
    pub capability_registry: crate::app::capabilities::CapabilityRegistry,
    /// Result of the last `Job::ValidateCredentials` run. (Gemini, DocAI).
    pub credential_validation_status: Option<(Result<(), String>, Result<(), String>)>,
    /// True once the buffers have been seeded from the environment.
    #[allow(dead_code)]
    pub api_keys_seeded: bool,
    /// Latest real-time API health polling results.
    pub api_health: Option<Vec<crate::app::api_verification::VerificationResult>>,

    /// Proposed auto-fix for the last encountered error
    pub pending_autofix: Option<crate::app::error::AppError>,
    /// Selected parser version for Document AI
    pub selected_parser_version: String,

    // -- Document AI Version Manager --
    /// Cached list of available processor versions
    pub docai_versions: Vec<crate::ai::document_ai::ProcessorVersionInfo>,
    /// True while fetching versions from the API
    pub docai_versions_loading: bool,
    /// Whether to show the version management panel
    pub docai_training_status: Option<String>,
    /// Active long-running operation name (training, deploy, etc.)
    pub docai_active_operation: Option<String>,
    /// Streaming logs from the UFO process (public for E2E/status surfaces).
    pub ufo_logs: Vec<String>,
    /// Whether UFO is actively running (public for E2E and cancel UI state).
    pub is_ufo_running: bool,
    /// User clicked Cancel while UFO was running. Terminal `Error` /
    /// `UfoAutoEditResult` still frees `in_flight` once via
    /// `ends_gui_tracked_job`; this flag only suppresses post-cancel error UX.
    pub ufo_user_cancelled: bool,
}

/// Font file extensions the custom-font drop target in the settings modal
/// advertises ("Drag and drop .ttf or .otf files here").
pub const SUPPORTED_FONT_EXTENSIONS: [&str; 2] = ["ttf", "otf"];

/// True when `path` has a font extension accepted by the custom-font drop
/// target. Shared with the global document drop handlers in `gui.rs`, which
/// must skip font files entirely so a drop that did not land on the target
/// can never raise the font-upload error flow.
pub fn is_supported_font_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_FONT_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

pub trait CommandPalette {
    fn draw_command_palette(&mut self, ctx: &egui::Context);
}

#[allow(dead_code)]
pub trait AppModals {
    fn draw_settings_modal(&mut self, ctx: &egui::Context);
    fn draw_backend_preferences(&mut self, ui: &mut egui::Ui);
    fn draw_transfer_dialog(&mut self, ctx: &egui::Context);
    fn draw_date_adjust_dialog(&mut self, ctx: &egui::Context);
    fn draw_ai_confirmation_dialog(&mut self, ctx: &egui::Context);
    fn draw_interactive_fallback_modal(&mut self, ctx: &egui::Context);
    fn draw_autofix_modal(&mut self, ctx: &egui::Context);
    fn draw_workflow_hitl_modal(&mut self, ctx: &egui::Context);
    fn draw_transfer_test_dialog(&mut self, ctx: &egui::Context);
    fn draw_api_keys_editor(&mut self, ui: &mut egui::Ui);
    fn draw_feedback_modal(&mut self, ctx: &egui::Context);
    fn draw_modals(&mut self, ctx: &egui::Context);
    fn draw_stuck_watchdog_modal(&mut self, ctx: &egui::Context);
    fn draw_discard_draft_confirm_modal(&mut self, ctx: &egui::Context);
}
