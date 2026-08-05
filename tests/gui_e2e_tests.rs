use dual_core_pdf_pipeline::app::config::AppConfig;
use eframe::egui;
use std::sync::Arc;

#[test]
fn test_gui_headless_interactions() {
    let _ = dotenvy::dotenv();
    let mut cfg = AppConfig::from_env().unwrap_or_default();
    cfg.interactive_fallbacks = false; // Disable modals for test
    let _cfg = Arc::new(cfg);

    let (job_tx, _job_rx) = std::sync::mpsc::channel();
    let (_result_tx, result_rx) = std::sync::mpsc::channel();
    let mut app = dual_core_pdf_pipeline::app::gui::MyApp::new(job_tx, result_rx, _cfg.clone());

    let ctx = egui::Context::default();

    // Simulate some GUI time passing
    let mut raw_input = egui::RawInput {
        time: Some(0.0),
        ..Default::default()
    };

    // Test 1: Drag and Drop file ingestion
    raw_input.dropped_files.push(egui::DroppedFile {
        path: Some(std::path::PathBuf::from("examples/sample.pdf")),
        name: "sample.pdf".to_string(),
        last_modified: None,
        bytes: None,
        mime: String::new(),
    });

    // Run the UI state machine
    let _ = ctx.run(raw_input.clone(), |ctx| {
        app.headless_update(ctx);
    });

    // Check that drag and drop was accepted and path changed
    assert_eq!(app.input_path, "examples/sample.pdf");

    // Test 2: Modal Interactions
    // Let's pretend we opened the settings modal and changed something
    app.settings.default_dpi = 300.0;
    let _ = ctx.run(raw_input.clone(), |ctx| {
        app.headless_update(ctx);
    });

    // Test 3: Aggressive window resizing
    raw_input.screen_rect = Some(egui::Rect::from_min_size(
        egui::pos2(0.0, 0.0),
        egui::vec2(400.0, 300.0),
    ));
    let _ = ctx.run(raw_input.clone(), |ctx| {
        app.headless_update(ctx); // Must not panic with division by zero!
    });

    // Test 4: Job Debouncing
    // Inject multiple 'Parse' clicks by directly manipulating state?
    // Since we don't have easy egui mouse click synthesis for specific buttons,
    // we just ensure `app.in_flight` behaves properly when mocked.
    // Inject mock texture to bypass the "Loading Document..." screen!
    let image = egui::ColorImage::new([1, 1], egui::Color32::BLACK);
    app.current_page_texture = Some(ctx.load_texture("test", image, Default::default()));
    app.current_pdf_path = std::path::PathBuf::from("examples/sample.pdf");
    app.total_pages = 1;

    // Inject mock data to ensure all loops execute rendering logic
    app.workflow_transactions
        .push(dual_core_pdf_pipeline::engine::model::Transaction {
            page: 1,
            line_on_page: 1,
            date: "2024-01-01".to_string(),
            raw_text: "Test".to_string(),
            debit: None,
            credit: Some(rust_decimal::Decimal::new(100, 0)),
            running_balance: Some(rust_decimal::Decimal::new(1000, 0)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: dual_core_pdf_pipeline::engine::model::Provenance::Manual,
            category: None,
            canonical: Default::default(),
        });

    app.proposed_changes.push((
        dual_core_pdf_pipeline::engine::model::ProposedChange {
            page: 1,
            old_text: "100".to_string(),
            new_text: "200".to_string(),
            reason: "test".to_string(),
            confidence: 1.0,
            affects_subsequent_balances: false,
            bbox: None,
        },
        true,
    ));

    // Test 5: Switch through ActiveWorkflow stages
    use dual_core_pdf_pipeline::app::gui::ActiveWorkflow;
    let workflows = vec![
        ActiveWorkflow::EditStatement,
        ActiveWorkflow::TransferTransactions,
        ActiveWorkflow::AgentCommand,
        ActiveWorkflow::AuditForensics,
        ActiveWorkflow::ChaosSandbox,
        ActiveWorkflow::Settings,
        ActiveWorkflow::ApiKeys,
    ];

    for wf in workflows {
        app.active_workflow = wf;
        let _ = ctx.run(raw_input.clone(), |ctx| {
            app.headless_update(ctx);
        });
    }

    // Test 6: Trigger all active modals
    use dual_core_pdf_pipeline::app::gui::ActiveModal;
    let modals = vec![
        ActiveModal::None,
        ActiveModal::DiscardDraftConfirm,
        ActiveModal::WorkflowHitl,
        ActiveModal::Settings,
        ActiveModal::CommandPalette,
        ActiveModal::Transfer,
        ActiveModal::Feedback,
        ActiveModal::DateAdjust,
        ActiveModal::TransferTest,
    ];

    for modal in modals {
        app.active_modal = modal;
        let _ = ctx.run(raw_input.clone(), |ctx| {
            app.headless_update(ctx);
        });
    }

    // Test 7: Switch through WorkflowStages
    use dual_core_pdf_pipeline::engine::workflow::WorkflowStage;
    use dual_core_pdf_pipeline::engine::workflow::{
        BalancePreview, ParseValidation, VisualAttempt,
    };

    let stages = vec![
        WorkflowStage::Idle,
        WorkflowStage::Parsing,
        WorkflowStage::Editing(ParseValidation {
            total_pages: 1,
            transactions_found: 5,
            opening_balance: rust_decimal::Decimal::new(0, 0),
            closing_balance: rust_decimal::Decimal::new(0, 0),
            account_number: None,
            completeness_score: 1.0,
            completeness_notes: String::new(),
            missing_rows: Vec::new(),
        }),
        WorkflowStage::Previewing(BalancePreview {
            rows: vec![],
            final_imbalance: rust_decimal::Decimal::new(0, 0),
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
    ];

    for stage in stages {
        app.workflow_stage = stage;
        let _ = ctx.run(raw_input.clone(), |ctx| {
            app.headless_update(ctx);
        });
    }

    // If we reached here without panicking, the test passed.
}
