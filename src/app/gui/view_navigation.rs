//! Document opening and keyboard shortcut navigation.
#![allow(unused_imports)]

use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::gui::state::{ActiveModal, AppView, MyApp, ToastKind};
use crate::app::runtime::Job;
use crate::engine::history::ChangeHistory;

impl MyApp {
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        // Read individual shortcut states instead of cloning the entire InputState.
        let ctrl_o = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O));
        let ctrl_z = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z));
        let ctrl_y = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Y));
        let ctrl_s = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S));
        let page_down = ctx
            .input(|i| i.key_pressed(egui::Key::PageDown) || i.key_pressed(egui::Key::ArrowRight));
        let page_up =
            ctx.input(|i| i.key_pressed(egui::Key::PageUp) || i.key_pressed(egui::Key::ArrowLeft));
        let zoom_in =
            ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
        let zoom_out = ctx.input(|i| i.key_pressed(egui::Key::Minus));
        let zoom_reset = ctx.input(|i| i.key_pressed(egui::Key::Num0));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        if escape {
            self.active_modal = ActiveModal::None;
            self.active_modal = ActiveModal::None;
            self.active_modal = ActiveModal::None;
            self.active_modal = ActiveModal::None;
        }

        if ctrl_o {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .pick_file()
            {
                self.open_pdf(path);
            }
        }
        if ctrl_z {
            if let Err(e) = self.job_tx.send(Job::Undo) {
                tracing::error!("Runtime disconnected: {}", e);
            }
        }
        if ctrl_y {
            if let Err(e) = self.job_tx.send(Job::Redo) {
                tracing::error!("Runtime disconnected: {}", e);
            }
        }
        if ctrl_s {
            if let Err(e) = self.job_tx.send(Job::ExportChangeHistory {
                output: PathBuf::from(&self.export_path),
            }) {
                tracing::error!("Runtime disconnected: {}", e);
            }
        }
        if page_down && self.current_page + 1 < self.total_pages {
            self.current_page += 1;
            self.request_render("current");
        }
        if page_up && self.current_page > 0 {
            self.current_page -= 1;
            self.request_render("current");
        }
        if zoom_in {
            self.zoom_factor = (self.zoom_factor * 1.15).clamp(0.1, 5.0);
            self.fit_to_view = false;
        }
        if zoom_out {
            self.zoom_factor = (self.zoom_factor * 0.85).clamp(0.1, 5.0);
            self.fit_to_view = false;
        }
        if zoom_reset {
            self.zoom_factor = 1.0;
            self.pan_offset = egui::Vec2::ZERO;
            self.fit_to_view = false;
        }
    }

    pub fn open_pdf(&mut self, path: PathBuf) {
        if !path.exists() {
            self.toast(
                ToastKind::Error,
                format!("File not found: {}", path.display()),
            );
            return;
        }
        self.input_path = path.to_string_lossy().to_string();
        self.activate_run_workspace(&path);
        self.current_pdf_path = path.clone();
        self.previous_pdf_path = None;
        self.history_state = ChangeHistory::new();
        self.proposed_changes.clear();
        self.last_imbalance = None;
        self.last_verification = None;
        self.last_warning = None;
        self.selected_block = None;
        // Stage 6: opening a new PDF invalidates any cached hash and any
        // in-flight workflow buffers - those belong to the previous file.
        self.workflow_input_hash = None;
        self.workflow_cell_buffers.clear();
        // Stage 8.5: clear the font analysis; the runtime will produce a
        // fresh one for the new PDF.
        self.font_analysis = None;
        if let Err(e) = self.job_tx.send(Job::LoadDocument {
            path: self.current_pdf_path.clone(),
            three_page_mode: self.settings.three_page_mode,
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
    }
}
