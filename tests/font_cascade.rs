//! Fail-closed integration contract for legacy automatic font generation.
//!
//! The operation remains in the worker protocol for compatibility, but it must
//! never synthesize glyphs, select a donor typeface, or create an output file.

use dual_core_pdf_pipeline::app::audit::AuditLog;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::runtime::{Job, PythonJob, PythonJobResult, Runtime};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn legacy_font_cascade_is_disabled_and_creates_no_artifact() {
    let directory = tempdir().unwrap();
    let audit = AuditLog::open(directory.path()).unwrap();
    let mut config = AppConfig::default();
    config.passphrase = "font-policy-test-passphrase-1234".into();
    config.log_dir = directory.path().join("logs");
    let (_runtime, job_tx, _job_rx) = Runtime::start(audit, Arc::new(config));

    let output_directory = directory.path().join("font-output");
    std::fs::create_dir_all(&output_directory).unwrap();
    let source = directory.path().join("source.pdf");
    std::fs::write(&source, b"%PDF-1.4\n%%EOF\n").unwrap();

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    job_tx
        .send(Job::Python(
            PythonJob::ReplicateFontForMissingChars {
                pdf_path: source.to_string_lossy().to_string(),
                font_name: "LegacySubset".into(),
                missing_chars_csv: "4,5,A".into(),
                output_dir: output_directory.to_string_lossy().to_string(),
            },
            reply_tx,
        ))
        .expect("send disabled font operation");

    let payload = match reply_rx.blocking_recv().expect("font operation reply") {
        PythonJobResult::Json(payload) => payload,
        other => panic!("unexpected font operation result: {other:?}"),
    };
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();

    assert_eq!(value["success"], false);
    assert_eq!(value["error"], "FONT_SUBSTITUTION_DISABLED");
    assert_eq!(value["extended_font_path"], serde_json::Value::Null);
    assert_eq!(value["still_missing"], serde_json::json!(["4", "5", "A"]));
    assert_eq!(value["tiers_used"], serde_json::json!([]));
    assert_eq!(
        std::fs::read_dir(output_directory).unwrap().count(),
        0,
        "disabled font operation must create no artifacts"
    );
}
