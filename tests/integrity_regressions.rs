mod fixtures;

use dual_core_pdf_pipeline::app::runtime::{Job, JobResult};
use dual_core_pdf_pipeline::engine::model::{ProposedChange, Provenance, Transaction};
use dual_core_pdf_pipeline::engine::workflow::{EditField, UserEdit, WorkflowFailure};
use dual_core_pdf_pipeline::pdf::engine::PdfEngine;
use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
use rust_decimal_macros::dec;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn typst_reconstruction_is_disabled_and_preserves_destination() {
    let workspace = tempfile::tempdir().unwrap();
    let input = workspace.path().join("input.pdf");
    let output = workspace.path().join("existing-output.pdf");
    fixtures::generate_test_pdf(2, &input);
    std::fs::write(&output, b"prior-valid-output").unwrap();

    let config = Arc::new(dual_core_pdf_pipeline::app::config::AppConfig::default());
    let audit_log = dual_core_pdf_pipeline::app::audit::AuditLog::open(workspace.path()).unwrap();
    let (_runtime, job_tx, result_rx) =
        dual_core_pdf_pipeline::app::runtime::Runtime::start(audit_log, config);
    job_tx
        .send(Job::TypstReconstruct {
            input,
            output: output.clone(),
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match result_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(JobResult::Error { job_label, message }) => {
                assert_eq!(job_label, "typst_reconstruct_disabled");
                assert!(message.contains("cannot preserve edit-in-place fidelity"));
                assert_eq!(std::fs::read(&output).unwrap(), b"prior-valid-output");
                return;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("result channel failed: {error}"),
        }
    }
    panic!("disabled Typst job did not emit a terminal error");
}

#[test]
fn short_document_batch_commits_every_edit_before_success() {
    let workspace = tempfile::tempdir().unwrap();
    let input = workspace.path().join("input.pdf");
    let output = workspace.path().join("output.pdf");
    fixtures::generate_test_pdf(2, &input);

    let engine = OxidizePdfEngine::new();
    let first_before = engine.get_text_blocks(&input, 0).unwrap();
    let second_before = engine.get_text_blocks(&input, 1).unwrap();
    assert_eq!(first_before.len(), 1);
    assert_eq!(second_before.len(), 1);

    let changes = vec![
        ProposedChange {
            page: 0,
            bbox: Some(first_before[0].bbox),
            old_text: first_before[0].text.clone(),
            new_text: "FIRST EDIT".into(),
            reason: "integrity regression".into(),
            confidence: 1.0,
            affects_subsequent_balances: false,
        },
        ProposedChange {
            page: 1,
            bbox: Some(second_before[0].bbox),
            old_text: second_before[0].text.clone(),
            new_text: "SECOND EDIT".into(),
            reason: "integrity regression".into(),
            confidence: 1.0,
            affects_subsequent_balances: false,
        },
    ];

    std::env::remove_var("TEST_CRASH_PYTHON_ACTOR");
    let config = Arc::new(dual_core_pdf_pipeline::app::config::AppConfig::default());
    let audit_log = dual_core_pdf_pipeline::app::audit::AuditLog::open(workspace.path()).unwrap();
    let (_runtime, job_tx, result_rx) =
        dual_core_pdf_pipeline::app::runtime::Runtime::start(audit_log, config);

    job_tx
        .send(Job::ApplyProposedChanges {
            input: input.clone(),
            output: output.clone(),
            changes,
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut terminal = None;
    while Instant::now() < deadline {
        match result_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(JobResult::ProposedChangesApplied {
                changes_applied,
                failures,
            }) => {
                terminal = Some((changes_applied, failures));
                break;
            }
            Ok(JobResult::Error { message, .. }) => panic!("batch failed: {message}"),
            Ok(JobResult::WorkflowFailed(failure)) => panic!("workflow failed: {failure:?}"),
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("result channel failed: {error}"),
        }
    }

    let (changes_applied, failures) = terminal.expect("missing terminal batch result");
    assert_eq!(changes_applied, 2);
    assert!(failures.is_empty());
    assert!(
        output.is_file(),
        "success must follow durable output creation"
    );

    let first_after = engine.get_text_blocks(&output, 0).unwrap();
    let second_after = engine.get_text_blocks(&output, 1).unwrap();
    let normalized_page_text = |blocks: &[dual_core_pdf_pipeline::pdf::TextBlock]| {
        blocks
            .iter()
            .flat_map(|block| block.text.chars())
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    };
    let first_text = normalized_page_text(&first_after);
    let second_text = normalized_page_text(&second_after);
    assert!(
        first_text.contains("FIRSTEDIT"),
        "page 1 does not contain the exact replacement: {first_text:?}"
    );
    assert!(
        second_text.contains("SECONDEDIT"),
        "page 2 does not contain the exact replacement: {second_text:?}"
    );
    assert!(!first_text.contains("Page1"));
    assert!(!second_text.contains("Page2"));
}

#[test]
fn confirm_and_render_rejects_unbalanced_ledger_before_output_mutation() {
    let workspace = tempfile::tempdir().unwrap();
    let input = workspace.path().join("input.pdf");
    let output = workspace.path().join("existing-output.pdf");
    fixtures::generate_test_pdf(1, &input);
    std::fs::copy(&input, &output).unwrap();
    let output_before = std::fs::read(&output).unwrap();

    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 1);
    let original_transactions = vec![Transaction {
        page: 0,
        line_on_page: 0,
        date: "01/01/2026".into(),
        raw_text: blocks[0].text.clone(),
        debit: Some(dec!(100.00)),
        credit: None,
        running_balance: Some(dec!(200.00)),
        bbox: Some(blocks[0].bbox),
        field_bboxes: Default::default(),
        provenance: Provenance::Manual,
        category: None,
        canonical: Default::default(),
    }];
    let edits = vec![UserEdit {
        page: 0,
        line_on_page: 0,
        bbox: blocks[0].bbox,
        old_text: blocks[0].text.clone(),
        new_text: "DESCRIPTION EDIT".into(),
        field: EditField::Description,
    }];

    let config = Arc::new(dual_core_pdf_pipeline::app::config::AppConfig::default());
    let audit_log = dual_core_pdf_pipeline::app::audit::AuditLog::open(workspace.path()).unwrap();
    let (_runtime, job_tx, result_rx) =
        dual_core_pdf_pipeline::app::runtime::Runtime::start(audit_log, config);
    job_tx
        .send(Job::WorkflowConfirmAndRender {
            input,
            output: output.clone(),
            edits,
            original_transactions,
            opening_balance: dec!(100.00),
            expected_closing: Some(dec!(250.00)),
            deep_font_replication: false,
            max_visual_attempts: 1,
            visual_threshold: 0.02,
            ignore_font_coverage: false,
            ignore_visual_fidelity: false,
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut rejected = false;
    while Instant::now() < deadline {
        match result_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(JobResult::WorkflowFailed(WorkflowFailure::FinalMathInvalid { imbalance })) => {
                assert_eq!(imbalance, dec!(-50.00));
                rejected = true;
                break;
            }
            Ok(JobResult::WorkflowComplete(outcome)) => {
                panic!("unbalanced workflow completed: {outcome:?}")
            }
            Ok(JobResult::Error { message, .. }) => panic!("unexpected runtime error: {message}"),
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("result channel failed: {error}"),
        }
    }

    assert!(rejected, "missing deterministic imbalance rejection");
    assert_eq!(std::fs::read(&output).unwrap(), output_before);
}

#[test]
fn ordered_offline_router_extracts_complete_canonical_ledger() {
    let workspace = tempfile::tempdir().unwrap();
    let input = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/stress_pdfs/Standard_Bank_Statement_01.pdf");
    let config = Arc::new(dual_core_pdf_pipeline::app::config::AppConfig::default());
    let audit_log = dual_core_pdf_pipeline::app::audit::AuditLog::open(workspace.path()).unwrap();
    let (_runtime, job_tx, result_rx) =
        dual_core_pdf_pipeline::app::runtime::Runtime::start(audit_log, config);

    job_tx
        .send(Job::ExtractTransactions {
            path: input,
            parser_mode: dual_core_pdf_pipeline::app::config::DocumentParserMode::OfflineHeuristic,
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        match result_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(JobResult::TransactionsExtracted(transactions)) => {
                assert_eq!(transactions.len(), 30);
                for transaction in transactions {
                    assert_eq!(
                        transaction.canonical.stable_row_id,
                        format!("p{}:r{}", transaction.page, transaction.line_on_page)
                    );
                    assert!(transaction
                        .canonical
                        .confidence
                        .is_some_and(|value| value >= 0.85));
                    assert!(!transaction.canonical.review_required);
                }
                return;
            }
            Ok(JobResult::Error { message, .. }) => panic!("offline extraction failed: {message}"),
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("result channel failed: {error}"),
        }
    }
    panic!("offline extraction did not produce a terminal result");
}

#[test]
fn zero_row_statement_is_never_reported_as_extraction_or_balance_success() {
    let workspace = tempfile::tempdir().unwrap();
    let input = workspace.path().join("non-statement.pdf");
    fixtures::generate_test_pdf(2, &input);

    let config = Arc::new(dual_core_pdf_pipeline::app::config::AppConfig::default());
    let audit_log = dual_core_pdf_pipeline::app::audit::AuditLog::open(workspace.path()).unwrap();
    let (_runtime, job_tx, result_rx) =
        dual_core_pdf_pipeline::app::runtime::Runtime::start(audit_log, config);

    job_tx
        .send(Job::ExtractTransactions {
            path: input.clone(),
            parser_mode: dual_core_pdf_pipeline::app::config::DocumentParserMode::OfflineHeuristic,
        })
        .unwrap();
    let extract_deadline = Instant::now() + Duration::from_secs(45);
    let mut extraction_rejected = false;
    while Instant::now() < extract_deadline {
        match result_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(JobResult::TransactionsExtracted(transactions)) => {
                panic!("zero-row fixture reported extraction success: {transactions:?}")
            }
            Ok(JobResult::Error { job_label, message }) if job_label == "extract_transactions" => {
                assert!(message.contains("no transaction rows"));
                extraction_rejected = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("result channel failed: {error}"),
        }
    }
    assert!(extraction_rejected, "zero-row extraction did not fail");

    job_tx
        .send(Job::BalanceStatement {
            path: input.clone(),
        })
        .unwrap();
    let balance_deadline = Instant::now() + Duration::from_secs(45);
    let mut balance_rejected = false;
    while Instant::now() < balance_deadline {
        match result_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(JobResult::BalanceProposed { imbalance, changes }) => {
                panic!(
                    "zero-row fixture reported balance success: imbalance={imbalance}, changes={changes:?}"
                )
            }
            Ok(JobResult::Error { job_label, message }) if job_label == "balance_statement" => {
                assert!(message.contains("no transaction rows"));
                balance_rejected = true;
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("result channel failed: {error}"),
        }
    }
    assert!(balance_rejected, "zero-row balance analysis did not fail");
}
