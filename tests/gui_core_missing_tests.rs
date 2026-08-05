use dual_core_pdf_pipeline::app::audit::AuditLog;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult, Runtime, RuntimeClient};
use dual_core_pdf_pipeline::engine::model::{FieldBboxes, Provenance, Transaction};
use std::sync::Arc;
use std::time::Duration;

fn setup_worker() -> (Runtime, RuntimeClient, mpsc::Receiver<JobResult>) {
    let audit_log = AuditLog::open("audit.log").unwrap();
    let config = Arc::new(AppConfig::default());
    Runtime::start(audit_log, config)
}

use std::sync::mpsc;

#[test]
fn test_submit_bug_report_coverage() {
    let (_worker, tx, rx) = setup_worker();
    tx.send(Job::SubmitBugReport {
        description: "Test bug report".into(),
        include_logs: false,
        include_audit: false,
    })
    .unwrap();

    let mut found = false;
    for _ in 0..10 {
        if let Ok(JobResult::BugReportSubmitted | JobResult::Error { .. }) =
            rx.recv_timeout(Duration::from_millis(500))
        {
            found = true;
            break;
        }
    }
    assert!(found, "SubmitBugReport handler did not respond in time");
}

#[test]
fn test_categorize_transactions_coverage() {
    let (_worker, tx, rx) = setup_worker();
    let txs = vec![Transaction {
        date: "2024-01-01".into(),
        raw_text: "WALMART".into(),
        debit: Some("-15.00".parse().unwrap()),
        credit: None,
        running_balance: Some(rust_decimal::Decimal::ZERO),
        page: 0,
        line_on_page: 0,
        bbox: Some([0.0; 4]),
        field_bboxes: FieldBboxes::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    }];
    tx.send(Job::CategorizeTransactions { transactions: txs })
        .unwrap();

    let mut found = false;
    for _ in 0..10 {
        if let Ok(JobResult::CategorizationReady(categorized)) =
            rx.recv_timeout(Duration::from_millis(500))
        {
            assert_eq!(categorized.len(), 1);
            assert_eq!(categorized[0].raw_text, "WALMART");
            found = true;
            break;
        }
    }
    assert!(
        found,
        "CategorizeTransactions handler did not respond in time"
    );
}

#[test]
fn test_generate_visual_alternatives_coverage() {
    let (_worker, tx, rx) = setup_worker();
    let input = std::path::PathBuf::from("dummy.pdf");
    let out_dir = std::path::PathBuf::from("out_dir");
    tx.send(Job::GenerateVisualAlternatives {
        input,
        out_dir,
        page: 0,
        edits: vec![],
        bbox: [0.0; 4],
    })
    .unwrap();

    let mut found = false;
    for _ in 0..10 {
        if let Ok(res) = rx.recv_timeout(Duration::from_millis(500)) {
            if let JobResult::Error { job_label, .. } = res {
                assert_eq!(job_label, "generate_visual_alternatives");
                found = true;
                break;
            } else if let JobResult::VisualAlternativesReady { .. } = res {
                found = true;
                break;
            }
        }
    }
    assert!(
        found,
        "GenerateVisualAlternatives handler did not respond in time"
    );
}

#[test]
fn test_cancel_job_coverage() {
    let (_worker, tx, _rx) = setup_worker();
    // Since cancel is best effort, we just ensure it doesn't panic
    tx.send(Job::Cancel { id: 999 }).unwrap();
    // Allow it to process
    std::thread::sleep(Duration::from_millis(100));
}
