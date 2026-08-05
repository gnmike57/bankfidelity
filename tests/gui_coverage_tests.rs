use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::gui::{ActiveModal, ActiveWorkflow, AppView, MyApp};
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult};
use egui_kittest::kittest::Queryable;
use std::sync::{mpsc, Arc};

fn make_app() -> (MyApp, mpsc::Receiver<Job>, mpsc::Sender<JobResult>) {
    let (job_tx, job_rx_out) = mpsc::channel::<Job>();
    let (job_tx_in, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let mut app = MyApp::new(job_tx, job_rx, config);

    // Setup some dummy state to enable more UI elements
    app.input_path = "dummy_input.pdf".to_string();
    app.output_path = "dummy_output.pdf".to_string();
    app.current_page = 0;
    app.total_pages = 5;

    (app, job_rx_out, job_tx_in)
}

#[allow(dead_code)]
fn try_click_labels(harness: &mut egui_kittest::Harness, labels: &[&str]) {
    for &label in labels {
        if let Some(btn) = harness.get_all_by_label_contains(label).next() {
            btn.click();
        }
        harness.step();
    }
}

#[test]
fn test_overkill_modals_coverage() {
    let (mut app, _, _) = make_app();

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

    let _common_buttons = [
        "Cancel",
        "Confirm",
        "Save",
        "Submit",
        "Close",
        "Export",
        "Discard",
        "Yes",
        "No",
        "Ok",
        "Start",
        "Stop",
        "Retry",
        "Delete",
        "Clear",
        "Update",
        "Transfer",
        "Add",
        "Re-analyze",
        "Run",
        "Continue",
    ];

    for modal in modals {
        app.active_modal = modal;
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });

        harness.step();

        // Step a few more times for any animations or state updates
        for _ in 0..3 {
            harness.step();
        }
    }
}

#[test]
fn test_overkill_workflows_coverage() {
    let (mut app, _, _) = make_app();

    let workflows = vec![
        ActiveWorkflow::EditStatement,
        ActiveWorkflow::TransferTransactions,
        ActiveWorkflow::AgentCommand,
        ActiveWorkflow::AuditForensics,
        ActiveWorkflow::ChaosSandbox,
        ActiveWorkflow::Settings,
        ActiveWorkflow::ApiKeys,
    ];

    let _common_buttons = [
        "Parse",
        "Fit",
        "100%",
        "🔍+",
        "🔍-",
        "Run Chaos Suite",
        "Submit Diagnostics",
        "Execute",
        "Start",
        "Stop",
        "▶",
        "◀",
        "🏷 Auto-Categorize",
        "🔄 Re-analyze",
        "🔄 Parse",
        "Return to Edit Review",
        "Cancel Workflow",
        "📂 Select Directory",
        "Save",
        "Clear",
        "Export",
        "Transfer",
        "Close",
        "Add",
        "Remove",
        "Review",
        "Apply",
        "Confirm",
        "Revert",
        "Undo",
        "Redo",
    ];

    let views = [
        AppView::SingleDocument,
        AppView::BatchProcessing,
        AppView::AuditExplorer,
    ];

    for _view in views {
        for workflow in &workflows {
            app.active_workflow = workflow.clone();

            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1920.0, 1080.0))
                .build(|ctx| {
                    app.headless_update(ctx);
                });

            harness.step();

            // Try pressing some keys that might trigger shortcuts
            harness.press_key(egui::Key::Enter);
            harness.step();
            harness.press_key(egui::Key::Escape);
            harness.step();
        }
    }
}

#[test]
fn test_overkill_edge_cases() {
    let (mut app, _, _) = make_app();

    // Force specific edge cases
    app.active_workflow = ActiveWorkflow::EditStatement;
    app.total_pages = 0; // Empty document
    app.input_path = "".to_string(); // No file selected
    app.output_path = "".to_string();

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();

        harness.step();
    }

    // Now with valid data
    app.input_path = "valid.pdf".to_string();
    app.total_pages = 10;
    app.current_page = 9; // Last page

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }

    app.current_page = 0; // First page

    {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1920.0, 1080.0))
            .build(|ctx| {
                app.headless_update(ctx);
            });
        harness.step();
    }
}
