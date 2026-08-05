//! Stage 12 / Item #5: end-to-end workflow smoke.
//!
//! Exercises the full Stage 1 → Stage 6 chain against the real AU statement
//! at `AU Bank Statements/IA_Bank_Statement_202602.pdf`:
//!
//!   1. WorkflowParseAndValidate: Document AI parse + Gemini completeness
//!   2. Edit a single transaction's debit value
//!   3. WorkflowPreview: balance cascade + per-row diff
//!   4. WorkflowConfirmAndRender: binary edit → visual loop → final DocAI re-parse
//!   5. Assert: rendered PDF exists, math is valid, visual diff is below threshold
//!
//! Marked `#[ignore]` because it requires real API keys + the AU sample PDF
//! and produces network traffic. Run manually with:
//!
//!   cargo test --test workflow_e2e -- --ignored --nocapture
//!
//! When the AU statement file is missing (e.g. CI without test data), the
//! test self-skips with a clear message.

use dual_core_pdf_pipeline::app::audit::AuditLog;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult, Runtime};
use dual_core_pdf_pipeline::engine::workflow::{EditField, UserEdit, WorkflowOutcome};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;

fn drain_until<F: Fn(&JobResult) -> bool>(
    rx: &std::sync::mpsc::Receiver<JobResult>,
    pred: F,
    max: Duration,
) -> Option<JobResult> {
    let deadline = std::time::Instant::now() + max;
    while std::time::Instant::now() < deadline {
        if let Ok(r) = rx.recv_timeout(Duration::from_millis(500)) {
            if pred(&r) {
                return Some(r);
            }
        }
    }
    None
}

#[test]
fn end_to_end_workflow_against_au_statement() {
    let pdf = PathBuf::from("AU Bank Statements/IA_Bank_Statement_202602.pdf");
    if !pdf.exists() {
        eprintln!(
            "[skip] AU statement not present at {}; e2e test self-skipped",
            pdf.display()
        );
        return;
    }

    // Real config is required — the test self-skips if Document AI / Gemini
    // aren't configured, since we'd just get auth failures otherwise.
    let cfg = match AppConfig::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("[skip] AppConfig::load failed: {e}; e2e self-skipped");
            return;
        }
    };
    if cfg.document_ai.is_none() {
        eprintln!("[skip] DOCUMENT_AI_* env vars not set; e2e self-skipped");
        return;
    }

    let dir = tempdir().unwrap();
    let audit = AuditLog::open(dir.path()).unwrap();
    let (_runtime, job_tx, job_rx) = Runtime::start(audit, cfg);
    let output = dir.path().join("edited.pdf");

    eprintln!("[e2e] Stage 1: parse + validate");
    job_tx
        .send(Job::WorkflowParseAndValidate {
            input: pdf.clone(),
            version: None,
            parser_mode: dual_core_pdf_pipeline::app::config::DocumentParserMode::DocumentAi,
            ai_provider: dual_core_pdf_pipeline::app::config::AiProviderMode::GeminiApiKey,
            ignore_offline_fallback: false,
        })
        .unwrap();
    let parse = drain_until(
        &job_rx,
        |r| {
            matches!(
                r,
                JobResult::WorkflowParseValidated { .. } | JobResult::Error { .. }
            )
        },
        Duration::from_secs(180),
    );
    let (validation, transactions) = match parse {
        Some(JobResult::WorkflowParseValidated {
            validation,
            transactions,
        }) => (validation, transactions),
        Some(JobResult::Error { message, .. }) => {
            panic!("workflow parse failed: {message}");
        }
        _ => panic!("workflow parse timed out"),
    };
    assert!(
        validation.transactions_found > 0,
        "Document AI returned zero transactions for the AU statement"
    );
    eprintln!(
        "[e2e]   parsed {} txs • opening {} • closing {}",
        validation.transactions_found, validation.opening_balance, validation.closing_balance
    );

    // Pick a transaction with a non-zero debit to mutate. We bump it by $0.01
    // so the cascading balance changes by a known amount.
    let target_idx = transactions
        .iter()
        .position(|t| t.debit.is_some() && t.bbox.is_some())
        .or_else(|| {
            transactions
                .iter()
                .position(|t| t.credit.is_some() && t.bbox.is_some())
        })
        .expect("no editable transaction with bbox");
    let target = &transactions[target_idx];
    let (field, old_value) = if let Some(d) = target.debit {
        (EditField::Debit, d)
    } else {
        (EditField::Credit, target.credit.unwrap())
    };
    let new_value = old_value + rust_decimal_macros::dec!(0.01);
    let edit = UserEdit {
        page: target.page,
        line_on_page: target.line_on_page,
        bbox: target.bbox.unwrap(),
        old_text: format!("{old_value:.2}"),
        new_text: format!("{new_value:.2}"),
        field,
    };

    eprintln!("[e2e] Stage 3: balance preview");
    job_tx
        .send(Job::WorkflowPreview {
            original_transactions: transactions.clone(),
            edits: vec![edit.clone()],
            opening_balance: validation.opening_balance,
            expected_closing: if validation.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                Some(validation.closing_balance)
            } else {
                None
            },
        })
        .unwrap();
    let preview = drain_until(
        &job_rx,
        |r| {
            matches!(
                r,
                JobResult::WorkflowPreviewBuilt(_)
                    | JobResult::WorkflowFailed(_)
                    | JobResult::Error { .. }
            )
        },
        Duration::from_secs(60),
    );
    let preview = match preview {
        Some(JobResult::WorkflowPreviewBuilt(p)) => p,
        Some(JobResult::WorkflowFailed(f)) => panic!("preview failed (WorkflowFailed): {f:?}"),
        Some(JobResult::Error { message, .. }) => panic!("preview failed: {message}"),
        _ => panic!("preview timed out"),
    };
    let changed = preview.changed_row_count();
    eprintln!(
        "[e2e]   preview: {} row(s) will change • final imbalance ${}",
        changed, preview.final_imbalance
    );
    assert!(
        changed >= 1,
        "preview should mark at least the edited row as changed"
    );

    eprintln!("[e2e] Stage 4-6: confirm + render + validate + final check");
    job_tx
        .send(Job::WorkflowConfirmAndRender {
            input: pdf.clone(),
            output: output.clone(),
            edits: vec![edit],
            original_transactions: vec![],
            opening_balance: rust_decimal::Decimal::ZERO,
            expected_closing: None,
            deep_font_replication: false,
            max_visual_attempts: 3,
            visual_threshold: 0.02,
            ignore_font_coverage: false,
            ignore_visual_fidelity: false,
        })
        .unwrap();

    let mut outcome: Option<WorkflowOutcome> = None;
    let mut failure_message: Option<String> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    while std::time::Instant::now() < deadline && outcome.is_none() && failure_message.is_none() {
        if let Ok(res) = job_rx.recv_timeout(Duration::from_millis(500)) {
            match res {
                JobResult::WorkflowComplete(o) => outcome = Some(o),
                JobResult::WorkflowFailed(f) => failure_message = Some(format!("{f:?}")),
                JobResult::Error { message, .. } => failure_message = Some(message),
                JobResult::FontCascadeUsed(report) => {
                    eprintln!("[e2e]   {}", report.one_line_summary());
                }
                JobResult::WorkflowStageChanged { stage } => {
                    eprintln!("[e2e]   stage: {}", stage.label());
                }
                _ => {}
            }
        }
    }

    if let Some(msg) = failure_message {
        panic!("workflow failed: {msg}");
    }
    let outcome = outcome.expect("workflow did not complete within timeout");
    eprintln!("[e2e]   {}", outcome.completion_summary);
    assert!(outcome.math_valid, "final math must be valid: {outcome:?}");
    assert!(
        Path::new(&outcome.final_pdf).exists(),
        "rendered PDF must exist: {}",
        outcome.final_pdf.display()
    );
    eprintln!("[e2e]   visual attempts: {}", outcome.visual_attempts);
    eprintln!("[e2e]   final imbalance: ${}", outcome.final_imbalance);
    eprintln!("[e2e] PASS");
}
