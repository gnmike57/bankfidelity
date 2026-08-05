//! End-to-end smoke test for the runtime job loop.
//!
//! Spins up a real `Runtime`, sends a `Ping`, and asserts a `Pong` comes back.
//! Also exercises history save/load round-trip via `LoadHistory`.

use dual_core_pdf_pipeline::app::audit::AuditLog;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult, Runtime};
use dual_core_pdf_pipeline::engine::history::ChangeHistory;
use std::path::PathBuf;
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
        if let Ok(r) = rx.recv_timeout(Duration::from_millis(200)) {
            if pred(&r) {
                return Some(r);
            }
        }
    }
    None
}

#[test]
fn runtime_ping_pong_smoke() {
    let dir = tempdir().unwrap();
    let audit = AuditLog::open(dir.path()).unwrap();
    let mut cfg_val = AppConfig::default();
    cfg_val.passphrase = "smoke-passphrase-1234567890".into();
    cfg_val.log_dir = dir.path().join("logs");
    let cfg = Arc::new(cfg_val);

    let (_runtime, job_tx, job_rx) = Runtime::start(audit, cfg);

    job_tx.send(Job::Ping).unwrap();
    let pong = drain_until(
        &job_rx,
        |r| matches!(r, JobResult::Pong),
        Duration::from_secs(15),
    );
    assert!(
        pong.is_some(),
        "did not receive Pong from runtime within timeout"
    );
}

#[test]
fn runtime_load_history_round_trips_through_actor() {
    let dir = tempdir().unwrap();
    let audit = AuditLog::open(dir.path()).unwrap();
    let mut cfg_val = AppConfig::default();
    cfg_val.passphrase = "smoke-passphrase-1234567890".into();
    cfg_val.log_dir = dir.path().join("logs");
    let cfg = Arc::new(cfg_val);

    // Save a tiny history file ahead of time.
    let mut original = ChangeHistory::new();
    original.push_change(0, "old".into(), "new".into(), [0.0; 4], "smoke".into());
    let history_path: PathBuf = dir.path().join("history.json");
    original.save_to_file(&history_path).unwrap();

    let (_runtime, job_tx, job_rx) = Runtime::start(audit, cfg);

    job_tx
        .send(Job::LoadHistory {
            input: history_path,
        })
        .unwrap();

    let res = drain_until(
        &job_rx,
        |r| matches!(r, JobResult::HistoryUpdated { .. }),
        Duration::from_secs(15),
    );
    assert!(res.is_some(), "expected HistoryUpdated within timeout");
    if let Some(JobResult::HistoryUpdated { history }) = res {
        assert_eq!(history.get_history().len(), 1);
        assert_eq!(history.get_history()[0].new_text, "new");
    }
}
