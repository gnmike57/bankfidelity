use dual_core_pdf_pipeline::app::audit::AuditLog;
use dual_core_pdf_pipeline::app::config::{AppConfig, DocumentParserMode};
use dual_core_pdf_pipeline::app::runtime::{Job, Runtime};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_runtime_stress_load() {
    let mut config = AppConfig::default();
    config.passphrase = "phase06-stress-test-passphrase-1234".into();
    let config = Arc::new(config);
    let audit_dir = tempfile::tempdir().unwrap();
    let audit_log = AuditLog::open(audit_dir.path()).unwrap();

    // Spawn the runtime server
    let (_runtime, job_tx, _job_rx) = Runtime::start(audit_log, config.clone());

    let test_pdf = PathBuf::from("examples/sample.pdf");
    let parser_mode = DocumentParserMode::OfflineHeuristic;
    let ai_provider = config.ai_provider;

    // Dispatch 10 concurrent WorkflowParseAndValidate jobs
    let num_jobs = 10;
    for i in 0..num_jobs {
        let job = Job::WorkflowParseAndValidate {
            input: test_pdf.clone(),
            version: None,
            parser_mode,
            ai_provider,
            ignore_offline_fallback: true,
        };
        assert!(job_tx.send(job).is_ok(), "Failed to enqueue job {}", i);
    }

    // Wait for all 10 jobs to complete
    let mut completed = 0;
    while completed < num_jobs {
        let res = _job_rx
            .recv_timeout(std::time::Duration::from_secs(300))
            .expect("Timeout waiting for job completion");
        match res {
            dual_core_pdf_pipeline::app::runtime::JobResult::WorkflowParseValidated { .. } => {
                completed += 1;
                println!("Job completed successfully! {}/{}", completed, num_jobs);
            }
            dual_core_pdf_pipeline::app::runtime::JobResult::Error { message, .. } => {
                completed += 1;
                println!("Job failed: {} ({}/{})", message, completed, num_jobs);
            }
            _ => {} // Ignore progress updates
        }
    }
}
