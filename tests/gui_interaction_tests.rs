use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::gui::{ActiveModal, ActiveWorkflow, MyApp};
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult};
use std::sync::{mpsc, Arc};

fn make_app() -> (MyApp, mpsc::Receiver<Job>, mpsc::Sender<JobResult>) {
    let (job_tx, job_rx_out) = mpsc::channel::<Job>();
    let (job_tx_in, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let app = MyApp::new(job_tx, job_rx, config);
    (app, job_rx_out, job_tx_in)
}

fn pump(app: &mut MyApp, _test_name: &str) {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build(|ctx| {
            app.headless_update(ctx);
        });
    harness.step();
}

#[test]
fn test_draw_settings_and_font_analysis() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::Settings;

    // Simulate some font analysis data
    app.status = "Testing".to_string();

    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build(|ctx| {
            app.headless_update(ctx);
        });
    harness.step();

    // Click "Re-analyze" in font analysis when None
    // if let Some(btn) = harness.try_get_by_label("Re-analyze") {
    //     btn.click();
    // }
    harness.step();
}

#[test]
fn test_draw_api_keys() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::ApiKeys;
    pump(&mut app, "api_keys");
}

#[test]
fn test_draw_editor_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::EditStatement;
    pump(&mut app, "editor_workflow");
}

#[test]
fn test_draw_transfer_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::TransferTransactions;
    pump(&mut app, "transfer_workflow");
}

#[test]
fn test_draw_agent_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::AgentCommand;
    pump(&mut app, "agent_workflow");
}

#[test]
fn test_draw_forensics_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::AuditForensics;
    pump(&mut app, "forensics_workflow");
}

#[test]
fn test_draw_chaos_sandbox() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::ChaosSandbox;
    pump(&mut app, "chaos_sandbox");
}

#[test]
fn test_modals() {
    let (mut app, _, _) = make_app();

    // Test Discard Draft Modal
    app.active_modal = ActiveModal::DiscardDraftConfirm;
    pump(&mut app, "discard_draft_modal");

    // Test Command Palette
    app.active_modal = ActiveModal::CommandPalette;
    pump(&mut app, "command_palette_modal");

    // Test Date Adjust
    app.active_modal = ActiveModal::DateAdjust;
    pump(&mut app, "date_adjust_modal");

    // Test Transfer Modal
    app.active_modal = ActiveModal::Transfer;
    pump(&mut app, "transfer_modal");

    // Test Feedback Modal
    app.active_modal = ActiveModal::Feedback;
    pump(&mut app, "feedback_modal");
}
