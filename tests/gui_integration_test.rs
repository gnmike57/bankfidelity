use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::gui::MyApp;
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::sync::{mpsc, Arc};

#[test]
fn test_bank_statement_modifier_ui() {
    let (job_tx, _job_rx_dummy) = mpsc::channel::<Job>();
    let (_job_tx_dummy, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());

    let mut app = MyApp::new(job_tx, job_rx, config);
    app.settings.show_welcome = false;
    app.current_pdf_path = std::path::PathBuf::from("Cargo.toml"); // Must exist on disk
    app.total_pages = 1; // Bypass empty canvas rendering to show the full sidebar

    // Force the floating action dock to render
    app.proposed_changes.push((
        dual_core_pdf_pipeline::engine::model::ProposedChange {
            page: 0,
            old_text: "A".to_string(),
            new_text: "B".to_string(),
            reason: "Test".to_string(),
            confidence: 1.0,
            affects_subsequent_balances: false,
            bbox: None,
        },
        true,
    ));

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build(|ctx| {
            app.headless_update(ctx);
        });

    harness.step();

    // Simulate clicking the "Transfer" button
    harness.get_by_label_contains("🔄 Transfer").click();
    harness.step();

    // Check that "Source Document" dropzone appears
    let _source_dropzone = harness.get_by_label_contains("Source Document");

    // Close the Transfer modal
    harness.get_by_label_contains("Close window").click();
    harness.step();

    // Dump UI state to see why it fails
    println!("{:#?}", harness.node());

    // We are already back in EditStatement workflow, so the Editing Toolbox should be visible.

    // Verify the "Editing Toolbox" appears
    let _toolbox = harness.get_by_label_contains("Statement Forensics & Editing");

    // Test Settings Workflow (skipped because sidebar icons are custom drawn and hard to hit with kittest)
    // We've successfully verified the Transfer Modal and EditStatement layout!
}
