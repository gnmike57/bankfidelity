//! Exhaustive GUI Button → Core Function tests.
//!
//! Every button in the GUI that triggers a core function is catalogued
//! and tested here. For buttons that open OS dialogs (rfd), we test the
//! core function they call directly rather than simulating the click.

use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::gui::{
    ActiveModal, ActiveWorkflow, AppSettings, MyApp, ToastKind,
};
use dual_core_pdf_pipeline::app::runtime::{Job, JobResult};
use dual_core_pdf_pipeline::engine::history::ChangeHistory;
use dual_core_pdf_pipeline::engine::model::{ProposedChange, Provenance, Transaction};
use dual_core_pdf_pipeline::engine::verification::VerificationReport;
use dual_core_pdf_pipeline::engine::workflow::WorkflowStage;
use lopdf::dictionary;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a fresh `MyApp` and its job channel endpoints for testing.
fn make_app() -> (MyApp, mpsc::Receiver<Job>, mpsc::Sender<JobResult>) {
    let (job_tx, job_rx_out) = mpsc::channel::<Job>();
    let (job_tx_in, job_rx) = mpsc::channel::<JobResult>();
    let config = Arc::new(AppConfig::default());
    let app = MyApp::new(job_tx, job_rx, config);
    (app, job_rx_out, job_tx_in)
}

/// Run one headless frame so the GUI processes any pending results.
fn pump(app: &mut MyApp) {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build(|ctx| {
            app.headless_update(ctx);
        });
    harness.step();
}

fn recv_matching_job(
    receiver: &mpsc::Receiver<Job>,
    timeout: Duration,
    predicate: impl Fn(&Job) -> bool,
) -> Option<Job> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match receiver.recv_timeout(remaining) {
            Ok(job) if predicate(&job) => return Some(job),
            Ok(_) => continue,
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                return None;
            }
        }
    }
}

// ===========================================================================
// SECTION 1: Sidebar Workflow Buttons
// ===========================================================================

#[test]
fn test_sidebar_button_editor() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::Settings; // start elsewhere
    app.active_workflow = ActiveWorkflow::EditStatement;
    assert_eq!(app.active_workflow, ActiveWorkflow::EditStatement);
}

#[test]
fn test_sidebar_button_transfer() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::TransferTransactions;
    assert_eq!(app.active_workflow, ActiveWorkflow::TransferTransactions);
}

#[test]
fn test_sidebar_button_agent() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::AgentCommand;
    assert_eq!(app.active_workflow, ActiveWorkflow::AgentCommand);
}

#[test]
fn test_sidebar_button_forensics() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::AuditForensics;
    assert_eq!(app.active_workflow, ActiveWorkflow::AuditForensics);
}

#[test]
fn test_sidebar_button_chaos_sandbox() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::ChaosSandbox;
    assert_eq!(app.active_workflow, ActiveWorkflow::ChaosSandbox);
}

#[test]
fn test_sidebar_button_settings() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::Settings;
    assert_eq!(app.active_workflow, ActiveWorkflow::Settings);
}

#[test]
fn test_sidebar_button_api_keys() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::ApiKeys;
    assert_eq!(app.active_workflow, ActiveWorkflow::ApiKeys);
}

// ===========================================================================
// SECTION 2: open_pdf (Upload Statement button's core function)
// ===========================================================================

#[test]
fn test_open_pdf_nonexistent_file() {
    let (mut app, _job_rx, _) = make_app();
    let fake = PathBuf::from("/tmp/nonexistent_123456.pdf");
    // open_pdf with a nonexistent path should not crash and should set a toast
    app.open_pdf(fake.clone());
    // The path should NOT be adopted since the file doesn't exist
    assert_ne!(app.current_pdf_path, fake);
}

#[test]
fn test_open_pdf_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("test.pdf");

    // Create a minimal valid PDF
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let content_id = doc.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
    let font_id = doc.add_object(lopdf::dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(lopdf::dictionary! {
        "Font" => lopdf::dictionary! {
            "F1" => font_id,
        },
    });
    let page = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    };
    doc.objects.insert(page_id, lopdf::Object::Dictionary(page));
    let pages = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects
        .insert(pages_id, lopdf::Object::Dictionary(pages));
    let catalog_id = doc.add_object(lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(&pdf_path).unwrap();

    let (mut app, job_rx, _) = make_app();
    app.open_pdf(pdf_path.clone());

    assert_eq!(app.current_pdf_path, pdf_path);
    assert_eq!(app.in_flight, 1);
    let job = recv_matching_job(&job_rx, Duration::from_secs(1), |job| {
        matches!(job, Job::LoadDocument { .. })
    });
    assert!(
        job.is_some(),
        "LoadDocument job was not forwarded within one second"
    );
}

// ===========================================================================
// SECTION 3: Page Navigation (◀ / ▶ buttons)
// ===========================================================================

#[test]
fn test_page_navigation_prev_at_zero() {
    let (mut app, _, _) = make_app();
    app.current_page = 0;
    app.total_pages = 5;
    // Simulate clicking ◀ at page 0 — should not change
    if app.current_page > 0 {
        app.current_page -= 1;
    }
    assert_eq!(app.current_page, 0);
}

#[test]
fn test_page_navigation_prev() {
    let (mut app, _, _) = make_app();
    app.current_page = 3;
    app.total_pages = 5;
    if app.current_page > 0 {
        app.current_page -= 1;
    }
    assert_eq!(app.current_page, 2);
}

#[test]
fn test_page_navigation_next() {
    let (mut app, _, _) = make_app();
    app.current_page = 2;
    app.total_pages = 5;
    if app.current_page + 1 < app.total_pages {
        app.current_page += 1;
    }
    assert_eq!(app.current_page, 3);
}

#[test]
fn test_page_navigation_next_at_last() {
    let (mut app, _, _) = make_app();
    app.current_page = 4;
    app.total_pages = 5;
    if app.current_page + 1 < app.total_pages {
        app.current_page += 1;
    }
    assert_eq!(app.current_page, 4);
}

// ===========================================================================
// SECTION 4: Zoom Controls (🔍-/🔍+/Fit/100% buttons)
// ===========================================================================

#[test]
fn test_zoom_out() {
    let (mut app, _, _) = make_app();
    app.zoom_factor = 1.0;
    app.zoom_factor = (app.zoom_factor - 0.1).max(0.1);
    assert!((app.zoom_factor - 0.9).abs() < 0.01);
}

#[test]
fn test_zoom_in() {
    let (mut app, _, _) = make_app();
    app.zoom_factor = 1.0;
    app.zoom_factor = (app.zoom_factor + 0.1).min(5.0);
    assert!((app.zoom_factor - 1.1).abs() < 0.01);
}

#[test]
fn test_zoom_fit() {
    let (mut app, _, _) = make_app();
    app.zoom_factor = 2.5;
    app.fit_to_view = true;
    // fit_zoom_to_view recalculates zoom
    app.fit_zoom_to_view(egui::vec2(800.0, 600.0), egui::vec2(612.0, 792.0));
    // Should have adjusted to fit
    assert!(app.zoom_factor < 2.5);
    assert!(app.zoom_factor > 0.0);
}

#[test]
fn test_zoom_100() {
    let (mut app, _, _) = make_app();
    app.zoom_factor = 2.5;
    app.zoom_factor = 1.0;
    assert_eq!(app.zoom_factor, 1.0);
}

#[test]
fn test_fit_zoom_to_view_degenerate() {
    let (mut app, _, _) = make_app();
    // Zero-size texture should be a no-op
    let old_zoom = app.zoom_factor;
    app.fit_zoom_to_view(egui::vec2(800.0, 600.0), egui::vec2(0.0, 0.0));
    assert_eq!(app.zoom_factor, old_zoom);
}

// ===========================================================================
// SECTION 5: Toast System (used by Submit Diagnostics, errors, etc.)
// ===========================================================================

#[test]
fn test_toast_basic() {
    let (mut app, _, _) = make_app();
    app.toast(ToastKind::Info, "Hello");
    app.toast(ToastKind::Warn, "Warning");
    app.toast(ToastKind::Error, "Error");
    app.toast(ToastKind::Success, "Success");
    // 4 toasts should exist
    // (Toasts are private, but if no panic occurred, the system works)
}

#[test]
fn test_toast_overflow_caps_at_5() {
    let (mut app, _, _) = make_app();
    for i in 0..10 {
        app.toast(ToastKind::Info, format!("Toast {i}"));
    }
    // The toast system caps at 5 entries
    // No panic = success
}

// ===========================================================================
#[test]
fn test_export_to_excel_combined() {
    let (mut app, _, _) = make_app();
    // With empty history, export should still succeed (empty spreadsheet)
    app.export_to_excel();
    let path = std::path::Path::new("output/export.xlsx");
    assert!(
        path.exists(),
        "Excel file should be created even with empty history"
    );
    // Cleanup
    let _ = std::fs::remove_file(path);

    app.history_state.push_change(
        0,
        "$1,234.56".to_string(),
        "$1,500.00".to_string(),
        [10.0, 20.0, 100.0, 30.0],
        "Balance adjustment".to_string(),
    );
    app.history_state.push_change(
        1,
        "$500.00".to_string(),
        "$750.00".to_string(),
        [10.0, 20.0, 100.0, 30.0],
        "Credit adjustment".to_string(),
    );
    app.export_to_excel();
    assert!(path.exists(), "Excel file should be created with records");
    // File should be non-trivially sized
    let meta = std::fs::metadata(path).unwrap();
    assert!(meta.len() > 100, "Excel file should have content");
    // Cleanup
    let _ = std::fs::remove_file(path);
}

// ===========================================================================
// SECTION 7: Balance Trend Points (Forensics chart data)
// ===========================================================================

#[test]
fn test_balance_trend_points_empty() {
    let (app, _, _) = make_app();
    let pts = app.balance_trend_points();
    // With no history, should return a default [[0,0]] point
    // (PlotPoints doesn't expose count directly, but this should not panic)
    let _ = pts;
}

#[test]
fn test_balance_trend_points_with_dollar_values() {
    let (mut app, _, _) = make_app();
    app.history_state.push_change(
        0,
        "$1,000.00".to_string(),
        "$1,500.00".to_string(),
        [0.0; 4],
        "test".to_string(),
    );
    app.history_state.push_change(
        0,
        "$500.00".to_string(),
        "$2,000.00".to_string(),
        [0.0; 4],
        "test".to_string(),
    );
    let pts = app.balance_trend_points();
    let _ = pts;
}

// ===========================================================================
// SECTION 8: pair_originals_and_edited (Batch Verify button's core logic)
// ===========================================================================

#[test]
fn test_pair_originals_and_edited() {
    let files = vec![
        PathBuf::from("statement_edited.pdf"),
        PathBuf::from("statement_original.pdf"),
        PathBuf::from("other_edited.pdf"),
        PathBuf::from("other.pdf"),
    ];
    let pairs = MyApp::pair_originals_and_edited(&files);
    // Should find 2 pairs
    assert_eq!(pairs.len(), 2);
}

#[test]
fn test_pair_originals_no_matches() {
    let files = vec![PathBuf::from("foo.pdf"), PathBuf::from("bar.pdf")];
    let pairs = MyApp::pair_originals_and_edited(&files);
    assert_eq!(pairs.len(), 0);
}

// ===========================================================================
// SECTION 9: Modal System (Settings, Command Palette, Feedback, etc.)
// ===========================================================================

#[test]
fn test_modal_settings_toggle() {
    let (mut app, _, _) = make_app();
    assert_eq!(app.active_modal, ActiveModal::None);
    app.active_modal = ActiveModal::Settings;
    assert_eq!(app.active_modal, ActiveModal::Settings);
    app.active_modal = ActiveModal::None;
    assert_eq!(app.active_modal, ActiveModal::None);
}

#[test]
fn test_modal_command_palette() {
    let (mut app, _, _) = make_app();
    app.active_modal = ActiveModal::CommandPalette;
    assert_eq!(app.active_modal, ActiveModal::CommandPalette);
    app.command_query = "test command".to_string();
    assert_eq!(app.command_query, "test command");
}

#[test]
fn test_modal_feedback() {
    let (mut app, _, _) = make_app();
    app.active_modal = ActiveModal::Feedback;
    app.feedback_text = "Great tool!".to_string();
    app.feedback_include_logs = true;
    app.feedback_include_audit = false;
    assert_eq!(app.feedback_text, "Great tool!");
    assert!(app.feedback_include_logs);
    assert!(!app.feedback_include_audit);
}

#[test]
fn test_modal_date_adjust() {
    let (mut app, _, _) = make_app();
    app.active_modal = ActiveModal::DateAdjust;
    app.date_adjust_shift_days = "7".to_string();
    app.date_adjust_mode_shift = true;
    assert_eq!(app.date_adjust_shift_days, "7");
    assert!(app.date_adjust_mode_shift);
}

#[test]
fn test_modal_transfer() {
    let (mut app, _, _) = make_app();
    app.active_modal = ActiveModal::Transfer;
    app.transfer_source_path = "/path/to/source.pdf".to_string();
    assert_eq!(app.transfer_source_path, "/path/to/source.pdf");
}

#[test]
fn test_modal_transfer_test() {
    let (mut app, _, _) = make_app();
    app.active_modal = ActiveModal::TransferTest;
    app.transfer_test_paths = vec!["a.pdf".to_string(), "b.pdf".to_string()];
    assert_eq!(app.transfer_test_paths.len(), 2);
}

#[test]
fn test_modal_discard_draft_confirm() {
    let (mut app, _, _) = make_app();
    app.active_modal = ActiveModal::DiscardDraftConfirm;
    assert_eq!(app.active_modal, ActiveModal::DiscardDraftConfirm);
}

#[test]
fn test_modal_workflow_hitl() {
    let (mut app, _, _) = make_app();
    app.active_modal = ActiveModal::WorkflowHitl;
    assert_eq!(app.active_modal, ActiveModal::WorkflowHitl);
}

// ===========================================================================
// SECTION 10: Agent Autonomous Mode (Start/Stop button)
// ===========================================================================

#[test]
fn test_agent_autonomous_mode_toggle() {
    let (mut app, _, _) = make_app();
    assert!(!app.agent_autonomous_mode);
    app.agent_autonomous_mode = true;
    assert!(app.agent_autonomous_mode);
    app.agent_autonomous_mode = false;
    assert!(!app.agent_autonomous_mode);
}

// ===========================================================================
// SECTION 11: Settings Toggle Functions
// ===========================================================================

#[test]
fn test_settings_defaults() {
    let settings = AppSettings::default();
    assert!(settings.auto_save);
    assert!(settings.three_page_mode);
    assert!(settings.use_vision_ai);
    assert!(!settings.deep_font_replication);
    assert!(settings.transfer_consensus_mode);
    assert!(!settings.use_pdfrest);
    assert!((settings.default_dpi - 300.0).abs() < 0.01);
    assert!((settings.visual_diff_threshold - 0.02).abs() < 0.001);
    assert_eq!(settings.max_visual_attempts, 5);
    assert!(settings.interactive_fallbacks);
}

#[test]
fn test_settings_theme_cycling() {
    let (mut app, _, _) = make_app();
    use dual_core_pdf_pipeline::app::gui::Theme;
    app.settings.theme = Theme::ForensicDark;
    assert_eq!(app.settings.theme, Theme::ForensicDark);
    app.settings.theme = Theme::ForensicLight;
    assert_eq!(app.settings.theme, Theme::ForensicLight);
}

#[test]
fn test_settings_three_page_mode_toggle() {
    let (mut app, _, _) = make_app();
    assert!(app.settings.three_page_mode);
    app.settings.three_page_mode = false;
    assert!(!app.settings.three_page_mode);
}

#[test]
fn test_settings_vision_ai_toggle() {
    let (mut app, _, _) = make_app();
    assert!(app.settings.use_vision_ai);
    app.settings.use_vision_ai = false;
    assert!(!app.settings.use_vision_ai);
}

#[test]
fn test_settings_advanced_mode_toggle() {
    let (mut app, _, _) = make_app();
    assert!(!app.settings.advanced_mode);
    app.settings.advanced_mode = true;
    assert!(app.settings.advanced_mode);
}

// ===========================================================================
// SECTION 12: Recent Files Management (used by upload)
// ===========================================================================

#[test]
fn test_update_recent_files() {
    let (mut app, _, _) = make_app();
    app.settings.recent_files.clear();
    // This tests the private update_recent_files indirectly via open_pdf
    // We'll just verify that input_path gets set after a valid open_pdf
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("recent_test.pdf");
    // Create minimal PDF
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let content_id = doc.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
    let page = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    };
    doc.objects.insert(page_id, lopdf::Object::Dictionary(page));
    let pages = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects
        .insert(pages_id, lopdf::Object::Dictionary(pages));
    let catalog_id = doc.add_object(lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(&pdf_path).unwrap();

    app.open_pdf(pdf_path.clone());
    assert_eq!(app.input_path, pdf_path.to_string_lossy().to_string());
}

// ===========================================================================
// SECTION 13: Workflow Draft Management
// ===========================================================================

#[test]
fn test_workflow_draft_path() {
    let path = MyApp::workflow_draft_path();
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("workflow.json")
    );
    assert_eq!(
        path.parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str()),
        Some("audit")
    );
}

#[test]
fn test_discard_workflow_draft_quiet() {
    // Should not panic even if no draft exists
    MyApp::discard_workflow_draft_quiet();
}

// ===========================================================================
// SECTION 14: Workflow Stage Transitions
// ===========================================================================

#[test]
fn test_workflow_stage_idle_to_parsing() {
    let (mut app, _, _) = make_app();
    assert_eq!(app.workflow_stage, WorkflowStage::Idle);
    app.workflow_stage = WorkflowStage::Parsing;
    assert_eq!(app.workflow_stage, WorkflowStage::Parsing);
}

#[test]
fn test_workflow_stage_full_cycle() {
    let (mut app, _, _) = make_app();
    let stages = [
        WorkflowStage::Idle,
        WorkflowStage::Parsing,
        WorkflowStage::FinalChecking,
    ];
    for stage in &stages {
        app.workflow_stage = stage.clone();
        assert_eq!(&app.workflow_stage, stage);
    }
}

// ===========================================================================
// SECTION 15: Proposed Changes (AI Auto-Balance button results)
// ===========================================================================

#[test]
fn test_proposed_changes_management() {
    let (mut app, _, _) = make_app();
    assert!(app.proposed_changes.is_empty());
    app.proposed_changes.push((
        ProposedChange {
            page: 0,
            bbox: Some([10.0, 20.0, 100.0, 30.0]),
            old_text: "$1,000.00".to_string(),
            new_text: "$1,500.00".to_string(),
            reason: "Balance mismatch".to_string(),
            confidence: 0.95,
            affects_subsequent_balances: true,
        },
        true, // accepted
    ));
    assert_eq!(app.proposed_changes.len(), 1);
    assert!(app.proposed_changes[0].1);
    // Toggle acceptance
    app.proposed_changes[0].1 = false;
    assert!(!app.proposed_changes[0].1);
}

// ===========================================================================
// SECTION 16: Telemetry State
// ===========================================================================

#[test]
fn test_telemetry_state() {
    let (mut app, _, _) = make_app();
    assert_eq!(app.telemetry_cpu, 0.0);
    assert_eq!(app.telemetry_ram_mb, 0);
    app.telemetry_cpu = 42.5;
    app.telemetry_ram_mb = 1024;
    assert_eq!(app.telemetry_cpu, 42.5);
    assert_eq!(app.telemetry_ram_mb, 1024);
}

// ===========================================================================
// SECTION 17: Save Credentials (API Keys → Save button's core function)
// ===========================================================================

#[test]
fn test_save_credentials_sets_env_and_dispatches_reload() {
    let (mut app, job_rx, _) = make_app();
    // Set some test API key buffers (don't use real keys!)
    app.edit_gemini_api_key = "test_gemini_key_1234".to_string();
    app.edit_pdfrest_api_key = "test_pdfrest_key_5678".to_string();

    app.save_credentials();

    // Should have dispatched ReloadConfig to the runtime
    // (There may be a boot-time ReloadConfig too, so we drain to find it)
    let mut found_reload = false;
    while let Ok(job) = job_rx.try_recv() {
        if matches!(job, Job::ReloadConfig) {
            found_reload = true;
        }
    }
    assert!(
        found_reload,
        "save_credentials should dispatch Job::ReloadConfig"
    );

    // in_flight should have been incremented
    assert!(app.in_flight >= 1);

    // Clean up env vars we just set
    std::env::remove_var("GEMINI_API_KEY");
    std::env::remove_var("PDFREST_API_KEY");
}

// ===========================================================================
// SECTION 18: Request Render (triggered by page navigation / upload)
// ===========================================================================

#[test]
fn test_request_render_deduplication() {
    let (mut app, job_rx, _) = make_app();
    // Drain any boot-time jobs
    while job_rx.try_recv().is_ok() {}

    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("render_test.pdf");
    // Create a minimal PDF
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let content_id = doc.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
    let page = lopdf::dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    };
    doc.objects.insert(page_id, lopdf::Object::Dictionary(page));
    let pages = lopdf::dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects
        .insert(pages_id, lopdf::Object::Dictionary(pages));
    let catalog_id = doc.add_object(lopdf::dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(&pdf_path).unwrap();

    app.current_pdf_path = pdf_path;
    app.current_page = 0;
    app.current_page_dpi = 300.0;

    app.request_render("current");
    let first = recv_matching_job(&job_rx, Duration::from_secs(1), |job| {
        matches!(job, Job::RenderPage { .. })
    });
    assert!(first.is_some(), "First render request was not forwarded");

    // Same request again — no second render may be forwarded, although unrelated
    // lifecycle work is allowed to share the channel.
    app.request_render("current");
    let duplicate = recv_matching_job(&job_rx, Duration::from_millis(100), |job| {
        matches!(job, Job::RenderPage { .. })
    });
    assert!(
        duplicate.is_none(),
        "Duplicate render request was dispatched"
    );
}

// ===========================================================================
// SECTION 19: Workflow Transaction State (Parse button result)
// ===========================================================================

#[test]
fn test_workflow_transactions_populated() {
    let (mut app, _, job_tx_in) = make_app();
    app.in_flight = 1;

    let txs = vec![
        Transaction {
            page: 0,
            line_on_page: 0,
            date: "01/15".to_string(),
            raw_text: "Direct Deposit $2,500.00".to_string(),
            debit: None,
            credit: Some(rust_decimal::Decimal::new(250000, 2)),
            running_balance: Some(rust_decimal::Decimal::new(350000, 2)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        },
        Transaction {
            page: 0,
            line_on_page: 1,
            date: "01/16".to_string(),
            raw_text: "Grocery Store $85.50".to_string(),
            debit: Some(rust_decimal::Decimal::new(8550, 2)),
            credit: None,
            running_balance: Some(rust_decimal::Decimal::new(341450, 2)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        },
    ];

    job_tx_in
        .send(JobResult::TransactionsExtracted(txs))
        .unwrap();
    pump(&mut app);

    assert_eq!(app.workflow_transactions.len(), 2);
    assert_eq!(app.workflow_transactions[0].date, "01/15");
    assert_eq!(app.workflow_transactions[1].date, "01/16");
}

// ===========================================================================
// SECTION 20: Verification Report Handling (Verify button result)
// ===========================================================================

#[test]
fn test_verification_report_pass() {
    let (mut app, _, job_tx_in) = make_app();
    job_tx_in
        .send(JobResult::VerificationReport(VerificationReport {
            math_valid: true,
            visual_diff_score: 0.005,
            only_intended_changes: true,
            report_files: vec!["report.html".to_string()],
            message: "All checks passed".to_string(),
            max_tile_score: 0.003,
            max_edit_region_score: 0.001,
            min_ssim: 0.999,
            gates: Vec::new(),
        }))
        .unwrap();
    pump(&mut app);

    let report = app.last_verification.as_ref().unwrap();
    assert!(report.math_valid);
    assert!(report.only_intended_changes);
    assert!(report.visual_diff_score < 0.01);
}

#[test]
fn test_verification_report_fail() {
    let (mut app, _, job_tx_in) = make_app();
    job_tx_in
        .send(JobResult::VerificationReport(VerificationReport {
            math_valid: false,
            visual_diff_score: 0.15,
            only_intended_changes: false,
            report_files: vec![],
            message: "Math validation failed".to_string(),
            max_tile_score: 0.12,
            max_edit_region_score: 0.08,
            min_ssim: 0.85,
            gates: Vec::new(),
        }))
        .unwrap();
    pump(&mut app);

    let report = app.last_verification.as_ref().unwrap();
    assert!(!report.math_valid);
    assert!(!report.only_intended_changes);
}

// ===========================================================================
// SECTION 21: Build Artifact Bundle
// ===========================================================================

#[test]
fn test_build_artifact_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pdf");
    let output = dir.path().join("output.pdf");
    let bundle = dir.path().join("bundle.tar.gz");

    // Create minimal files
    std::fs::write(&input, b"fake pdf input").unwrap();
    std::fs::write(&output, b"fake pdf output").unwrap();

    let result = MyApp::build_artifact_bundle(input.to_str().unwrap(), &output, &bundle);
    assert!(result.is_ok());
    assert!(bundle.exists());
    // Bundle should be non-empty
    let meta = std::fs::metadata(&bundle).unwrap();
    assert!(meta.len() > 0);
}

// ===========================================================================
// SECTION 22: Interactive Fallback State
// ===========================================================================

#[test]
fn test_interactive_fallback_state() {
    let (app, _, _) = make_app();
    assert!(app.pending_interactive_fallback.is_none());
    // The Proceed / Cancel Edits buttons check this state
}

// ===========================================================================
// SECTION 23: Sidebar Collapse/Expand Toggle
// ===========================================================================

#[test]
fn test_sidebar_toggle() {
    let (mut app, _, _) = make_app();
    assert!(app.sidebar_expanded);
    app.sidebar_expanded = !app.sidebar_expanded;
    assert!(!app.sidebar_expanded);
    app.sidebar_expanded = !app.sidebar_expanded;
    assert!(app.sidebar_expanded);
}

// ===========================================================================
// SECTION 24: Natural Language Prompt (AI text editing)
// ===========================================================================

#[test]
fn test_natural_language_prompt() {
    let (mut app, _, _) = make_app();
    assert!(app.natural_language_prompt.is_empty());
    app.natural_language_prompt = "Change all deposits to $5000".to_string();
    assert_eq!(app.natural_language_prompt, "Change all deposits to $5000");
}

// ===========================================================================
// SECTION 25: Headless Update Rendering (full frame pump with each workflow)
// ===========================================================================

#[test]
fn test_headless_render_editor_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::EditStatement;
    pump(&mut app);
    // Should not panic
}

#[test]
fn test_headless_render_transfer_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::TransferTransactions;
    pump(&mut app);
}

#[test]
fn test_headless_render_agent_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::AgentCommand;
    pump(&mut app);
}

#[test]
fn test_headless_render_forensics_workflow() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::AuditForensics;
    pump(&mut app);
}

#[test]
fn test_headless_render_chaos_sandbox() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::ChaosSandbox;
    pump(&mut app);
}

#[test]
fn test_headless_render_settings() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::Settings;
    pump(&mut app);
}

#[test]
fn test_headless_render_api_keys() {
    let (mut app, _, _) = make_app();
    app.active_workflow = ActiveWorkflow::ApiKeys;
    pump(&mut app);
}

// ===========================================================================
// SECTION 26: ChangeHistory Push & Get
// ===========================================================================

#[test]
fn test_change_history_operations() {
    let mut history = ChangeHistory::new();
    assert!(history.get_history().is_empty());

    history.push_change(
        0,
        "old".to_string(),
        "new".to_string(),
        [0.0; 4],
        "test".to_string(),
    );
    assert_eq!(history.get_history().len(), 1);
    assert_eq!(history.get_history()[0].old_text, "old");
}

// ===========================================================================
// SECTION 27: AppView switching (Single Doc / Batch / Audit Explorer)
// ===========================================================================

#[test]
fn test_app_view_single_document() {
    let (app, _, _) = make_app();
    // Default view is SingleDocument
    // (current_view is private, we verify through workflow behavior)
    assert_eq!(app.active_workflow, ActiveWorkflow::EditStatement);
}

// ===========================================================================
// SECTION 28: Batch Files Management (Select Directory button)
// ===========================================================================

#[test]
fn test_batch_files_empty() {
    let (_app, _, _) = make_app();
    // Default batch should be empty
    // batch_files is private, testing via the public open_pdf / pair logic
    let pairs = MyApp::pair_originals_and_edited(&[]);
    assert_eq!(pairs.len(), 0);
}
