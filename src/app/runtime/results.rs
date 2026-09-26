use super::ids::JobId;
use crate::engine::history::{ChangeHistory, ChangeRecord};
use std::path::PathBuf;

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
    /// Useful payload of [`Job::ValidateCredentials`]: an intermediate
    /// report, NOT a terminal result. Under the shared completion contract
    /// the job sends this payload first and then closes with exactly one
    /// terminal [`JobResult::JobCompleted`] (`validate_credentials`).
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
    /// The shared terminal completion every consumer (GUI in-flight slots,
    /// `wait_for_terminal_result` / `wait_for_operation_completion` in the
    /// CLI, `collect_results` in the HTTP server, routed [`JobTicket`]s and
    /// the [`CancellationRegistry`]) keys off.
    ///
    /// Completion contract: a job emits its useful payloads first (e.g.
    /// [`Self::ApiKeysVerified`] for [`Job::ValidateCredentials`]) and then
    /// exactly one terminal result — this variant or an error /
    /// cancellation variant — so every consumer observes the same end of
    /// life. `Job::ValidateCredentials` and `Job::CleanupTempFiles` follow
    /// this contract with the `validate_credentials` / `cleanup_temp_files`
    /// labels.
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
