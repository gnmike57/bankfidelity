use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::gui::MyApp;
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult};
use dual_core_pdf_pipeline::engine::verification::VerificationReport;
use dual_core_pdf_pipeline::engine::workflow::WorkflowStage;
use egui_kittest::kittest::Queryable;
use std::sync::{mpsc, Arc};

#[test]
fn test_app_initialization_state() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (_job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());

    let app = MyApp::new(job_tx, job_rx, config);

    // Default state should be Idle
    assert_eq!(app.workflow_stage, WorkflowStage::Idle);
    assert_eq!(app.in_flight, 0);
}

#[test]
fn test_update_processes_job_results() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    // Simulate verification completion
    job_tx_dummy
        .send(JobResult::VerificationReport(VerificationReport {
            math_valid: true,
            visual_diff_score: 0.01,
            only_intended_changes: true,
            report_files: vec![],
            message: "OK".to_string(),
            max_tile_score: 0.01,
            max_edit_region_score: 0.01,
            min_ssim: 1.0,
            gates: Vec::new(),
        }))
        .unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });

        harness.step();
    }

    // Verification should be stored
    assert!(app.last_verification.is_some());
    assert!(app.last_verification.as_ref().unwrap().math_valid);
}

#[test]
fn test_handle_job_result_document_loaded() {
    let (job_tx, job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    // Simulate DocumentLoaded
    job_tx_dummy
        .send(JobResult::DocumentLoaded {
            layout_json: "{}".to_string(),
            total_pages: 5,
        })
        .unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    // Verify state updates
    assert_eq!(app.total_pages, 5);
    assert_eq!(app.status, "Loaded 5 page(s)");

    // It should have sent a ParseAndValidate job to the channel
    let mut found_parse_job = false;
    while let Ok(sent_job) = job_rx_dummy.try_recv() {
        if let Job::WorkflowParseAndValidate { .. } = sent_job {
            found_parse_job = true;
        }
    }
    assert!(
        found_parse_job,
        "Expected ParseAndValidate job to be dispatched"
    );
}

#[test]
fn test_handle_job_result_change_applied() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    let record = dual_core_pdf_pipeline::engine::history::ChangeRecord {
        id: 1,
        timestamp: "now".to_string(),
        page: 0,
        bbox: [0.0, 0.0, 10.0, 10.0],
        old_text: "old".to_string(),
        new_text: "new".to_string(),
        description: "Edit".to_string(),
        snapshot_path: None,
        snapshot_evidence: None,
        obj_id: None,
        provenance: "UserEdit".to_string(),
    };

    job_tx_dummy
        .send(JobResult::ChangeApplied {
            record: record.clone(),
            requires_visual_review: true,
        })
        .unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    assert_eq!(app.status, "Change applied");
}

#[test]
fn test_handle_job_result_font_analysis() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    let analysis = dual_core_pdf_pipeline::engine::font_analysis::FontAnalysis {
        fonts: vec![],
        summary: dual_core_pdf_pipeline::engine::font_analysis::FontAnalysisSummary {
            total_fonts: 0,
            fonts_needing_action: 0,
            missing_digit_count: 0,
            missing_letter_count: 0,
            missing_other_count: 0,
            all_fonts_covered: true,
        },
    };

    job_tx_dummy
        .send(JobResult::FontAnalysisReady(analysis))
        .unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    // Test completes successfully if it does not panic.
}

#[test]
fn test_handle_job_result_transactions_extracted() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);
    app.in_flight = 1;

    let txs = vec![dual_core_pdf_pipeline::engine::model::Transaction {
        page: 1,
        line_on_page: 1,
        date: "01/01".to_string(),
        raw_text: "Test".to_string(),
        debit: None,
        credit: None,
        running_balance: None,
        bbox: None,
        field_bboxes: Default::default(),
        provenance: dual_core_pdf_pipeline::engine::model::Provenance::Manual,
        category: None,
        canonical: Default::default(),
    }];

    job_tx_dummy
        .send(JobResult::TransactionsExtracted(txs))
        .unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    assert_eq!(app.in_flight, 0);
    assert_eq!(app.workflow_transactions.len(), 1);
}

#[test]
fn test_handle_job_result_error() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    job_tx_dummy
        .send(JobResult::Error {
            job_label: "TestJob".to_string(),
            message: "Something went wrong".to_string(),
        })
        .unwrap();

    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build(|ctx| {
            app.headless_update(ctx);
        });
    harness.step();

    // Since it's an unknown error, it should trigger pending_autofix
    // which displays the autofix modal. We assert the modal is rendered.
    let node = harness
        .get_all_by_label_contains("⚠️ Operation Failed")
        .next();
    assert!(node.is_some());

    // Also, error logs should be exported
    let dir = std::path::PathBuf::from("audit/error_reports");
    assert!(dir.exists());
}

#[test]
fn test_handle_job_result_progress() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    job_tx_dummy
        .send(JobResult::Progress {
            label: "Processing...".to_string(),
            fraction: 0.5,
        })
        .unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    assert!(app.progress.is_some());
    let prog = app.progress.as_ref().unwrap();
    assert_eq!(prog.label, "Processing...");
    assert_eq!(prog.fraction, 0.5);

    // Complete progress
    job_tx_dummy
        .send(JobResult::Progress {
            label: "Processing...".to_string(),
            fraction: 1.0,
        })
        .unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    assert!(app.progress.is_none());
}

#[test]
fn test_handle_job_result_pong() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    job_tx_dummy.send(JobResult::Pong).unwrap();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    // Verify toast is sent (we can't easily assert on toasts directly here,
    // but we can ensure it doesn't panic)
}
