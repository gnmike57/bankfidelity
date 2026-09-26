#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Characterization tests for the runtime's `Job` / `JobResult` contract.
//!
//! These pin CURRENT behaviour (including oddities, flagged with `// NOTE:`)
//! so later refactors that move code out of `src/app/runtime.rs` cannot change
//! it silently.
//!
//! Enumeration technique: every table below is driven by an exhaustive `match`
//! with NO wildcard arm. Adding a new `Job` or `JobResult` variant is therefore
//! a COMPILE ERROR in this file until the new variant is pinned here. A
//! secondary test asserts the constructor list (`all_jobs` / `all_results`)
//! really contains one instance of every variant.
//!
//! Visibility limits (no `src/` changes allowed in this ticket):
//! `Job::document_path`, `Job::default_timeout`, `TerminalTracker::new` and
//! `ResultSink` are crate-private. `document_path` and `default_timeout` are
//! observed through the public `JobTicket::metadata()` (`document_id` /
//! `deadline`). The tracker is observed end-to-end through a real `Runtime`.
//!
//! Completion contract: a job emits its useful payloads first and then
//! exactly one terminal result. `Job::ValidateCredentials` and
//! `Job::CleanupTempFiles` used to finish without any terminal result, which
//! hung routed `JobTicket` waiters and leaked their `CancellationRegistry`
//! entries until shutdown timed out; the corrected lifecycle (payload(s)
//! first, then one terminal `JobCompleted`) is pinned end-to-end through a
//! real `Runtime` below.

use dual_core_pdf_pipeline::ai::document_ai::ProcessorVersionInfo;
use dual_core_pdf_pipeline::app::api_verification::VerificationReport as ApiVerificationReport;
use dual_core_pdf_pipeline::app::audit::AuditLog;
use dual_core_pdf_pipeline::app::config::{AiProviderMode, AppConfig, DocumentParserMode};
use dual_core_pdf_pipeline::app::runtime::{
    CancellationRegistry, Job, JobResult, JobTicket, OperationDisposition, PythonJob, Runtime,
    RuntimeClient,
};
use dual_core_pdf_pipeline::app::watchdog::WatchdogEvent;
use dual_core_pdf_pipeline::engine::ai_confirm::{AiConfirmation, AiConfirmationResponse};
use dual_core_pdf_pipeline::engine::date_adjust::DateAdjustMode;
use dual_core_pdf_pipeline::engine::font_analysis::{
    FontAnalysis, FontAnalysisSummary, FontCascadeReport,
};
use dual_core_pdf_pipeline::engine::history::{ChangeHistory, ChangeRecord};
use dual_core_pdf_pipeline::engine::interactive_fallback::{
    InteractiveFallbackRequest, InteractiveFallbackResponse,
};
use dual_core_pdf_pipeline::engine::transfer::TransferResult;
use dual_core_pdf_pipeline::engine::transfer_test_harness::TestHarnessReport;
use dual_core_pdf_pipeline::engine::verification::VerificationReport as EngineVerificationReport;
use dual_core_pdf_pipeline::engine::workflow::{
    BalancePreview, ParseValidation, VisualAttempt, WorkflowFailure, WorkflowOutcome, WorkflowStage,
};
use rust_decimal::Decimal;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

const JOB_VARIANT_COUNT: usize = 44;
const RESULT_VARIANT_COUNT: usize = 46;

// ---------------------------------------------------------------------------
// Job table
// ---------------------------------------------------------------------------

/// Distinct, non-existent relative path per role so that `document_path()`
/// field selection (input vs output, original vs edited, ...) is observable.
fn p(name: &str) -> PathBuf {
    PathBuf::from(format!("bf_contract_nonexistent/{name}.pdf"))
}

/// One fresh instance of every `Job` variant (`Job` is not `Clone`).
fn all_jobs() -> Vec<Job> {
    let (py_tx, _py_rx) = tokio::sync::oneshot::channel();
    vec![
        Job::Ping,
        Job::UfoAutoEdit {
            path: p("ufo_path"),
            context: String::new(),
        },
        Job::CancelUfo,
        Job::Python(PythonJob::Ping, py_tx),
        Job::LoadDocument {
            path: p("load_path"),
            three_page_mode: false,
        },
        Job::AnalyzeFonts {
            path: p("analyze_path"),
        },
        Job::RenderPage {
            path: p("render_path"),
            page: 0,
            dpi: 72.0,
            tag: String::new(),
        },
        Job::ApplyChange {
            input: p("apply_input"),
            output: p("apply_output"),
            page: 0,
            bbox: [0.0; 4],
            new_text: String::new(),
            old_text: String::new(),
            description: String::new(),
            deep_font_replication: false,
        },
        Job::CompleteFont {
            path: p("complete_font_path"),
            font_name: String::new(),
        },
        Job::Undo,
        Job::Redo,
        Job::BalanceStatement {
            path: p("balance_path"),
        },
        Job::ExtractTransactions {
            path: p("extract_path"),
            parser_mode: DocumentParserMode::OfflineHeuristic,
        },
        Job::NaturalLanguageEdit {
            prompt: String::new(),
            transactions: Vec::new(),
        },
        Job::CategorizeTransactions {
            transactions: Vec::new(),
        },
        Job::ApplyProposedChanges {
            input: p("proposed_input"),
            output: p("proposed_output"),
            changes: Vec::new(),
        },
        Job::GenerateVisualAlternatives {
            input: p("visalt_input"),
            out_dir: p("visalt_out_dir"),
            page: 0,
            edits: Vec::new(),
            bbox: [0.0; 4],
        },
        Job::ExportChangeHistory {
            output: p("export_output"),
        },
        Job::LoadHistory {
            input: p("load_history_input"),
        },
        Job::Verify {
            original: p("verify_original"),
            edited: p("verify_edited"),
            output_dir: p("verify_output_dir"),
            intended_edits: Vec::new(),
            use_pdfrest: false,
            pdfrest_key: None,
            auto_match_dpi: false,
        },
        Job::ExplainImbalance {
            transactions_json: String::new(),
            opening_balance: 0.0,
            closing_balance: 0.0,
            imbalance: 0.0,
        },
        Job::Cancel { id: 0 },
        Job::SubmitBugReport {
            description: String::new(),
            include_logs: false,
            include_audit: false,
        },
        Job::TypstReconstruct {
            input: p("typst_input"),
            output: p("typst_output"),
        },
        Job::McpRenderPage {
            input: p("mcp_input"),
            page: 0,
        },
        Job::ReloadConfig,
        Job::ValidateCredentials,
        Job::BalanceAndApplyAll {
            input: p("bal_all_input"),
            output: p("bal_all_output"),
            auto_apply: false,
        },
        Job::CleanupTempFiles,
        Job::WorkflowParseAndValidate {
            input: p("wf_parse_input"),
            version: None,
            parser_mode: DocumentParserMode::OfflineHeuristic,
            ai_provider: AiProviderMode::ManualOnly,
            ignore_offline_fallback: false,
        },
        Job::WorkflowPreview {
            original_transactions: Vec::new(),
            edits: Vec::new(),
            opening_balance: Decimal::ZERO,
            expected_closing: None,
        },
        Job::WorkflowConfirmAndRender {
            input: p("wf_confirm_input"),
            output: p("wf_confirm_output"),
            edits: Vec::new(),
            original_transactions: Vec::new(),
            opening_balance: Decimal::ZERO,
            expected_closing: None,
            deep_font_replication: false,
            max_visual_attempts: 1,
            visual_threshold: 0.02,
            ignore_font_coverage: false,
            ignore_visual_fidelity: false,
        },
        Job::AiFixVisualFidelity {
            input: p("ai_fix_input"),
            page: 0,
        },
        Job::TransferTransactions {
            source_pdf: p("transfer_source"),
            target_pdf: p("transfer_target"),
            output_pdf: p("transfer_output"),
        },
        Job::AdjustDatePeriods {
            input: p("dates_input"),
            output: p("dates_output"),
            mode: DateAdjustMode::ShiftDays(0),
        },
        Job::AiConfirmationResponse(AiConfirmationResponse {
            id: Uuid::nil(),
            selected_option: 0,
            user_note: None,
        }),
        Job::InteractiveFallbackResponse(InteractiveFallbackResponse {
            id: Uuid::nil(),
            selected_alternative_id: String::new(),
        }),
        Job::RunTransferTests {
            statements: vec![p("transfer_tests_statement")],
            max_iterations: 1,
        },
        Job::AiCommand {
            prompt: String::new(),
            path: p("ai_command_path"),
        },
        Job::ListDocAiVersions,
        Job::DeployDocAiVersion {
            version_id: String::new(),
        },
        Job::UndeployDocAiVersion {
            version_id: String::new(),
        },
        Job::SetDefaultDocAiVersion {
            version_id: String::new(),
        },
        Job::TrainDocAiVersion {
            display_name: String::new(),
            base_version: None,
        },
    ]
}

struct JobPin<'a> {
    /// Unique index; proves `all_jobs()` holds exactly one of each variant.
    idx: usize,
    label: &'static str,
    is_fast: bool,
    timeout_secs: u64,
    /// Which path field `document_path()` selects (borrowed from the job).
    document_path: Option<&'a Path>,
}

const FAST: u64 = 300;
const SLOW: u64 = 15 * 60;

/// Exhaustive, wildcard-free pin of `label`, `is_fast`, `default_timeout`
/// and `document_path` for every `Job` variant.
fn pin_job(job: &Job) -> JobPin<'_> {
    let (idx, label, is_fast, timeout_secs, document_path): (
        usize,
        &'static str,
        bool,
        u64,
        Option<&Path>,
    ) = match job {
        Job::Ping => (0, "ping", true, FAST, None),
        Job::UfoAutoEdit { .. } => (1, "ufo_auto_edit", false, SLOW, None),
        Job::CancelUfo => (2, "cancel_ufo", false, SLOW, None),
        Job::Python(..) => (3, "python", false, SLOW, None),
        Job::LoadDocument { path, .. } => (4, "load_document", false, SLOW, Some(path)),
        Job::AnalyzeFonts { path } => (5, "analyze_fonts", false, SLOW, Some(path)),
        Job::RenderPage { path, .. } => (6, "render_page", false, SLOW, Some(path)),
        Job::ApplyChange { input, .. } => (7, "apply_change", false, SLOW, Some(input)),
        Job::CompleteFont { path, .. } => (8, "complete_font", false, SLOW, Some(path)),
        Job::Undo => (9, "undo", true, FAST, None),
        Job::Redo => (10, "redo", true, FAST, None),
        Job::BalanceStatement { path } => (11, "balance_statement", false, SLOW, Some(path)),
        Job::ExtractTransactions { path, .. } => {
            (12, "extract_transactions", false, SLOW, Some(path))
        }
        Job::NaturalLanguageEdit { .. } => (13, "natural_language_edit", false, SLOW, None),
        Job::CategorizeTransactions { .. } => (14, "categorize_transactions", false, SLOW, None),
        Job::ApplyProposedChanges { input, .. } => {
            (15, "apply_proposed_changes", false, SLOW, Some(input))
        }
        Job::GenerateVisualAlternatives { input, .. } => {
            (16, "generate_visual_alternatives", false, SLOW, Some(input))
        }
        Job::ExportChangeHistory { .. } => (17, "export_change_history", false, SLOW, None),
        Job::LoadHistory { .. } => (18, "load_history", false, SLOW, None),
        // NOTE: Verify keys the document id off `edited`, not `original`.
        Job::Verify { edited, .. } => (19, "verify", false, SLOW, Some(edited)),
        Job::ExplainImbalance { .. } => (20, "explain_imbalance", false, SLOW, None),
        Job::Cancel { .. } => (21, "cancel", true, FAST, None),
        Job::SubmitBugReport { .. } => (22, "submit_bug_report", false, SLOW, None),
        Job::TypstReconstruct { input, .. } => (23, "typst_reconstruct", false, SLOW, Some(input)),
        // NOTE: McpRenderPage has an `input` PDF but document_path() returns
        // None (falls into the `_` arm), so it gets no document_id.
        Job::McpRenderPage { .. } => (24, "mcp_render_page", false, SLOW, None),
        Job::ReloadConfig => (25, "reload_config", true, FAST, None),
        Job::ValidateCredentials => (26, "validate_credentials", false, SLOW, None),
        Job::BalanceAndApplyAll { input, .. } => {
            (27, "balance_and_apply_all", false, SLOW, Some(input))
        }
        Job::CleanupTempFiles => (28, "cleanup_temp_files", true, FAST, None),
        Job::WorkflowParseAndValidate { input, .. } => {
            (29, "workflow_parse_and_validate", false, SLOW, Some(input))
        }
        Job::WorkflowPreview { .. } => (30, "workflow_preview", false, SLOW, None),
        Job::WorkflowConfirmAndRender { input, .. } => {
            (31, "workflow_confirm_and_render", false, SLOW, Some(input))
        }
        Job::AiFixVisualFidelity { input, .. } => {
            (32, "ai_fix_visual_fidelity", false, SLOW, Some(input))
        }
        // NOTE: TransferTransactions keys off the TARGET pdf.
        Job::TransferTransactions { target_pdf, .. } => {
            (33, "transfer_transactions", false, SLOW, Some(target_pdf))
        }
        Job::AdjustDatePeriods { input, .. } => {
            (34, "adjust_date_periods", false, SLOW, Some(input))
        }
        Job::AiConfirmationResponse(_) => (35, "ai_confirmation_response", false, SLOW, None),
        Job::InteractiveFallbackResponse(_) => {
            (36, "interactive_fallback_response", false, SLOW, None)
        }
        Job::RunTransferTests { .. } => (37, "run_transfer_tests", false, SLOW, None),
        Job::AiCommand { path, .. } => (38, "ai_command", false, SLOW, Some(path)),
        Job::ListDocAiVersions => (39, "list_docai_versions", false, SLOW, None),
        Job::DeployDocAiVersion { .. } => (40, "deploy_docai_version", false, SLOW, None),
        Job::UndeployDocAiVersion { .. } => (41, "undeploy_docai_version", false, SLOW, None),
        Job::SetDefaultDocAiVersion { .. } => (42, "set_default_docai_version", false, SLOW, None),
        Job::TrainDocAiVersion { .. } => (43, "train_docai_version", false, SLOW, None),
    };
    JobPin {
        idx,
        label,
        is_fast,
        timeout_secs,
        document_path,
    }
}

/// Mirror of the runtime's private `document_id_for_path` for non-existent
/// relative paths (canonicalize fails, so the raw path is hashed).
fn expected_document_id(path: &Path) -> String {
    use sha2::Digest;
    let normalized = path.to_string_lossy().replace('\\', "/");
    sha2::Sha256::digest(normalized.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn job_table_covers_every_variant_exactly_once() {
    let jobs = all_jobs();
    assert_eq!(jobs.len(), JOB_VARIANT_COUNT);
    let indices: BTreeSet<usize> = jobs.iter().map(|j| pin_job(j).idx).collect();
    assert_eq!(
        indices.len(),
        JOB_VARIANT_COUNT,
        "duplicate/missing Job pin"
    );
    assert_eq!(
        indices,
        (0..JOB_VARIANT_COUNT).collect::<BTreeSet<_>>(),
        "Job pin indices must be contiguous 0..N"
    );
}

#[test]
fn job_label_and_is_fast_are_pinned_for_all_variants() {
    let mut labels = BTreeSet::new();
    for job in all_jobs() {
        let pin = pin_job(&job);
        assert_eq!(job.label(), pin.label, "label() drifted for {job:?}");
        assert_eq!(job.is_fast(), pin.is_fast, "is_fast() drifted for {job:?}");
        assert!(labels.insert(pin.label), "duplicate label {}", pin.label);
    }
    assert_eq!(labels.len(), JOB_VARIANT_COUNT);
    let fast: BTreeSet<&str> = all_jobs()
        .iter()
        .filter(|j| j.is_fast())
        .map(|j| j.label())
        .collect();
    assert_eq!(
        fast,
        BTreeSet::from([
            "ping",
            "undo",
            "redo",
            "cancel",
            "reload_config",
            "cleanup_temp_files"
        ])
    );
}

/// `default_timeout()` and `document_path()` are crate-private; the runtime
/// consumes them in `JobMetadata::for_job_with_mode`, which is observable via
/// `JobTicket::metadata()` (`deadline`, `document_id`, `label`).
#[test]
fn job_default_timeout_and_document_path_pinned_via_ticket_metadata() {
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let client = RuntimeClient::from(job_tx);
    let mut seen_doc_ids = 0usize;

    for job in all_jobs() {
        let pin_idx;
        let (label, timeout, doc) = {
            let pin = pin_job(&job);
            pin_idx = pin.idx;
            (
                pin.label,
                Duration::from_secs(pin.timeout_secs),
                pin.document_path.map(expected_document_id),
            )
        };
        let before = Instant::now();
        let ticket = client.submit(job).expect("submit");
        let meta = ticket.metadata();

        assert_eq!(meta.label, label, "metadata label, variant {pin_idx}");
        assert_eq!(meta.document_id, doc, "document_path() drifted for {label}");
        if doc.is_some() {
            seen_doc_ids += 1;
        }
        let window = meta.deadline.saturating_duration_since(before);
        assert!(
            window >= timeout && window < timeout + Duration::from_secs(30),
            "default_timeout() drifted for {label}: got {window:?}, want ~{timeout:?}"
        );
    }
    // Pinned count of variants that yield Some(document_path()).
    assert_eq!(seen_doc_ids, 18);

    // Drain the forwarding thread so every job was really handed over.
    let mut forwarded = 0;
    while job_rx.recv_timeout(Duration::from_secs(2)).is_ok() {
        forwarded += 1;
        if forwarded == JOB_VARIANT_COUNT {
            break;
        }
    }
    assert_eq!(forwarded, JOB_VARIANT_COUNT);
}

// ---------------------------------------------------------------------------
// JobResult table
// ---------------------------------------------------------------------------

fn all_results() -> Vec<JobResult> {
    vec![
        JobResult::Pong,
        JobResult::UfoAutoEditResult(serde_json::Value::Null),
        JobResult::UfoLog(String::new()),
        JobResult::ApiKeysVerified(ApiVerificationReport::new(Vec::new())),
        JobResult::DocumentLoaded {
            layout_json: String::new(),
            total_pages: 0,
        },
        JobResult::PageRendered {
            png_bytes: Vec::new(),
            page: 0,
            dpi: 72.0,
            tag: String::new(),
            width_pts: 612.0,
            height_pts: 792.0,
        },
        JobResult::ChangeApplied {
            record: ChangeRecord {
                id: 1,
                timestamp: String::new(),
                page: 0,
                old_text: String::new(),
                new_text: String::new(),
                bbox: [0.0; 4],
                description: String::new(),
                snapshot_path: None,
                snapshot_evidence: None,
                provenance: String::new(),
                obj_id: None,
            },
            requires_visual_review: false,
        },
        JobResult::HistoryUpdated {
            history: ChangeHistory::new(),
        },
        JobResult::FontCompleted(String::new()),
        JobResult::ChangeHistoryExported { path: p("export") },
        JobResult::TransactionsExtracted(Vec::new()),
        JobResult::NaturalLanguageEditReady(Vec::new()),
        JobResult::CategorizationReady(Vec::new()),
        JobResult::VerificationReport(EngineVerificationReport {
            math_valid: true,
            visual_diff_score: 0.0,
            only_intended_changes: true,
            report_files: Vec::new(),
            message: String::new(),
            max_tile_score: 0.0,
            max_edit_region_score: 0.0,
            min_ssim: 1.0,
            gates: Vec::new(),
        }),
        JobResult::FontAnalysisReady(FontAnalysis {
            fonts: Vec::new(),
            summary: FontAnalysisSummary {
                total_fonts: 0,
                fonts_needing_action: 0,
                missing_digit_count: 0,
                missing_letter_count: 0,
                missing_other_count: 0,
                all_fonts_covered: true,
            },
        }),
        JobResult::FontCascadeUsed(FontCascadeReport {
            success: false,
            original_font: String::new(),
            extended_font_path: None,
            tiers_used: Vec::new(),
            synthesised: Vec::new(),
            donor_extended: Vec::new(),
            ai_extended: Vec::new(),
            still_missing: Vec::new(),
            workflow_attempt: 0,
        }),
        JobResult::BalanceProposed {
            imbalance: Decimal::ZERO,
            changes: Vec::new(),
        },
        JobResult::McpRenderComplete {
            base64_png: String::new(),
        },
        JobResult::ProposedChangesApplied {
            changes_applied: 0,
            failures: Vec::new(),
        },
        JobResult::ImbalanceExplained {
            explanation: String::new(),
        },
        JobResult::ConfigReloaded {
            generation: 0,
            config: Arc::new(AppConfig::default()),
            document_ai_configured: false,
            gemini_configured: false,
            pro_editing_available: false,
        },
        JobResult::Error {
            job_label: String::new(),
            message: String::new(),
        },
        JobResult::NuclearFallbackRequired(String::new()),
        JobResult::Progress {
            label: String::new(),
            fraction: 0.0,
        },
        JobResult::Cancelled { id: 0 },
        JobResult::TimedOut {
            id: 0,
            job_label: String::new(),
        },
        JobResult::ReconstructComplete {
            output_path: p("reconstruct"),
        },
        JobResult::BugReportSubmitted,
        JobResult::WorkflowStageChanged {
            stage: WorkflowStage::Idle,
        },
        JobResult::WorkflowParseValidated {
            validation: ParseValidation {
                total_pages: 0,
                transactions_found: 0,
                opening_balance: Decimal::ZERO,
                closing_balance: Decimal::ZERO,
                account_number: None,
                completeness_score: 1.0,
                completeness_notes: String::new(),
                missing_rows: Vec::new(),
            },
            transactions: Vec::new(),
        },
        JobResult::WorkflowPreviewBuilt(BalancePreview::default()),
        JobResult::WorkflowVisualAttempt(VisualAttempt {
            attempt: 1,
            max_attempts: 3,
            diff_score: 0.01,
            threshold: 0.02,
            only_intended: true,
            message: String::new(),
        }),
        JobResult::VisualAlternativesReady(Vec::new()),
        JobResult::WorkflowComplete(WorkflowOutcome {
            final_pdf: p("workflow_final"),
            transactions_re_parsed: 0,
            final_imbalance: Decimal::ZERO,
            math_valid: true,
            visual_attempts: 0,
            completion_summary: String::new(),
        }),
        JobResult::WorkflowFailed(WorkflowFailure::ParseFailed(String::new())),
        JobResult::TransferComplete(TransferResult {
            output_path: p("transfer_out"),
            source_tx_count: 0,
            target_tx_count: 0,
            pages_added: 0,
            pages_removed: 0,
            math_verified: true,
            visual_verified: true,
            visual_score: 0.0,
            math_imbalance: Decimal::ZERO,
            stages_completed: 0,
            total_duration_secs: 0.0,
            corrections_applied: 0,
            retries_attempted: 0,
            synthesized_fonts_used: false,
            visual_proof_path: None,
        }),
        JobResult::TransferFailed {
            stage: String::new(),
            message: String::new(),
        },
        JobResult::DatesAdjusted {
            records: Vec::new(),
            output_path: p("dates_out"),
        },
        JobResult::AiConfirmationNeeded(AiConfirmation {
            id: Uuid::nil(),
            stage: String::new(),
            question: String::new(),
            options: Vec::new(),
            context: String::new(),
            confidence: 0.5,
            default_answer: None,
        }),
        JobResult::InteractiveFallbackRequired(InteractiveFallbackRequest {
            id: Uuid::nil(),
            stage: String::new(),
            error_details: String::new(),
            alternatives: Vec::new(),
        }),
        JobResult::TransferTestsComplete(TestHarnessReport {
            timestamp: String::new(),
            statement_count: 0,
            total_pairs: 0,
            passed: 0,
            failed: 0,
            results: Vec::new(),
            total_duration_secs: 0.0,
        }),
        JobResult::JobCompleted {
            job_label: String::new(),
            disposition: OperationDisposition::NoOp,
            artifact: None,
            message: String::new(),
        },
        JobResult::DocAiVersionsListed(Vec::<ProcessorVersionInfo>::new()),
        JobResult::DocAiVersionOperationStarted {
            operation_name: String::new(),
            description: String::new(),
        },
        JobResult::DocAiVersionError(String::new()),
        JobResult::WatchdogEvent(WatchdogEvent::Recovered),
    ]
}

struct ResultPin {
    idx: usize,
    disposition: Option<OperationDisposition>,
    ends_gui_tracked_job: bool,
}

/// Exhaustive, wildcard-free pin of `disposition` and `ends_gui_tracked_job`
/// for every `JobResult` variant. `is_terminal()` is pinned as
/// `disposition.is_some()` by the test (and cross-checked independently).
fn pin_result(result: &JobResult) -> ResultPin {
    use OperationDisposition::{Cancelled, Failed, Succeeded};
    let (idx, disposition, ends_gui_tracked_job): (usize, Option<OperationDisposition>, bool) =
        match result {
            JobResult::Pong => (0, Some(Succeeded), true),
            JobResult::UfoAutoEditResult(_) => (1, Some(Succeeded), true),
            JobResult::UfoLog(_) => (2, None, false),
            // NOTE: `ApiKeysVerified` is the useful payload of
            // `Job::ValidateCredentials`, not a terminal: it keeps
            // `disposition() == None` and does not free a GUI wait slot. The
            // job closes with exactly one terminal
            // `JobCompleted { job_label: "validate_credentials" }` after it,
            // pinned by `validate_credentials_emits_payload_then_single_terminal_completion`.
            JobResult::ApiKeysVerified(_) => (3, None, false),
            JobResult::DocumentLoaded { .. } => (4, None, false),
            // NOTE: PageRendered ends the GUI wait but is NOT a terminal
            // (no disposition), so TerminalTracker never treats it as final.
            JobResult::PageRendered { .. } => (5, None, true),
            JobResult::ChangeApplied { .. } => (6, Some(Succeeded), true),
            JobResult::HistoryUpdated { .. } => (7, None, false),
            JobResult::FontCompleted(_) => (8, Some(Succeeded), true),
            JobResult::ChangeHistoryExported { .. } => (9, Some(Succeeded), true),
            JobResult::TransactionsExtracted(_) => (10, Some(Succeeded), true),
            JobResult::NaturalLanguageEditReady(_) => (11, Some(Succeeded), true),
            JobResult::CategorizationReady(_) => (12, Some(Succeeded), true),
            JobResult::VerificationReport(_) => (13, Some(Succeeded), true),
            JobResult::FontAnalysisReady(_) => (14, None, false),
            JobResult::FontCascadeUsed(_) => (15, None, false),
            JobResult::BalanceProposed { .. } => (16, Some(Succeeded), true),
            JobResult::McpRenderComplete { .. } => (17, Some(Succeeded), true),
            JobResult::ProposedChangesApplied { .. } => (18, Some(Succeeded), true),
            JobResult::ImbalanceExplained { .. } => (19, Some(Succeeded), true),
            JobResult::ConfigReloaded { .. } => (20, Some(Succeeded), true),
            JobResult::Error { .. } => (21, Some(Failed), true),
            JobResult::NuclearFallbackRequired(_) => (22, Some(Failed), true),
            JobResult::Progress { .. } => (23, None, false),
            JobResult::Cancelled { .. } => (24, Some(Cancelled), true),
            JobResult::TimedOut { .. } => (25, Some(OperationDisposition::TimedOut), true),
            JobResult::ReconstructComplete { .. } => (26, Some(Succeeded), true),
            JobResult::BugReportSubmitted => (27, Some(Succeeded), true),
            JobResult::WorkflowStageChanged { .. } => (28, None, false),
            // NOTE: WorkflowParseValidated frees the GUI slot yet is not a
            // TerminalTracker terminal.
            JobResult::WorkflowParseValidated { .. } => (29, None, true),
            JobResult::WorkflowPreviewBuilt(_) => (30, Some(Succeeded), true),
            JobResult::WorkflowVisualAttempt(_) => (31, None, false),
            JobResult::VisualAlternativesReady(_) => (32, Some(Succeeded), true),
            JobResult::WorkflowComplete(_) => (33, Some(Succeeded), true),
            JobResult::WorkflowFailed(_) => (34, Some(Failed), true),
            JobResult::TransferComplete(_) => (35, Some(Succeeded), true),
            JobResult::TransferFailed { .. } => (36, Some(Failed), true),
            // NOTE: DatesAdjusted is a success payload but has no disposition
            // (not terminal) while still ending the GUI wait.
            JobResult::DatesAdjusted { .. } => (37, None, true),
            JobResult::AiConfirmationNeeded(_) => (38, None, false),
            JobResult::InteractiveFallbackRequired(_) => (39, None, false),
            JobResult::TransferTestsComplete(_) => (40, Some(Succeeded), true),
            // NOTE: disposition is payload-driven; the fixture uses NoOp.
            JobResult::JobCompleted { disposition, .. } => (41, Some(*disposition), true),
            JobResult::DocAiVersionsListed(_) => (42, None, false),
            JobResult::DocAiVersionOperationStarted { .. } => (43, None, false),
            // NOTE: DocAiVersionError is Failed/terminal.
            JobResult::DocAiVersionError(_) => (44, Some(Failed), true),
            JobResult::WatchdogEvent(_) => (45, None, false),
        };
    ResultPin {
        idx,
        disposition,
        ends_gui_tracked_job,
    }
}

#[test]
fn job_result_table_covers_every_variant_exactly_once() {
    let results = all_results();
    assert_eq!(results.len(), RESULT_VARIANT_COUNT);
    let indices: BTreeSet<usize> = results.iter().map(|r| pin_result(r).idx).collect();
    assert_eq!(
        indices,
        (0..RESULT_VARIANT_COUNT).collect::<BTreeSet<_>>(),
        "JobResult pin indices must be contiguous 0..N"
    );
}

#[test]
fn job_result_disposition_terminal_and_gui_flags_are_pinned_for_all_variants() {
    for result in all_results() {
        let pin = pin_result(&result);
        assert_eq!(
            result.disposition(),
            pin.disposition,
            "disposition() drifted for {result:?}"
        );
        assert_eq!(
            result.is_terminal(),
            pin.disposition.is_some(),
            "is_terminal() drifted for {result:?}"
        );
        assert_eq!(
            result.ends_gui_tracked_job(),
            pin.ends_gui_tracked_job,
            "ends_gui_tracked_job() drifted for {result:?}"
        );
        // Contract: strict terminal is a subset of GUI-ending results.
        if result.is_terminal() {
            assert!(result.ends_gui_tracked_job(), "{result:?}");
        }
    }
}

#[test]
fn job_completed_disposition_is_payload_driven_for_every_disposition() {
    use OperationDisposition::*;
    for d in [Succeeded, NoOp, Partial, Failed, Cancelled, TimedOut] {
        let r = JobResult::completed("x", d, None, "m");
        assert_eq!(r.disposition(), Some(d));
        assert!(r.is_terminal());
        assert!(r.ends_gui_tracked_job());
    }
}

// ---------------------------------------------------------------------------
// CancellationRegistry lifecycle
// ---------------------------------------------------------------------------

#[test]
fn cancellation_registry_register_cancel_complete_wait_until_empty() {
    let reg = CancellationRegistry::new();
    assert!(reg.is_empty());
    assert!(reg.wait_until_empty(Duration::from_millis(10)));

    let t1 = reg.register(1001);
    let t2 = reg.register(1002);
    assert_eq!(reg.len(), 2);
    assert!(!t1.is_cancelled());
    assert!(!t2.is_cancelled());
    assert!(!reg.wait_until_empty(Duration::from_millis(30)));

    // cancel: token cancelled, entry removed, second cancel is a no-op.
    assert!(reg.cancel(1001));
    assert!(t1.is_cancelled());
    assert_eq!(reg.len(), 1);
    assert!(!reg.cancel(1001));
    assert!(!reg.cancel(9999));

    // complete: entry removed WITHOUT cancelling the token; unknown id no-op.
    reg.complete(9999);
    assert_eq!(reg.len(), 1);
    reg.complete(1002);
    assert!(!t2.is_cancelled());
    assert!(reg.is_empty());
    assert!(reg.wait_until_empty(Duration::from_millis(10)));
}

#[test]
fn cancellation_registry_wait_is_woken_by_completion_from_another_thread() {
    let reg = CancellationRegistry::new();
    let _t = reg.register(2001);
    let reg2 = reg.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        reg2.complete(2001);
    });
    assert!(reg.wait_until_empty(Duration::from_secs(10)));
    handle.join().unwrap();
}

#[test]
fn cancellation_registry_request_cancel_all_retains_entries_but_cancel_all_drains() {
    let reg = CancellationRegistry::new();
    let a = reg.register(3001);
    let b = reg.register(3002);

    reg.request_cancel_all();
    assert!(a.is_cancelled() && b.is_cancelled());
    // NOTE: entries are retained until a terminal result confirms completion.
    assert_eq!(reg.len(), 2);
    assert!(!reg.wait_until_empty(Duration::from_millis(30)));

    reg.cancel_all();
    assert!(reg.is_empty());
    assert!(reg.wait_until_empty(Duration::from_millis(10)));
}

// ---------------------------------------------------------------------------
// Exactly-once terminal emission, observed through a real Runtime
// ---------------------------------------------------------------------------
//
// NOTE: `TerminalTracker::new` / `ResultSink::new` are crate-private, so the
// drop-path (synthetic `JobResult::Error` "panicked or exited silently") and
// direct duplicate-terminal suppression cannot be constructed from an
// integration test without touching `src/`. They stay covered by the
// in-module tests `terminal_tracker_*` in `src/app/runtime.rs`; the test below
// pins the externally observable consequence: one terminal per job.
//
// NOTE: `Runtime::start` broadcasts the startup `Job::CleanupTempFiles`
// terminal on this shared channel. It is background (not routed through a
// `ResultSink`) and the CLI's `wait_for_terminal_result` skips it, so this
// test skips it too when counting a tracked job's terminals.

/// True when `result` is the startup `Job::CleanupTempFiles` terminal
/// broadcast, which must not count against another job's terminal budget.
fn is_background_cleanup_terminal(result: &JobResult) -> bool {
    matches!(
        result,
        JobResult::JobCompleted { job_label, .. } if job_label == "cleanup_temp_files"
    )
}

#[test]
fn runtime_emits_exactly_one_terminal_result_per_tracked_job() {
    let dir = tempfile::tempdir().unwrap();
    let audit = AuditLog::open(dir.path()).unwrap();
    let mut cfg_val = AppConfig::default();
    cfg_val.passphrase = "contract-passphrase-1234567890".into();
    cfg_val.log_dir = dir.path().join("logs");
    let (_runtime, job_tx, job_rx) = Runtime::start(audit, Arc::new(cfg_val));

    // BalanceAndApplyAll is wrapped in a TerminalTracker inside the runtime.
    job_tx
        .send(Job::BalanceAndApplyAll {
            input: dir.path().join("does_not_exist.pdf"),
            output: dir.path().join("out.pdf"),
            auto_apply: false,
        })
        .unwrap();

    let mut terminals = Vec::new();
    let first_deadline = Instant::now() + Duration::from_secs(60);
    while terminals.is_empty() && Instant::now() < first_deadline {
        if let Ok(r) = job_rx.recv_timeout(Duration::from_millis(200)) {
            // Skip the startup cleanup terminal (background, untracked).
            if is_background_cleanup_terminal(&r) {
                continue;
            }
            if r.is_terminal() {
                terminals.push(r);
            }
        }
    }
    assert_eq!(terminals.len(), 1, "expected a terminal result");
    // Grace window: any duplicate/late terminal would show up here.
    let grace = Instant::now() + Duration::from_secs(2);
    while Instant::now() < grace {
        if let Ok(r) = job_rx.recv_timeout(Duration::from_millis(100)) {
            if is_background_cleanup_terminal(&r) {
                continue;
            }
            assert!(
                !r.is_terminal(),
                "second terminal emitted after the first: {r:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Corrected lifecycle: useful payload(s), then exactly one terminal
// ---------------------------------------------------------------------------
//
// `Job::ValidateCredentials` and `Job::CleanupTempFiles` historically did
// their useful work and finished without ever emitting a terminal result, so
// routed `JobTicket` waiters hung and their `CancellationRegistry` entries
// stayed registered until graceful shutdown timed out. The tests below pin
// the corrected contract end-to-end through a real `Runtime`: payload(s)
// first, then exactly one terminal `JobCompleted`, with no second terminal.

/// Drain a routed ticket until its terminal result (or `timeout`).
/// Returns `(non_terminal_payloads, terminal_results)`. After the first
/// terminal a grace window runs so a duplicate/late terminal is caught.
fn drain_routed_ticket(ticket: &JobTicket, timeout: Duration) -> (Vec<JobResult>, Vec<JobResult>) {
    let deadline = Instant::now() + timeout;
    let mut payloads = Vec::new();
    let mut terminals = Vec::new();
    while Instant::now() < deadline && terminals.is_empty() {
        match ticket.recv_timeout(Duration::from_millis(200)) {
            Ok(result) if result.is_terminal() => terminals.push(result),
            Ok(result) => payloads.push(result),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let grace = Instant::now() + Duration::from_secs(2);
    while Instant::now() < grace {
        match ticket.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => {
                assert!(
                    !result.is_terminal(),
                    "second terminal emitted after the first: {result:?}"
                );
                payloads.push(result);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    (payloads, terminals)
}

#[test]
fn cleanup_temp_files_emits_exactly_one_terminal_completion() {
    let dir = tempfile::tempdir().unwrap();
    let audit = AuditLog::open(dir.path()).unwrap();
    let mut cfg_val = AppConfig::default();
    cfg_val.passphrase = "contract-passphrase-1234567890".into();
    cfg_val.log_dir = dir.path().join("logs");
    let (_runtime, client, _broadcast_rx) = Runtime::start(audit, Arc::new(cfg_val));

    let ticket = client.submit(Job::CleanupTempFiles).expect("submit");
    assert_eq!(ticket.metadata().label, "cleanup_temp_files");

    let (payloads, terminals) = drain_routed_ticket(&ticket, Duration::from_secs(60));
    assert!(
        payloads
            .iter()
            .all(|result| matches!(result, JobResult::Progress { .. })),
        "cleanup must not emit intermediate payloads other than progress: {payloads:?}"
    );
    assert_eq!(
        terminals.len(),
        1,
        "cleanup must emit exactly one terminal result, got: {terminals:?}"
    );
    let terminal = &terminals[0];
    assert!(
        matches!(
            terminal,
            JobResult::JobCompleted {
                job_label,
                disposition: OperationDisposition::Succeeded,
                ..
            } if job_label == "cleanup_temp_files"
        ),
        "expected a successful cleanup_temp_files JobCompleted, got: {terminal:?}"
    );
    // The terminal also frees the GUI wait slot (terminal ⊆ GUI-ending).
    assert!(terminal.ends_gui_tracked_job());
}

#[test]
fn validate_credentials_emits_payload_then_single_terminal_completion() {
    let dir = tempfile::tempdir().unwrap();
    let audit = AuditLog::open(dir.path()).unwrap();
    let mut cfg_val = AppConfig::default();
    cfg_val.passphrase = "contract-passphrase-1234567890".into();
    cfg_val.log_dir = dir.path().join("logs");
    let (_runtime, client, _broadcast_rx) = Runtime::start(audit, Arc::new(cfg_val));

    let ticket = client.submit(Job::ValidateCredentials).expect("submit");
    assert_eq!(ticket.metadata().label, "validate_credentials");

    let (payloads, terminals) = drain_routed_ticket(&ticket, Duration::from_secs(120));

    // The useful payload is delivered first …
    let report = payloads
        .iter()
        .find_map(|result| match result {
            JobResult::ApiKeysVerified(report) => Some(report),
            _ => None,
        })
        .expect("ValidateCredentials must emit its ApiKeysVerified payload");
    // … and is classified as intermediate: it landed in `payloads` (not
    // `terminals`), so it neither ends the tracker lifecycle nor frees a GUI
    // wait slot. Only the terminal below does.
    let payload = JobResult::ApiKeysVerified(report.clone());
    assert!(!payload.is_terminal());
    assert!(!payload.ends_gui_tracked_job());

    // Exactly one terminal …
    assert_eq!(
        terminals.len(),
        1,
        "validate_credentials must emit exactly one terminal result, got: {terminals:?}"
    );
    let terminal = &terminals[0];
    let (job_label, disposition) = match terminal {
        JobResult::JobCompleted {
            job_label,
            disposition,
            ..
        } => (job_label.as_str(), *disposition),
        other => panic!("expected a JobCompleted terminal, got: {other:?}"),
    };
    assert_eq!(job_label, "validate_credentials");
    // … whose disposition mirrors the report payload (payload-driven, exactly
    // like the runtime's `report.exit_code()` mapping).
    let expected = match report.exit_code() {
        0 => OperationDisposition::Succeeded,
        1 => OperationDisposition::Partial,
        _ => OperationDisposition::Failed,
    };
    assert_eq!(disposition, expected, "disposition must mirror the payload");
    assert!(terminal.ends_gui_tracked_job());
}
