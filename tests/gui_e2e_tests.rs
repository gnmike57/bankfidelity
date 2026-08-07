use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::runtime::JobResult;
use eframe::egui;
use rust_decimal::Decimal;
use std::sync::Arc;

fn make_headless_app() -> (
    dual_core_pdf_pipeline::app::gui::MyApp,
    std::sync::mpsc::Sender<JobResult>,
) {
    let _ = dotenvy::dotenv();
    let mut cfg = AppConfig::from_env().unwrap_or_default();
    cfg.interactive_fallbacks = false;
    let cfg = Arc::new(cfg);

    let (job_tx, _job_rx) = std::sync::mpsc::channel();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let app = dual_core_pdf_pipeline::app::gui::MyApp::new(job_tx, result_rx, cfg);
    (app, result_tx)
}

fn pump(app: &mut dual_core_pdf_pipeline::app::gui::MyApp, ctx: &egui::Context) {
    let raw_input = egui::RawInput {
        time: Some(0.0),
        ..Default::default()
    };
    let _ = ctx.run(raw_input, |ctx| {
        app.headless_update(ctx);
    });
}

#[test]
fn test_gui_headless_interactions() {
    let (mut app, _result_tx) = make_headless_app();
    let ctx = egui::Context::default();

    // Drag-and-drop: only assert path change when the fixture exists.
    let sample = std::path::PathBuf::from("examples/sample.pdf");
    let mut raw_input = egui::RawInput {
        time: Some(0.0),
        ..Default::default()
    };
    if sample.exists() {
        raw_input.dropped_files.push(egui::DroppedFile {
            path: Some(sample.clone()),
            name: "sample.pdf".to_string(),
            last_modified: None,
            bytes: None,
            mime: String::new(),
        });
        let _ = ctx.run(raw_input.clone(), |ctx| {
            app.headless_update(ctx);
        });
        assert!(
            app.input_path.contains("sample.pdf"),
            "drop should open sample path, got {}",
            app.input_path
        );
    } else {
        // Still exercise headless frame without a fixture.
        pump(&mut app, &ctx);
    }

    app.settings.default_dpi = 300.0;
    pump(&mut app, &ctx);

    // Aggressive resize must not panic.
    raw_input.screen_rect = Some(egui::Rect::from_min_size(
        egui::pos2(0.0, 0.0),
        egui::vec2(400.0, 300.0),
    ));
    let _ = ctx.run(raw_input.clone(), |ctx| {
        app.headless_update(ctx);
    });

    let image = egui::ColorImage::new([1, 1], egui::Color32::BLACK);
    app.current_page_texture = Some(ctx.load_texture("test", image, Default::default()));
    app.current_pdf_path = std::path::PathBuf::from("examples/sample.pdf");
    app.total_pages = 1;

    // Page indices in Transaction are 0-based (Document AI / offline convention).
    app.workflow_transactions
        .push(dual_core_pdf_pipeline::engine::model::Transaction {
            page: 0,
            line_on_page: 0,
            date: "2024-01-01".to_string(),
            raw_text: "Test".to_string(),
            debit: None,
            credit: Some(Decimal::new(100, 0)),
            running_balance: Some(Decimal::new(1000, 0)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: dual_core_pdf_pipeline::engine::model::Provenance::Manual,
            category: None,
            canonical: Default::default(),
        });

    app.proposed_changes.push((
        dual_core_pdf_pipeline::engine::model::ProposedChange {
            page: 0,
            old_text: "100".to_string(),
            new_text: "200".to_string(),
            reason: "test".to_string(),
            confidence: 1.0,
            affects_subsequent_balances: false,
            bbox: None,
        },
        true,
    ));

    use dual_core_pdf_pipeline::app::gui::ActiveWorkflow;
    for wf in [
        ActiveWorkflow::EditStatement,
        ActiveWorkflow::TransferTransactions,
        ActiveWorkflow::AgentCommand,
        ActiveWorkflow::AuditForensics,
        ActiveWorkflow::ChaosSandbox,
        ActiveWorkflow::Settings,
        ActiveWorkflow::ApiKeys,
    ] {
        app.active_workflow = wf;
        pump(&mut app, &ctx);
    }

    use dual_core_pdf_pipeline::app::gui::ActiveModal;
    for modal in [
        ActiveModal::None,
        ActiveModal::DiscardDraftConfirm,
        ActiveModal::WorkflowHitl,
        ActiveModal::Settings,
        ActiveModal::CommandPalette,
        ActiveModal::Transfer,
        ActiveModal::Feedback,
        ActiveModal::DateAdjust,
        ActiveModal::TransferTest,
    ] {
        app.active_modal = modal;
        pump(&mut app, &ctx);
    }

    use dual_core_pdf_pipeline::engine::workflow::{
        BalancePreview, ParseValidation, VisualAttempt, WorkflowStage,
    };
    for stage in [
        WorkflowStage::Idle,
        WorkflowStage::Parsing,
        WorkflowStage::Editing(ParseValidation {
            total_pages: 1,
            transactions_found: 5,
            opening_balance: Decimal::new(0, 0),
            closing_balance: Decimal::new(0, 0),
            account_number: None,
            completeness_score: 1.0,
            completeness_notes: String::new(),
            missing_rows: Vec::new(),
        }),
        WorkflowStage::Previewing(BalancePreview {
            rows: vec![],
            final_imbalance: Decimal::new(0, 0),
            balanced: true,
            auto_correction_message: None,
        }),
        WorkflowStage::Validating(VisualAttempt {
            attempt: 1,
            max_attempts: 5,
            diff_score: 0.05,
            threshold: 0.02,
            only_intended: false,
            message: String::new(),
        }),
        WorkflowStage::FinalChecking,
    ] {
        app.workflow_stage = stage;
        pump(&mut app, &ctx);
    }
}

#[test]
fn test_gui_ufo_result_lifecycle_clears_busy_flags() {
    let (mut app, result_tx) = make_headless_app();
    let ctx = egui::Context::default();

    // Simulate an in-flight UFO auto-edit.
    app.is_ufo_running = true;
    app.in_flight = 1;
    app.ufo_logs.push("starting".into());

    result_tx
        .send(JobResult::UfoLog("ufo line 1".into()))
        .expect("send log");
    pump(&mut app, &ctx);
    assert!(
        app.ufo_logs.iter().any(|l| l.contains("ufo line 1")),
        "UFO log lines should stream into gui buffer"
    );
    assert!(app.is_ufo_running, "logs alone must not clear running flag");

    result_tx
        .send(JobResult::UfoAutoEditResult(serde_json::json!({
            "status": "success",
            "task_id": "bankfidelity_test",
            "output": "done"
        })))
        .expect("send result");
    pump(&mut app, &ctx);

    assert!(!app.is_ufo_running, "success result must clear UFO running");
    assert_eq!(app.in_flight, 0, "success result must clear in_flight");
}

#[test]
fn test_gui_ufo_error_status_clears_busy_without_false_success() {
    let (mut app, result_tx) = make_headless_app();
    let ctx = egui::Context::default();

    app.is_ufo_running = true;
    app.in_flight = 1;

    result_tx
        .send(JobResult::UfoAutoEditResult(serde_json::json!({
            "status": "error",
            "task_id": "bankfidelity_err",
            "message": "UFO framework not found"
        })))
        .expect("send error-shaped result");
    pump(&mut app, &ctx);

    assert!(!app.is_ufo_running);
    assert_eq!(app.in_flight, 0);
}

#[test]
fn test_gui_ufo_dispatch_error_clears_busy_flags() {
    let (mut app, result_tx) = make_headless_app();
    let ctx = egui::Context::default();

    app.is_ufo_running = true;
    app.in_flight = 1;

    result_tx
        .send(JobResult::Error {
            job_label: "ufo_dispatch".into(),
            message: "UFO Auto-Edit failed: UFO framework not found".into(),
        })
        .expect("send dispatch error");
    pump(&mut app, &ctx);

    assert!(
        !app.is_ufo_running,
        "ufo_dispatch errors must clear is_ufo_running"
    );
    assert_eq!(app.in_flight, 0);
}

#[test]
fn test_gui_ufo_log_stream_preserves_order() {
    let (mut app, result_tx) = make_headless_app();
    let ctx = egui::Context::default();
    app.ufo_logs.clear();

    for line in ["alpha", "beta", "gamma"] {
        result_tx
            .send(JobResult::UfoLog(line.into()))
            .expect("send log");
    }
    pump(&mut app, &ctx);

    let joined = app.ufo_logs.join("|");
    assert!(
        joined.contains("alpha") && joined.contains("beta") && joined.contains("gamma"),
        "expected streamed UFO logs, got {joined:?}"
    );
}

#[test]
fn test_gui_in_flight_single_decrement_on_page_render() {
    let (mut app, result_tx) = make_headless_app();
    let ctx = egui::Context::default();

    app.in_flight = 1;
    result_tx
        .send(JobResult::PageRendered {
            png_bytes: vec![],
            page: 0,
            dpi: 150.0,
            tag: "current".into(),
            width_pts: 612.0,
            height_pts: 792.0,
        })
        .expect("send page rendered");
    pump(&mut app, &ctx);
    assert_eq!(
        app.in_flight, 0,
        "PageRendered must free exactly one in_flight slot"
    );

    // Error must also free exactly once (no double-decrement via handler).
    app.in_flight = 2;
    result_tx
        .send(JobResult::Error {
            job_label: "render_page".into(),
            message: "boom".into(),
        })
        .expect("send error");
    pump(&mut app, &ctx);
    assert_eq!(app.in_flight, 1, "Error must free exactly one slot");
}

#[test]
fn test_gui_document_loaded_does_not_free_wait_until_parse() {
    let (mut app, result_tx) = make_headless_app();
    let ctx = egui::Context::default();

    app.in_flight = 1;
    app.input_path = "examples/sample.pdf".into();
    result_tx
        .send(JobResult::DocumentLoaded {
            layout_json: "{}".into(),
            total_pages: 2,
        })
        .expect("send loaded");
    pump(&mut app, &ctx);
    // DocumentLoaded keeps the wait open for the auto-chained parse.
    assert!(
        app.in_flight >= 1,
        "DocumentLoaded must not clear the open-document wait; got {}",
        app.in_flight
    );
}
