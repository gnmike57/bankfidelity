// Python operations run only through the supervised versioned worker process.
use crate::app::audit::AuditLog;
use crate::engine::history::{ChangeHistory, ChangeRecord};
use crate::engine::segments::{GlobalEdit, SegmentManager, SegmentMap};
use crate::pdf::engine::PdfEngine;
use crate::pdf::ReplaceOutcome;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

/// Opaque per-job handle. The runtime returns one when a job is enqueued;
/// callers can later `Job::Cancel` it.

use super::*;
use crate::app::runtime::client::*;
use crate::app::runtime::python_job::*;
use crate::app::runtime::tracking::*;
use crate::app::runtime::core::*;

pub type JobId = u64;

pub fn alloc_job_id() -> JobId {
    NEXT_JOB_ID.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone)]
pub struct JobMetadata {
    pub job_id: JobId,
    pub document_id: Option<String>,
    pub correlation_id: Uuid,
    pub label: &'static str,
    pub submitted_at: std::time::SystemTime,
    pub deadline: std::time::Instant,
    pub execution_mode: ExecutionMode,
}

impl JobMetadata {
    pub(crate) fn for_job(job: &Job) -> Self {
        Self::for_job_with_mode(job, ExecutionMode::Interactive)
    }

    fn for_job_with_mode(job: &Job, execution_mode: ExecutionMode) -> Self {
        Self {
            job_id: alloc_job_id(),
            document_id: job.document_path().map(document_id_for_path),
            correlation_id: Uuid::new_v4(),
            label: job.label(),
            submitted_at: std::time::SystemTime::now(),
            deadline: std::time::Instant::now() + job.default_timeout(),
            execution_mode,
        }
    }
}

pub(crate) struct JobEnvelope {
    pub(crate) metadata: JobMetadata,
    pub(crate) job: Job,
    pub(crate) route: Option<mpsc::Sender<JobResult>>,
}

impl JobEnvelope {
    pub(crate) fn broadcast(job: Job) -> Self {
        Self {
            metadata: JobMetadata::for_job(&job),
            job,
            route: None,
        }
    }

    pub(crate) fn broadcast_with_mode(job: Job, execution_mode: ExecutionMode) -> Self {
        Self {
            metadata: JobMetadata::for_job_with_mode(&job, execution_mode),
            job,
            route: None,
        }
    }

    pub(crate) fn routed_with_mode(
        job: Job,
        route: mpsc::Sender<JobResult>,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self {
            metadata: JobMetadata::for_job_with_mode(&job, execution_mode),
            job,
            route: Some(route),
        }
    }
}

pub struct JobTicket {
    pub(crate) metadata: JobMetadata,
    pub(crate) results: mpsc::Receiver<JobResult>,
    pub(crate) client: RuntimeClient,
}

impl JobTicket {
    pub fn metadata(&self) -> &JobMetadata {
        &self.metadata
    }

    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<JobResult, mpsc::RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }

    pub fn try_recv(&self) -> Result<JobResult, mpsc::TryRecvError> {
        self.results.try_recv()
    }

    pub fn cancel(&self) -> Result<JobId, RuntimeSubmitError> {
        self.client.send(Job::Cancel {
            id: self.metadata.job_id,
        })
    }
}

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

    fn document_path(&self) -> Option<&Path> {
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

    fn default_timeout(&self) -> std::time::Duration {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationDisposition {
    Succeeded,
    NoOp,
    Partial,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
pub enum JobResult {
    Pong,
    UfoAutoEditResult(serde_json::Value),
    UfoLog(String),
    ApiKeysVerified(crate::app::api_verification::VerificationReport),
    DocumentLoaded {
        layout_json: String,
        total_pages: usize,
    },
    PageRendered {
        png_bytes: Vec<u8>,
        page: usize,
        dpi: f32,
        tag: String,
        width_pts: f32,
        height_pts: f32,
    },
    ChangeApplied {
        record: ChangeRecord,
        requires_visual_review: bool,
    },
    HistoryUpdated {
        history: ChangeHistory,
    },
    FontCompleted(String),
    ChangeHistoryExported {
        path: PathBuf,
    },
    TransactionsExtracted(Vec<crate::engine::model::Transaction>),
    NaturalLanguageEditReady(Vec<crate::engine::model::Transaction>),
    CategorizationReady(Vec<crate::engine::model::Transaction>),
    VerificationReport(crate::engine::verification::VerificationReport),
    /// Stage 8.5: per-font usage and coverage breakdown for the loaded PDF.
    /// Sent automatically after `Job::LoadDocument` and on demand from
    /// `Job::AnalyzeFonts`.
    FontAnalysisReady(crate::engine::font_analysis::FontAnalysis),
    /// Stage 12 / Item #3: emitted when the workflow's font cascade was
    /// invoked because the apply step hit FONT_COVERAGE_INSUFFICIENT.
    /// The GUI uses this to surface a small audit line summarising which
    /// tiers were used and which characters each tier contributed.
    FontCascadeUsed(crate::engine::font_analysis::FontCascadeReport),
    BalanceProposed {
        imbalance: rust_decimal::Decimal,
        changes: Vec<crate::engine::model::ProposedChange>,
    },
    McpRenderComplete {
        base64_png: String,
    },
    ProposedChangesApplied {
        changes_applied: usize,
        failures: Vec<String>,
    },
    ImbalanceExplained {
        explanation: String,
    },
    /// Emitted after a [`Job::ReloadConfig`]: reports whether the reloaded
    /// config has working AI credentials so the GUI can update its status line.
    ConfigReloaded {
        generation: u64,
        config: std::sync::Arc<crate::app::config::AppConfig>,
        document_ai_configured: bool,
        gemini_configured: bool,
        pro_editing_available: bool,
    },
    Error {
        job_label: String,
        message: String,
    },
    NuclearFallbackRequired(String),
    Progress {
        label: String,
        fraction: f32,
    },
    /// A job tagged with this `JobId` was cancelled before it finished.
    Cancelled {
        id: JobId,
    },
    TimedOut {
        id: JobId,
        job_label: String,
    },
    ReconstructComplete {
        output_path: std::path::PathBuf,
    },
    BugReportSubmitted,

    // ----- Multi-stage workflow ------------------------------------------
    WorkflowStageChanged {
        stage: crate::engine::workflow::WorkflowStage,
    },
    WorkflowParseValidated {
        validation: crate::engine::workflow::ParseValidation,
        transactions: Vec<crate::engine::model::Transaction>,
    },
    WorkflowPreviewBuilt(crate::engine::workflow::BalancePreview),
    WorkflowVisualAttempt(crate::engine::workflow::VisualAttempt),
    VisualAlternativesReady(Vec<(String, Vec<u8>)>),
    WorkflowComplete(crate::engine::workflow::WorkflowOutcome),
    WorkflowFailed(crate::engine::workflow::WorkflowFailure),

    // ----- Transfer Transactions ------------------------------------------
    TransferComplete(crate::engine::transfer::TransferResult),
    TransferFailed {
        stage: String,
        message: String,
    },

    // ----- Date Adjustment -------------------------------------------------
    DatesAdjusted {
        records: Vec<crate::engine::date_adjust::DateShiftRecord>,
        output_path: PathBuf,
    },

    // ----- AI Confirmation -------------------------------------------------
    AiConfirmationNeeded(crate::engine::ai_confirm::AiConfirmation),
    InteractiveFallbackRequired(crate::engine::interactive_fallback::InteractiveFallbackRequest),

    // ----- Transfer Test Harness -------------------------------------------
    TransferTestsComplete(crate::engine::transfer_test_harness::TestHarnessReport),

    // ----- General Lifecycle -----------------------------------------------
    JobCompleted {
        job_label: String,
        disposition: OperationDisposition,
        artifact: Option<PathBuf>,
        message: String,
    },

    // ----- Document AI Version Management ----------------------------------
    DocAiVersionsListed(Vec<crate::ai::document_ai::ProcessorVersionInfo>),
    DocAiVersionOperationStarted {
        operation_name: String,
        description: String,
    },
    DocAiVersionError(String),
    WatchdogEvent(crate::app::watchdog::WatchdogEvent),
}

impl JobResult {
    /// True only for results that definitively end a tracked job lifecycle.
    /// Intermediate payloads must be enumerated by consumers, not inferred as
    /// terminal merely because they are not progress messages.
    pub fn disposition(&self) -> Option<OperationDisposition> {
        match self {
            Self::Error { .. }
            | Self::WorkflowFailed(_)
            | Self::TransferFailed { .. }
            | Self::DocAiVersionError(_)
            | Self::NuclearFallbackRequired(_) => Some(OperationDisposition::Failed),
            Self::Cancelled { .. } => Some(OperationDisposition::Cancelled),
            Self::TimedOut { .. } => Some(OperationDisposition::TimedOut),
            Self::WorkflowComplete(_)
            | Self::TransferComplete(_)
            | Self::Pong
            | Self::UfoAutoEditResult(_)
            | Self::McpRenderComplete { .. }
            | Self::ChangeApplied { .. }
            | Self::FontCompleted(_)
            | Self::ChangeHistoryExported { .. }
            | Self::TransactionsExtracted(_)
            | Self::NaturalLanguageEditReady(_)
            | Self::CategorizationReady(_)
            | Self::VerificationReport(_)
            | Self::BalanceProposed { .. }
            | Self::ProposedChangesApplied { .. }
            | Self::ConfigReloaded { .. }
            | Self::ImbalanceExplained { .. }
            | Self::ReconstructComplete { .. }
            | Self::BugReportSubmitted
            | Self::WorkflowPreviewBuilt(_)
            | Self::VisualAlternativesReady(_)
            | Self::TransferTestsComplete(_) => Some(OperationDisposition::Succeeded),
            Self::JobCompleted { disposition, .. } => Some(*disposition),
            _ => None,
        }
    }

    /// True only for results that definitively end a tracked job lifecycle
    /// from the runtime `TerminalTracker` perspective (strict).
    pub fn is_terminal(&self) -> bool {
        self.disposition().is_some()
    }

    /// True when this result should free one GUI `in_flight` wait slot.
    ///
    /// Broader than [`Self::is_terminal`]: many jobs complete with a success
    /// payload (e.g. `PageRendered`, `TransactionsExtracted`) that is not a
    /// `TerminalTracker` terminal event but still ends the user wait.
    /// Intermediate stream events (`Progress`, `UfoLog`, side-effect fonts)
    /// must return false.
    pub fn ends_gui_tracked_job(&self) -> bool {
        match self {
            // Intermediate / side-channel — never free a wait slot.
            Self::Progress { .. }
            | Self::UfoLog(_)
            | Self::WatchdogEvent(_)
            | Self::FontAnalysisReady(_)
            | Self::FontCascadeUsed(_)
            | Self::HistoryUpdated { .. }
            | Self::WorkflowStageChanged { .. }
            | Self::WorkflowVisualAttempt(_)
            | Self::AiConfirmationNeeded(_)
            | Self::InteractiveFallbackRequired(_)
            | Self::DocAiVersionsListed(_)
            | Self::DocAiVersionOperationStarted { .. }
            | Self::ApiKeysVerified(_)
            // DocumentLoaded auto-chains into parse; keep the same wait open.
            | Self::DocumentLoaded { .. } => false,

            // Failures and explicit terminals.
            Self::Error { .. }
            | Self::Cancelled { .. }
            | Self::TimedOut { .. }
            | Self::WorkflowFailed(_)
            | Self::TransferFailed { .. }
            | Self::JobCompleted { .. }
            | Self::NuclearFallbackRequired(_)
            // Success payloads that complete a user-dispatched job.
            | Self::Pong
            | Self::UfoAutoEditResult(_)
            | Self::McpRenderComplete { .. }
            | Self::PageRendered { .. }
            | Self::ChangeApplied { .. }
            | Self::FontCompleted(_)
            | Self::ChangeHistoryExported { .. }
            | Self::TransactionsExtracted(_)
            | Self::NaturalLanguageEditReady(_)
            | Self::CategorizationReady(_)
            | Self::VerificationReport(_)
            | Self::BalanceProposed { .. }
            | Self::ProposedChangesApplied { .. }
            | Self::ConfigReloaded { .. }
            | Self::ImbalanceExplained { .. }
            | Self::ReconstructComplete { .. }
            | Self::BugReportSubmitted
            | Self::WorkflowParseValidated { .. }
            | Self::WorkflowPreviewBuilt(_)
            | Self::VisualAlternativesReady(_)
            | Self::WorkflowComplete(_)
            | Self::TransferComplete(_)
            | Self::DatesAdjusted { .. }
            | Self::TransferTestsComplete(_)
            | Self::DocAiVersionError(_) => true,
        }
    }

    pub fn completed(
        job_label: impl Into<String>,
        disposition: OperationDisposition,
        artifact: Option<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self::JobCompleted {
            job_label: job_label.into(),
            disposition,
            artifact,
            message: message.into(),
        }
    }
}

