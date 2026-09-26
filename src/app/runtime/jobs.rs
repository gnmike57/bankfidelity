use super::ids::JobId;
use super::python_job::{PythonJob, PythonJobResult};
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;

#[derive(Debug)]
pub enum Job {
    Ping,
    UfoAutoEdit {
        path: PathBuf,
        context: String,
    },
    CancelUfo,
    Python(PythonJob, oneshot::Sender<PythonJobResult>),
    LoadDocument {
        path: PathBuf,
        three_page_mode: bool,
    },
    /// Stage 8.5: standalone font analysis trigger. Useful from a "Re-analyze"
    /// menu in the GUI; LoadDocument also fires this automatically.
    AnalyzeFonts {
        path: PathBuf,
    },
    RenderPage {
        path: PathBuf,
        page: usize,
        dpi: f32,
        tag: String,
    },
    ApplyChange {
        input: PathBuf,
        output: PathBuf,
        page: usize,
        bbox: [f32; 4],
        new_text: String,
        old_text: String,
        description: String,
        deep_font_replication: bool,
    },
    CompleteFont {
        path: PathBuf,
        font_name: String,
    },
    Undo,
    Redo,
    BalanceStatement {
        path: PathBuf,
    },
    ExtractTransactions {
        path: PathBuf,
        parser_mode: crate::app::config::DocumentParserMode,
    },
    NaturalLanguageEdit {
        prompt: String,
        transactions: Vec<crate::engine::model::Transaction>,
    },
    CategorizeTransactions {
        transactions: Vec<crate::engine::model::Transaction>,
    },
    ApplyProposedChanges {
        input: PathBuf,
        output: PathBuf,
        changes: Vec<crate::engine::model::ProposedChange>,
    },
    GenerateVisualAlternatives {
        input: PathBuf,
        out_dir: PathBuf,
        page: usize,
        edits: Vec<crate::engine::workflow::UserEdit>,
        bbox: [f32; 4],
    },
    ExportChangeHistory {
        output: PathBuf,
    },
    LoadHistory {
        input: PathBuf,
    },
    Verify {
        original: PathBuf,
        edited: PathBuf,
        output_dir: PathBuf,
        intended_edits: Vec<crate::engine::verification::VerificationIntent>,
        use_pdfrest: bool,
        pdfrest_key: Option<String>,
        auto_match_dpi: bool,
    },
    ExplainImbalance {
        transactions_json: String,
        opening_balance: f64,
        closing_balance: f64,
        imbalance: f64,
    },

    /// Cancel a previously-enqueued job by its [`JobId`]. Best-effort; the
    /// task may have already finished. The runtime drops the token, so any
    /// `tokio::select!` watching `cancelled()` exits with a structured error.
    Cancel {
        id: JobId,
    },
    SubmitBugReport {
        description: String,
        include_logs: bool,
        include_audit: bool,
    },
    TypstReconstruct {
        input: std::path::PathBuf,
        output: std::path::PathBuf,
    },
    McpRenderPage {
        input: std::path::PathBuf,
        page: usize,
    },

    /// Hot-reload the runtime's `AppConfig` from the current process
    /// environment. The GUI sends this after the user updates API keys /
    /// credentials in-app (which write `.env` and `std::env::set_var`), so
    /// subsequent Document AI / Gemini jobs pick up the new values without an
    /// application restart.
    ReloadConfig,

    /// Trigger an active validation check on the AI credentials
    ValidateCredentials,

    /// Run the Smart Balance Engine and, when `auto_apply` is true, apply every
    /// proposed adjustment to the PDF in one shot (the "Adjust entire bank
    /// statement accordingly and apply all edits" button). When `auto_apply`
    /// is false this behaves like [`Job::BalanceStatement`].
    BalanceAndApplyAll {
        input: PathBuf,
        output: PathBuf,
        auto_apply: bool,
    },
    /// Cleanup orphaned temporary files from crash recovery
    CleanupTempFiles,

    // ----- Multi-stage workflow -------------------------------------------
    /// Stage 1: parse with Document AI then validate completeness with Gemini.
    WorkflowParseAndValidate {
        input: PathBuf,
        version: Option<String>,
        /// Which document parser the user selected in Backend Preferences.
        parser_mode: crate::app::config::DocumentParserMode,
        /// Which AI provider the user selected (used for completeness validation).
        ai_provider: crate::app::config::AiProviderMode,
        ignore_offline_fallback: bool,
    },
    /// Stage 3: build a balance preview from edits without writing the PDF.
    WorkflowPreview {
        original_transactions: Vec<crate::engine::model::Transaction>,
        edits: Vec<crate::engine::workflow::UserEdit>,
        opening_balance: rust_decimal::Decimal,
        expected_closing: Option<rust_decimal::Decimal>,
    },
    /// Stage 4 + 5 + 6: apply edits, render, validate visually in a loop, then
    /// re-parse with Document AI to confirm math.
    WorkflowConfirmAndRender {
        input: PathBuf,
        output: PathBuf,
        edits: Vec<crate::engine::workflow::UserEdit>,
        original_transactions: Vec<crate::engine::model::Transaction>,
        opening_balance: rust_decimal::Decimal,
        expected_closing: Option<rust_decimal::Decimal>,
        deep_font_replication: bool,
        max_visual_attempts: u32,
        visual_threshold: f64,
        ignore_font_coverage: bool,
        ignore_visual_fidelity: bool,
    },
    /// Use AI to fix text box issues and visual fidelity differences
    AiFixVisualFidelity {
        input: PathBuf,
        page: usize,
    },
    /// Transfer transactions from one bank statement PDF to another,
    /// adapting formats and verifying math + visual fidelity.
    TransferTransactions {
        source_pdf: PathBuf,
        target_pdf: PathBuf,
        output_pdf: PathBuf,
    },
    /// Bulk-shift or remap all transaction dates.
    AdjustDatePeriods {
        input: PathBuf,
        output: PathBuf,
        mode: crate::engine::date_adjust::DateAdjustMode,
    },
    /// User's response to an AI confirmation question.
    AiConfirmationResponse(crate::engine::ai_confirm::AiConfirmationResponse),
    InteractiveFallbackResponse(crate::engine::interactive_fallback::InteractiveFallbackResponse),
    /// Run cross-statement transfer tests on a set of PDFs.
    RunTransferTests {
        statements: Vec<PathBuf>,
        max_iterations: u32,
    },
    AiCommand {
        prompt: String,
        path: PathBuf,
    },

    // -- Document AI Version Management --
    /// Fetch list of available processor versions from the API.
    ListDocAiVersions,
    /// Deploy a specific processor version for inference.
    DeployDocAiVersion {
        version_id: String,
    },
    /// Undeploy a specific processor version.
    UndeployDocAiVersion {
        version_id: String,
    },
    /// Set a version as the default processor version.
    SetDefaultDocAiVersion {
        version_id: String,
    },
    /// Trigger training of a new custom processor version.
    TrainDocAiVersion {
        display_name: String,
        base_version: Option<String>,
    },
}

impl Job {
    pub fn is_fast(&self) -> bool {
        matches!(
            self,
            Job::Ping
                | Job::Undo
                | Job::Redo
                | Job::Cancel { .. }
                | Job::ReloadConfig
                | Job::CleanupTempFiles
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Job::McpRenderPage { .. } => "mcp_render_page",
            Self::Ping => "ping",
            Self::Python(..) => "python",
            Self::LoadDocument { .. } => "load_document",
            Self::AnalyzeFonts { .. } => "analyze_fonts",
            Self::RenderPage { .. } => "render_page",
            Self::ApplyChange { .. } => "apply_change",
            Self::CompleteFont { .. } => "complete_font",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::BalanceStatement { .. } => "balance_statement",
            Self::ExtractTransactions { .. } => "extract_transactions",
            Self::NaturalLanguageEdit { .. } => "natural_language_edit",
            Self::CategorizeTransactions { .. } => "categorize_transactions",
            Self::ApplyProposedChanges { .. } => "apply_proposed_changes",
            Self::GenerateVisualAlternatives { .. } => "generate_visual_alternatives",
            Self::ExportChangeHistory { .. } => "export_change_history",
            Self::LoadHistory { .. } => "load_history",
            Self::Verify { .. } => "verify",
            Self::Cancel { .. } => "cancel",
            Self::SubmitBugReport { .. } => "submit_bug_report",
            Self::TypstReconstruct { .. } => "typst_reconstruct",
            Self::ExplainImbalance { .. } => "explain_imbalance",
            Self::ReloadConfig => "reload_config",
            Self::ValidateCredentials => "validate_credentials",
            Self::BalanceAndApplyAll { .. } => "balance_and_apply_all",
            Self::CleanupTempFiles => "cleanup_temp_files",
            Self::WorkflowParseAndValidate { .. } => "workflow_parse_and_validate",
            Self::WorkflowPreview { .. } => "workflow_preview",
            Self::WorkflowConfirmAndRender { .. } => "workflow_confirm_and_render",
            Self::AiFixVisualFidelity { .. } => "ai_fix_visual_fidelity",
            Self::TransferTransactions { .. } => "transfer_transactions",
            Self::AdjustDatePeriods { .. } => "adjust_date_periods",
            Self::AiConfirmationResponse(_) => "ai_confirmation_response",
            Self::InteractiveFallbackResponse(_) => "interactive_fallback_response",
            Self::RunTransferTests { .. } => "run_transfer_tests",
            Self::AiCommand { .. } => "ai_command",
            Self::ListDocAiVersions => "list_docai_versions",
            Self::DeployDocAiVersion { .. } => "deploy_docai_version",
            Self::UndeployDocAiVersion { .. } => "undeploy_docai_version",
            Self::SetDefaultDocAiVersion { .. } => "set_default_docai_version",
            Self::TrainDocAiVersion { .. } => "train_docai_version",
            Self::UfoAutoEdit { .. } => "ufo_auto_edit",
            Self::CancelUfo => "cancel_ufo",
        }
    }

    pub(crate) fn document_path(&self) -> Option<&Path> {
        match self {
            Self::LoadDocument { path, .. }
            | Self::AnalyzeFonts { path }
            | Self::RenderPage { path, .. }
            | Self::CompleteFont { path, .. }
            | Self::BalanceStatement { path }
            | Self::ExtractTransactions { path, .. } => Some(path),
            Self::ApplyChange { input, .. }
            | Self::ApplyProposedChanges { input, .. }
            | Self::GenerateVisualAlternatives { input, .. }
            | Self::TypstReconstruct { input, .. }
            | Self::BalanceAndApplyAll { input, .. }
            | Self::WorkflowParseAndValidate { input, .. }
            | Self::WorkflowConfirmAndRender { input, .. }
            | Self::AiFixVisualFidelity { input, .. }
            | Self::AdjustDatePeriods { input, .. } => Some(input),
            Self::Verify { edited, .. } => Some(edited),
            Self::TransferTransactions { target_pdf, .. } => Some(target_pdf),
            Self::AiCommand { path, .. } => Some(path),
            _ => None,
        }
    }

    pub(crate) fn default_timeout(&self) -> std::time::Duration {
        use std::time::Duration;
        match self {
            Self::Ping
            | Self::Undo
            | Self::Redo
            | Self::Cancel { .. }
            | Self::ReloadConfig
            | Self::CleanupTempFiles => Duration::from_secs(300),
            Self::WorkflowParseAndValidate { .. }
            | Self::WorkflowConfirmAndRender { .. }
            | Self::TransferTransactions { .. }
            | Self::RunTransferTests { .. }
            | Self::Verify { .. }
            | Self::TypstReconstruct { .. } => Duration::from_secs(15 * 60),
            _ => Duration::from_secs(15 * 60),
        }
    }
}
