//! Workflow and table editing views.
#![allow(unused_imports)]

use eframe::egui;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::app::gui::state::{
    ActiveModal, ActiveWorkflow, AppModals, AppSettings, AppView, MyApp, Theme, ToastKind,
};
use crate::app::runtime::{Job, JobId, RuntimeSubmitError};
use crate::engine::history::ChangeHistory;
use crate::engine::verification::VerificationReport;
use egui_plot::PlotPoints;

impl MyApp {
    pub(crate) fn draw_sidebar(&mut self, ctx: &egui::Context) {
        // Animate the sidebar width for a buttery smooth expansion
        let target_width = if self.sidebar_expanded { 240.0 } else { 70.0 };
        let width =
            ctx.animate_value_with_time(egui::Id::new("sidebar_width_anim"), target_width, 0.3);

        let frame = egui::Frame {
            inner_margin: egui::Margin::same(12.0),
            rounding: egui::Rounding {
                nw: 0.0,
                sw: 0.0,
                ne: 24.0,
                se: 24.0,
            },
            fill: ctx.style().visuals.window_fill.linear_multiply(0.95), // Slight translucency
            stroke: egui::Stroke::new(1.0_f32, ctx.style().visuals.widgets.inactive.bg_fill),
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 8.0),
                blur: 16.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(80),
            },
            ..Default::default()
        };

        egui::SidePanel::left("sidebar")
            .frame(frame)
            .exact_width(width)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(16.0);

                // Toggle Button (Hamburger)
                ui.horizontal(|ui| {
                    let toggle_text = if self.sidebar_expanded {
                        "≡  Collapse"
                    } else {
                        "≡"
                    };
                    let btn =
                        egui::Button::new(egui::RichText::new(toggle_text).size(18.0).strong())
                            .frame(false)
                            .min_size(egui::vec2(ui.available_width(), 40.0));

                    if ui.add(btn).clicked() {
                        self.sidebar_expanded = !self.sidebar_expanded;
                    }
                });

                ui.add_space(32.0);

                let mut selected = self.active_workflow.clone();
                let workflows = [
                    (ActiveWorkflow::EditStatement, "📄", "Editor"),
                    (ActiveWorkflow::TransferTransactions, "⇄", "Transfer"),
                    (ActiveWorkflow::AgentCommand, "⌘", "Commands"),
                    (ActiveWorkflow::Settings, "⚙", "Settings"),
                    (ActiveWorkflow::ApiKeys, "🔑", "API Keys"),
                ];

                for (workflow, icon, text) in workflows {
                    let is_selected = self.active_workflow == workflow;

                    // Custom pill-shaped active state
                    let bg_color = if is_selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let text_color = if is_selected {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().text_color()
                    };

                    let btn_text = if width > 120.0 {
                        format!("{}  {}", icon, text)
                    } else {
                        icon.to_string()
                    };

                    let response = ui.allocate_rect(
                        egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), 48.0),
                        ),
                        egui::Sense::click(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &btn_text)
                    });

                    // Hover animation
                    let hover_factor =
                        ctx.animate_bool(response.id.with("hover"), response.hovered());
                    let final_bg = if is_selected {
                        bg_color
                    } else {
                        ui.visuals()
                            .widgets
                            .hovered
                            .bg_fill
                            .linear_multiply(hover_factor)
                    };

                    // Draw the custom button
                    ui.painter().rect(
                        response.rect,
                        egui::Rounding::same(12.0),
                        final_bg,
                        egui::Stroke::NONE,
                    );

                    // Draw the text
                    let text_pos = response.rect.min
                        + egui::vec2(
                            if width > 120.0 {
                                16.0
                            } else {
                                (width - 24.0) / 2.0
                            },
                            14.0,
                        );
                    ui.painter().text(
                        text_pos,
                        egui::Align2::LEFT_TOP,
                        btn_text,
                        egui::FontId::new(16.0, egui::FontFamily::Proportional),
                        text_color,
                    );

                    if response.clicked() {
                        selected = workflow;
                    }

                    ui.advance_cursor_after_rect(response.rect);
                    ui.add_space(8.0);
                }

                self.active_workflow = selected;

                // Bottom anchored branding
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(16.0);
                    let opacity = ctx.animate_value_with_time(
                        egui::Id::new("sidebar_brand_anim"),
                        if self.sidebar_expanded { 1.0 } else { 0.0 },
                        0.2,
                    );
                    if opacity > 0.1 {
                        ui.label(
                            egui::RichText::new("Antigravity\nStatement Forensics")
                                .size(12.0)
                                .color(ui.visuals().text_color().linear_multiply(0.4 * opacity)),
                        );
                    }
                });
            });
    }

    pub(crate) fn draw_edit_statement_workflow(&mut self, ctx: &egui::Context) {
        let frame = egui::Frame {
            inner_margin: egui::Margin::same(16.0),
            fill: ctx.style().visuals.panel_fill,
            stroke: egui::Stroke::NONE,
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 8.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(40),
            },
            ..Default::default()
        };

        // Streaming Logs Drawer (if UFO is running or logs exist)
        if !self.ufo_logs.is_empty() {
            egui::TopBottomPanel::top("ufo_logs_panel")
                .resizable(true)
                .min_height(100.0)
                .max_height(300.0)
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::from_gray(30))
                        .inner_margin(8.0),
                )
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new("UFO Agent Logs")
                            .strong()
                            .color(egui::Color32::LIGHT_GREEN),
                    );
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let text = self.ufo_logs.join("\n");
                            ui.add(
                                egui::TextEdit::multiline(&mut text.as_str())
                                    .font(egui::TextStyle::Monospace)
                                    .text_color(egui::Color32::from_gray(200))
                                    .desired_width(f32::INFINITY)
                                    .interactive(false),
                            );
                        });
                });
        }

        // 1. Top Bar: Upload Dropzone & History Thumbnails
        egui::TopBottomPanel::top("edit_top_bar")
            .frame(frame)
            .exact_height(90.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Modern Upload Button
                    let upload_btn = ui.add_sized(
                        [160.0, 58.0],
                        egui::Button::new(
                            egui::RichText::new("📥  Upload Statement")
                                .size(15.0)
                                .strong(),
                        )
                        .fill(ui.visuals().selection.bg_fill),
                    );

                    if upload_btn.clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("PDF", &["pdf"])
                            .pick_file()
                        {
                            self.open_pdf(path);
                        }
                    }

                    ui.add_space(10.0);

                    // 🪄 📄 Auto-Edit (UFO) Button
                    let auto_edit_btn = ui.add_sized(
                        [180.0, 58.0],
                        egui::Button::new(
                            egui::RichText::new("🪄 📄 Auto-Edit (UFO)")
                                .size(15.0)
                                .strong()
                                .color(self.settings.theme.palette().bg),
                        )
                        .fill(self.settings.theme.palette().accent), // Use accent color for high visibility
                    );

                    if auto_edit_btn.clicked() {
                        if self.current_pdf_path.exists() {
                            let mut context = "BankFidelity State Context:\n".to_string();
                            context.push_str(&format!(
                                "Total Transactions loaded: {}\n",
                                self.workflow_transactions.len()
                            ));
                            if let Some(err) = &self.last_imbalance {
                                context.push_str(&format!(
                                    "Current Error: Statement is out of balance. Difference: {:.2}\n",
                                    err
                                ));
                            } else if let Some(verification) = &self.last_verification {
                                context.push_str(&format!(
                                    "Verification State: {verification:#?}\n"
                                ));
                            }

                            self.ufo_logs.clear();
                            match self.dispatch_workflow_job(Job::UfoAutoEdit {
                                path: self.current_pdf_path.clone(),
                                context,
                            }) {
                                Ok(_) => {
                                    self.is_ufo_running = true;
                                    self.in_flight += 1;
                                }
                                Err(e) => {
                                    self.is_ufo_running = false;
                                    tracing::error!("Failed to dispatch Auto-Edit: {e}");
                                    self.toast(
                                        ToastKind::Error,
                                        format!("Failed to start UFO Auto-Edit: {e}"),
                                    );
                                }
                            }
                        } else {
                            self.toast(ToastKind::Warn, "Please open a statement first.");
                        }
                    }

                    if self.is_ufo_running {
                        ui.add_space(10.0);
                        let cancel_btn = ui.add_sized(
                            [120.0, 58.0],
                            egui::Button::new(
                                egui::RichText::new("🛑 Cancel")
                                    .size(15.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(220, 50, 50)),
                        );

                        if cancel_btn.clicked() {
                            let _ = self.dispatch_workflow_job(Job::CancelUfo);
                            // Clear local busy flags immediately. Do NOT touch
                            // in_flight here: CancelUfo kills the process and
                            // the UfoAutoEdit spawn always emits Error or
                            // UfoAutoEditResult, which frees exactly one wait
                            // slot via ends_gui_tracked_job. Early decrement
                            // would double-count when that terminal result arrives.
                            self.is_ufo_running = false;
                            self.ufo_user_cancelled = true;
                            self.progress = None;
                            self.toast(ToastKind::Warn, "UFO Task Cancelled.");
                        }
                    }

                    ui.add_space(20.0);
                    let sep = egui::Separator::default().vertical().spacing(30.0);
                    ui.add(sep);

                    if self.current_pdf_path.exists() {
                        // Visual Progress Stepper (Stage 9)
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Workflow Progress")
                                    .color(ui.visuals().text_color().linear_multiply(0.6))
                                    .size(12.0),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                let steps =
                                    ["1. Load", "2. Edit & Diff", "3. Verify Math", "4. Export"];
                                // Determine current step based on state
                                let current_step = if self.last_verification.is_some() {
                                    3 // Validated, ready to export
                                } else if !self.workflow_transactions.is_empty() {
                                    2 // Edited/Balanced, pending validation
                                } else {
                                    1 // Loaded, pending edits
                                };

                                for (i, step) in steps.iter().enumerate() {
                                    let is_active = i <= current_step;
                                    let is_current = i == current_step;

                                    let color = if is_current {
                                        self.settings.theme.palette().accent
                                    } else if is_active {
                                        self.settings.theme.palette().success
                                    } else {
                                        self.settings.theme.palette().weak.linear_multiply(0.3)
                                    };

                                    let text = egui::RichText::new(*step).color(color).strong();
                                    ui.label(text);

                                    if i < steps.len() - 1 {
                                        ui.add_space(8.0);
                                        let line_color = if is_active {
                                            self.settings
                                                .theme
                                                .palette()
                                                .success
                                                .linear_multiply(0.5)
                                        } else {
                                            self.settings.theme.palette().weak.linear_multiply(0.1)
                                        };
                                        let (rect, _resp) = ui.allocate_exact_size(
                                            egui::vec2(40.0, 2.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().hline(
                                            rect.min.x..=rect.max.x,
                                            rect.center().y,
                                            egui::Stroke::new(2.0_f32, line_color),
                                        );
                                        ui.add_space(8.0);
                                    }
                                }
                            });
                        });
                    } else {
                        // History Thumbnail Strip
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Recent Statements")
                                    .color(ui.visuals().text_color().linear_multiply(0.6))
                                    .size(12.0),
                            );
                            ui.add_space(4.0);
                            egui::ScrollArea::horizontal()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let recent = self.settings.recent_files.clone();
                                        if recent.is_empty() {
                                            ui.label(
                                                egui::RichText::new("No recent files")
                                                    .italics()
                                                    .color(
                                                        ui.visuals()
                                                            .text_color()
                                                            .linear_multiply(0.3),
                                                    ),
                                            );
                                        }
                                        for f in recent.into_iter().take(5) {
                                            let label = std::path::Path::new(&f)
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy();

                                            if ui
                                                .add_sized([120.0, 36.0], egui::Button::new(label))
                                                .clicked()
                                            {
                                                self.open_pdf(std::path::PathBuf::from(f));
                                            }
                                        }
                                    });
                                });
                        });
                    }

                    // Add Report Bug button to the far right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🐛 Submit Diagnostics").clicked() {
                            self.active_modal = ActiveModal::Feedback;
                        }
                    });
                });
            });

        // 2. Right Toolbox: The 5 specific e2e editing actions
        let right_frame = egui::Frame {
            inner_margin: egui::Margin::same(16.0),
            fill: ctx.style().visuals.window_fill.linear_multiply(0.98),
            stroke: egui::Stroke::new(1.0_f32, ctx.style().visuals.widgets.inactive.bg_fill),
            shadow: egui::epaint::Shadow {
                offset: egui::vec2(0.0, 12.0),
                blur: 24.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(100),
            },
            ..Default::default()
        };

        egui::SidePanel::right("edit_toolbox")
            .frame(right_frame)
            .exact_width(340.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.heading(egui::RichText::new("Statement Forensics & Editing").strong());
                ui.add_space(24.0);

                if let Some(block) = self.selected_block.clone() {
                    egui::Frame::group(ui.style())
                        .fill(ui.visuals().faint_bg_color)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                egui::RichText::new("Properties")
                                    .color(ui.visuals().text_color().linear_multiply(0.6))
                                    .size(12.0),
                            );
                            ui.add_space(4.0);
                            ui.label(format!("Font: {}", block.font));
                            ui.label(format!("Size: {:.1} pt", block.size));
                        });

                    ui.add_space(16.0);

                    ui.label(
                        egui::RichText::new("Edit Content")
                            .color(ui.visuals().text_color().linear_multiply(0.6))
                            .size(12.0),
                    );
                    ui.add_space(4.0);

                    let text_edit = egui::TextEdit::multiline(&mut self.new_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(ui.available_width())
                        .margin(egui::vec2(12.0, 12.0));
                    ui.add(text_edit);

                    ui.add_space(20.0);

                    let btn_size = egui::vec2(ui.available_width(), 44.0);

                    if ui
                        .add_sized(
                            btn_size,
                            egui::Button::new(egui::RichText::new("Apply single edit").strong()),
                        )
                        .clicked()
                    {
                        let original = block.text.clone();
                        let new_text = self.new_text.clone();
                        if let Err(e) = self.job_tx.send(crate::app::runtime::Job::ApplyChange {
                            input: self.current_pdf_path.clone(),
                            output: std::path::PathBuf::from(&self.output_path),
                            page: self.current_page,
                            bbox: block.bbox,
                            old_text: original,
                            new_text,
                            description: "Manual Edit".to_string(),
                            deep_font_replication: self.settings.deep_font_replication,
                        }) {
                            tracing::error!("Runtime disconnected: {}", e);
                        }
                        self.in_flight += 1;
                        self.new_text.clear();
                        self.selected_block = None;
                    }
                    ui.add_space(5.0);
                    ui.add_space(5.0);
                    ui.separator();
                    ui.heading("Live Output Preview");
                    ui.add_space(10.0);
                    if let Some(tex) = &self.after_texture {
                        let max_size = ui.available_size() - egui::vec2(0.0, 20.0);
                        let tex_size = tex.size_vec2();
                        // Prevent scale from going unbounded, but fit it to the panel width
                        let scale = (ui.available_width() / tex_size.x)
                            .min(max_size.y / tex_size.y)
                            .min(1.0);
                        ui.add(egui::Image::new(tex).fit_to_exact_size(tex_size * scale));
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.weak("Preview will appear after an edit is applied.");
                        });
                    }
                    ui.add_space(5.0);
                    if ui
                        .add_sized(btn_size, egui::Button::new("Preview edits required"))
                        .clicked()
                    {
                        self.toast(ToastKind::Info, "Generating required edits proposal...");
                        let _ = self
                            .job_tx
                            .send(crate::app::runtime::Job::BalanceStatement {
                                path: std::path::PathBuf::from(&self.input_path),
                            });
                        self.in_flight += 1;
                    }
                    ui.add_space(5.0);
                    if ui
                        .add_sized(btn_size, egui::Button::new("Verify preview independently"))
                        .clicked()
                    {
                        self.toast(ToastKind::Info, "Running independent local verification...");
                        let intended_edits: Vec<crate::engine::verification::VerificationIntent> =
                            self.history_state
                                .get_history()
                                .iter()
                                .map(|record| crate::engine::verification::VerificationIntent {
                                    page: record.page,
                                    bbox: record.bbox,
                                    old_text: record.old_text.clone(),
                                    new_text: record.new_text.clone(),
                                })
                                .collect();
                        let _ = self.job_tx.send(crate::app::runtime::Job::Verify {
                            original: std::path::PathBuf::from(&self.input_path),
                            edited: std::path::PathBuf::from(&self.output_path),
                            output_dir: std::path::PathBuf::from("audit"),
                            intended_edits,
                            use_pdfrest: self.settings.verification_renderer
                                == crate::app::config::VerificationMode::PdfRestCloud,
                            pdfrest_key: self.config.pdfrest_api_key.clone(),
                            auto_match_dpi: self.settings.auto_match_dpi,
                        });
                        self.in_flight += 1;
                    }
                    ui.add_space(5.0);
                    if ui
                        .add_sized(
                            btn_size,
                            egui::Button::new("Perform * edits and perform complete balance out")
                                .fill(egui::Color32::from_rgb(0, 100, 0)),
                        )
                        .clicked()
                    {
                        self.toast(ToastKind::Info, "Executing full auto-balance editing...");
                        let _ = self
                            .job_tx
                            .send(crate::app::runtime::Job::BalanceStatement {
                                path: std::path::PathBuf::from(&self.input_path),
                            });
                        self.in_flight += 1;
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.weak("Click any text on the canvas to begin editing.");
                    });
                }
            });

        // 3. Central Panel: Context-aware zooming PDF canvas
        // This reuses the existing robust central panel rendering logic
        self.draw_central_panel(ctx);
    }

    pub(crate) fn draw_transfer_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("⇄ Cross-Ledger Migration Engine");
            ui.add_space(20.0);

            ui.columns(2, |columns| {
                // Source Dropzone
                columns[0].group(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Source Statement");
                        ui.label("Transactions will be extracted from this document.");
                        ui.add_space(10.0);
                        if ui
                            .add_sized([200.0, 80.0], egui::Button::new("📥 Upload Source\n(PDF)"))
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("PDF", &["pdf"])
                                .pick_file()
                            {
                                self.transfer_source_path = path.to_string_lossy().to_string();
                            }
                        }
                        if !self.transfer_source_path.is_empty() {
                            ui.add_space(5.0);
                            ui.colored_label(
                                egui::Color32::LIGHT_GREEN,
                                format!(
                                    "Selected: {}",
                                    std::path::Path::new(&self.transfer_source_path)
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                ),
                            );
                        }
                    });
                });

                // Target Dropzone
                columns[1].group(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading("Destination Ledger");
                        ui.label("Transactions will be injected into this document.");
                        ui.add_space(10.0);
                        if ui
                            .add_sized([200.0, 80.0], egui::Button::new("📥 Upload Target\n(PDF)"))
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("PDF", &["pdf"])
                                .pick_file()
                            {
                                self.input_path = path.to_string_lossy().to_string();
                            }
                        }
                        if !self.input_path.is_empty() && self.input_path != "examples/sample.pdf" {
                            ui.add_space(5.0);
                            ui.colored_label(
                                egui::Color32::LIGHT_GREEN,
                                format!(
                                    "Selected: {}",
                                    std::path::Path::new(&self.input_path)
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                ),
                            );
                        }
                    });
                });
            });

            ui.add_space(30.0);

            // Execute Transfer Action
            ui.vertical_centered(|ui| {
                let can_transfer = !self.transfer_source_path.is_empty()
                    && !self.input_path.is_empty()
                    && self.input_path != "examples/sample.pdf";

                let btn_id = ui.id().with("exec_transfer_btn");
                let hovered = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(btn_id))
                    .unwrap_or(false);
                let anim = ui.ctx().animate_bool_with_time(btn_id, hovered, 0.15);
                let expand_w = anim * 15.0;
                let expand_h = anim * 6.0;

                let bg_color = if can_transfer {
                    egui::Color32::from_rgb(0, 120 + (anim * 40.0) as u8, 0)
                } else {
                    egui::Color32::DARK_GRAY
                };

                let btn = egui::Button::new(
                    egui::RichText::new("⚡ Execute Complete Transfer").size(16.0 + (anim * 1.5)),
                )
                .min_size(egui::vec2(400.0 + expand_w, 60.0 + expand_h))
                .fill(bg_color);

                let response = ui.add_enabled(can_transfer, btn);
                if response.hovered() != hovered {
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(btn_id, response.hovered()));
                    ui.ctx().request_repaint();
                }

                if response.clicked() {
                    self.toast(
                        ToastKind::Info,
                        "Initiating Cross-Document Transaction Transfer...",
                    );
                    let _ = self
                        .job_tx
                        .send(crate::app::runtime::Job::ExtractTransactions {
                            path: std::path::PathBuf::from(&self.transfer_source_path),
                            parser_mode: self.settings.document_parser,
                        });
                    self.in_flight += 1;
                }

                if !can_transfer {
                    ui.add_space(5.0);
                    ui.weak("Please upload both a Source and Target statement to begin.");
                }
            });

            ui.add_space(30.0);
            ui.separator();
            ui.add_space(10.0);

            // Shared History Thumbnail Row
            ui.label("Recent Statements (Click to assign to Target):");
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let recent = self.settings.recent_files.clone();
                        for f in recent.into_iter().take(8) {
                            let label = std::path::Path::new(&f)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy();
                            if ui
                                .add_sized([120.0, 80.0], egui::Button::new(label))
                                .clicked()
                            {
                                self.input_path = f.clone();
                            }
                        }
                    });
                });
        });
    }

    pub(crate) fn draw_agent_command_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Command Center");
            ui.separator();
            ui.add_space(10.0);

            // Command input
            ui.horizontal(|ui| {
                ui.label("Instruction:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.command_query)
                        .desired_width(ui.available_width() - 80.0)
                        .hint_text("e.g. 'Change all transaction dates to 2026'"),
                );

                if (ui.button("Execute").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.command_query.is_empty() {
                    let prompt = std::mem::take(&mut self.command_query);
                    self.toast(ToastKind::Info, format!("Executing AI command: {}", prompt));
                    self.in_flight += 1;
                    let _ = self.job_tx.send(Job::NaturalLanguageEdit {
                        prompt,
                        transactions: self.workflow_transactions.clone(),
                    });
                }
            });

            ui.add_space(20.0);

            ui.group(|ui| {
                ui.heading("Manual commands only");
                ui.label("Background scraping and autonomous model training are not included in v1. Commands run only when you submit them here.");
            });
        });
    }

    pub(crate) fn draw_chaos_sandbox_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Controlled test runner unavailable");
            ui.label("The internal chaos suite is not exposed in the v1 application. No test has been started.");
        });
    }
    pub(crate) fn draw_settings_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("⚙️ System Configuration & Integrations");
            ui.separator();
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Theme:");
                        egui::ComboBox::from_id_salt("theme_selector")
                            .selected_text(format!("{:?}", self.settings.theme))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    Theme::ForensicDark,
                                    "Forensic Terminal (Dark)",
                                );
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    Theme::ForensicLight,
                                    "Laboratory (Light)",
                                );
                            });
                    });

                    ui.add_space(20.0);
                    self.draw_font_analysis_section(ui);
                    ui.add_space(20.0);
                    self.draw_workflow_section(ui);
                });
        });
    }

    pub(crate) fn draw_api_keys_workflow(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🔑 API Keys & Integration Management");
            ui.separator();
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.draw_api_keys_editor(ui);
                });
        });
    }

    pub(crate) fn draw_workflow_edit_table(&mut self, ui: &mut egui::Ui) {
        use crate::engine::workflow::{EditField, UserEdit};
        if self.workflow_transactions.is_empty() {
            return;
        }
        let palette = self.settings.theme.palette();

        ui.horizontal(|ui| {
            ui.label(format!(
                "📋 Inline edit ({} rows) - Tab to next field, ↶ reverts row",
                self.workflow_transactions.len()
            ));
            ui.add_space(8.0);
            if ui.button("🏷 Auto-Categorize").clicked() {
                if let Err(e) = self
                    .job_tx
                    .send(crate::app::runtime::Job::CategorizeTransactions {
                        transactions: self.workflow_transactions.clone(),
                    })
                {
                    tracing::error!("Runtime disconnected: {}", e);
                }
            }
        });

        // Snapshot what we need; the closure below mutates self.workflow_edits
        // and self.workflow_cell_buffers, so collect transaction copies first.
        let txs: Vec<crate::engine::model::Transaction> = self.workflow_transactions.clone();

        let mut cell_changes: Vec<(usize, usize, EditField, String, [f32; 4], String)> = Vec::new();
        let mut row_reverts: Vec<(usize, usize)> = Vec::new();

        egui::ScrollArea::both().auto_shrink([false, false])
            .max_height(220.0)
            .id_salt("workflow-edit-table")
            .show(ui, |ui| {
                use egui_extras::{Column, TableBuilder};
                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(28.0)) // P
                    .column(Column::auto().at_least(28.0)) // L
                    .column(Column::initial(78.0))         // Date
                    .column(Column::initial(150.0).at_least(100.0)) // Desc
                    .column(Column::initial(80.0))         // Category
                    .column(Column::initial(82.0))         // Debit
                    .column(Column::initial(82.0))         // Credit
                    .column(Column::initial(94.0))         // Balance
                    .column(Column::auto().at_least(28.0)) // Revert
                    .header(20.0, |mut header| {
                        for label in ["P", "#", "Date", "Description", "Category", "Debit", "Credit", "Balance", ""].iter() {
                            header.col(|ui| { ui.strong(*label); });
                        }
                    })
                    .body(|mut body| {
                        for tx in txs.iter() {
                            let key = (tx.page, tx.line_on_page);
                            let has_edit = self
                                .workflow_edits
                                .iter()
                                .any(|e| e.page == key.0 && e.line_on_page == key.1);

                            body.row(20.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(format!("{}", tx.page + 1));
                                });
                                row.col(|ui| {
                                    ui.label(format!("{}", tx.line_on_page + 1));
                                });

                                // Date - text field
                                row.col(|ui| {
                                    let buf = Self::cell_buffer(
                                        &mut self.workflow_cell_buffers,
                                        &self.workflow_edits,
                                        tx,
                                        EditField::Date,
                                        || tx.date.clone(),
                                    );
                                    if ui
                                        .add(egui::TextEdit::singleline(buf).desired_width(76.0))
                                        .changed()
                                    {
                                        cell_changes.push((
                                            tx.page,
                                            tx.line_on_page,
                                            EditField::Date,
                                            buf.clone(),
                                            Self::bbox_for_field(tx, EditField::Date),
                                            tx.date.clone(),
                                        ));
                                    }
                                });

                                // Description - text field
                                row.col(|ui| {
                                    let buf = Self::cell_buffer(
                                        &mut self.workflow_cell_buffers,
                                        &self.workflow_edits,
                                        tx,
                                        EditField::Description,
                                        || tx.raw_text.clone(),
                                    );
                                    if ui
                                        .add(egui::TextEdit::singleline(buf).desired_width(148.0))
                                        .changed()
                                    {
                                        cell_changes.push((
                                            tx.page,
                                            tx.line_on_page,
                                            EditField::Description,
                                            buf.clone(),
                                            Self::bbox_for_field(tx, EditField::Description),
                                            tx.raw_text.clone(),
                                        ));
                                    }
                                });

                                // Category - label
                                row.col(|ui| {
                                    let cat_text = tx.category.as_deref().unwrap_or("-");
                                    ui.label(egui::RichText::new(cat_text).color(palette.weak));
                                });

                                // Debit / Credit / Balance - money fields with red border on parse failure.
                                Self::money_cell(
                                    &mut row,
                                    &mut self.workflow_cell_buffers,
                                    &self.workflow_edits,
                                    tx,
                                    EditField::Debit,
                                    tx.debit,
                                    palette.warn,
                                    &mut cell_changes,
                                );
                                Self::money_cell(
                                    &mut row,
                                    &mut self.workflow_cell_buffers,
                                    &self.workflow_edits,
                                    tx,
                                    EditField::Credit,
                                    tx.credit,
                                    palette.warn,
                                    &mut cell_changes,
                                );
                                Self::money_cell(
                                    &mut row,
                                    &mut self.workflow_cell_buffers,
                                    &self.workflow_edits,
                                    tx,
                                    EditField::RunningBalance,
                                    tx.running_balance,
                                    palette.warn,
                                    &mut cell_changes,
                                );

                                // Revert column
                                row.col(|ui| {
                                    let label = if has_edit { "↶" } else { " " };
                                    if ui
                                        .add_enabled(
                                            has_edit,
                                            egui::Button::new(label).small(),
                                        )
                                        .on_hover_text("Revert all queued edits on this row")
                                        .clicked()
                                    {
                                        row_reverts.push((tx.page, tx.line_on_page));
                                    }
                                });
                            });
                        }
                    });
            });

        // Apply collected changes after the table render so we don't double-borrow self.
        for (page, line, field, new_text, bbox, old_text) in cell_changes {
            self.upsert_edit(UserEdit {
                page,
                line_on_page: line,
                bbox,
                old_text,
                new_text,
                field,
            });
        }
        if !row_reverts.is_empty() {
            for (page, line) in row_reverts {
                self.revert_row_edits(page, line);
            }
        }
    }

    /// Pick the per-field bbox for an edit. Falls back to the row-level
    /// bbox when the field-specific one isn't known (older parses, manual
    /// transactions). Stage 7.5 - without this, a debit edit would redact
    /// the entire row.
    fn bbox_for_field(
        tx: &crate::engine::model::Transaction,
        field: crate::engine::workflow::EditField,
    ) -> [f32; 4] {
        use crate::engine::workflow::EditField;
        let specific = match field {
            EditField::Date => tx.field_bboxes.date,
            EditField::Description => tx.field_bboxes.description,
            EditField::Debit => tx.field_bboxes.debit,
            EditField::Credit => tx.field_bboxes.credit,
            EditField::RunningBalance => tx.field_bboxes.running_balance,
        };
        specific.or(tx.bbox).unwrap_or([0.0; 4])
    }

    /// Get-or-init the per-cell text buffer. If the user has already queued
    /// an edit for this cell, the buffer reflects the queued new text;
    /// otherwise it starts from the parsed value.
    fn cell_buffer<'a>(
        buffers: &'a mut std::collections::HashMap<
            (usize, usize, crate::engine::workflow::EditField),
            String,
        >,
        edits: &[crate::engine::workflow::UserEdit],
        tx: &crate::engine::model::Transaction,
        field: crate::engine::workflow::EditField,
        default: impl FnOnce() -> String,
    ) -> &'a mut String {
        let key = (tx.page, tx.line_on_page, field);
        buffers.entry(key).or_insert_with(|| {
            edits
                .iter()
                .find(|e| {
                    e.page == tx.page && e.line_on_page == tx.line_on_page && e.field == field
                })
                .map(|e| e.new_text.clone())
                .unwrap_or_else(default)
        })
    }

    /// Render a single money cell (debit/credit/balance). Red border when
    /// the typed text isn't parseable.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn money_cell(
        row: &mut egui_extras::TableRow<'_, '_>,
        buffers: &mut std::collections::HashMap<
            (usize, usize, crate::engine::workflow::EditField),
            String,
        >,
        edits: &[crate::engine::workflow::UserEdit],
        tx: &crate::engine::model::Transaction,
        field: crate::engine::workflow::EditField,
        original: Option<rust_decimal::Decimal>,
        warn_color: egui::Color32,
        out: &mut Vec<(
            usize,
            usize,
            crate::engine::workflow::EditField,
            String,
            [f32; 4],
            String,
        )>,
    ) {
        row.col(|ui| {
            let buf = Self::cell_buffer(buffers, edits, tx, field, || {
                original.map(|v| format!("{v:.2}")).unwrap_or_default()
            });
            let valid = buf.trim().is_empty()
                || buf
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
                    .collect::<String>()
                    .parse::<f64>()
                    .is_ok();
            let mut edit = egui::TextEdit::singleline(buf).desired_width(80.0);
            if !valid {
                edit = edit.text_color(warn_color);
            }
            let resp = ui.add(edit);
            if resp.changed() {
                let old_text = original.map(|v| format!("{v:.2}")).unwrap_or_default();
                out.push((
                    tx.page,
                    tx.line_on_page,
                    field,
                    buf.clone(),
                    Self::bbox_for_field(tx, field),
                    old_text,
                ));
            }
        });
    }

    /// Insert or replace the edit on (page, line, field). When the new
    /// text equals the originally-parsed value, the edit is removed from
    /// the queue instead - typing a value back to its original is
    /// equivalent to no edit at all.
    fn upsert_edit(&mut self, mut edit: crate::engine::workflow::UserEdit) {
        // If the new text equals the original, drop any matching edit.
        let parsed_original = self
            .workflow_transactions
            .iter()
            .find(|t| t.page == edit.page && t.line_on_page == edit.line_on_page);
        let original_text = parsed_original
            .map(|t| match edit.field {
                crate::engine::workflow::EditField::Date => t.date.clone(),
                crate::engine::workflow::EditField::Description => t.raw_text.clone(),
                crate::engine::workflow::EditField::Debit => t
                    .debit
                    .map(|v| format!("{:.2}", v.round_dp(2)))
                    .unwrap_or_default(),
                crate::engine::workflow::EditField::Credit => t
                    .credit
                    .map(|v| format!("{:.2}", v.round_dp(2)))
                    .unwrap_or_default(),
                crate::engine::workflow::EditField::RunningBalance => t
                    .running_balance
                    .map(|v| format!("{:.2}", v.round_dp(2)))
                    .unwrap_or_default(),
            })
            .unwrap_or_default();

        // Use the original text we just looked up.
        if edit.old_text.is_empty() {
            edit.old_text = original_text.clone();
        }

        // No-op if the user typed back to the original - drop it.
        if edit.new_text == original_text {
            self.workflow_edits.retain(|e| {
                !(e.page == edit.page
                    && e.line_on_page == edit.line_on_page
                    && e.field == edit.field)
            });
            self.workflow_dirty = true;
            return;
        }

        if let Some(slot) = self.workflow_edits.iter_mut().find(|e| {
            e.page == edit.page && e.line_on_page == edit.line_on_page && e.field == edit.field
        }) {
            slot.new_text = edit.new_text;
            slot.bbox = edit.bbox;
        } else {
            self.workflow_edits.push(edit);
        }
        self.workflow_dirty = true;
    }

    /// Drop every queued edit on (page, line) and reset the cell buffers
    /// for that row so the table reflects the parsed values.
    fn revert_row_edits(&mut self, page: usize, line_on_page: usize) {
        let before = self.workflow_edits.len();
        self.workflow_edits
            .retain(|e| !(e.page == page && e.line_on_page == line_on_page));
        let removed = before.saturating_sub(self.workflow_edits.len());
        // Clear the cached cell buffers for this row so they re-init from
        // the parsed transaction next frame.
        self.workflow_cell_buffers
            .retain(|(p, l, _), _| !(*p == page && *l == line_on_page));
        if removed > 0 {
            self.workflow_dirty = true;
            self.toast(
                ToastKind::Info,
                format!(
                    "Reverted {} edit(s) on P{} L{}",
                    removed,
                    page + 1,
                    line_on_page + 1
                ),
            );
        }
    }

    /// Stage 8.5: per-font breakdown for the loaded PDF. Shows the user which
    /// fonts can be edited freely and which would need glyph creation, with
    /// an exact list of missing characters per font and the creation scope.
    pub fn draw_font_analysis_section(&mut self, ui: &mut egui::Ui) {
        let palette = self.settings.theme.palette();
        let analysis = match &self.font_analysis {
            Some(a) => a.clone(),
            None => {
                ui.collapsing("🔤 Font analysis", |ui| {
                    ui.label("Loading...");
                    if ui.button("Re-analyze").clicked() {
                        if let Err(e) = self.job_tx.send(Job::AnalyzeFonts {
                            path: PathBuf::from(&self.input_path),
                        }) {
                            tracing::error!("Runtime disconnected: {}", e);
                        }
                        self.in_flight += 1;
                    }
                });
                return;
            }
        };

        let header = if analysis.summary.all_fonts_covered {
            format!(
                "🔤 Font analysis - ✅ {} font(s), all covered",
                analysis.summary.total_fonts
            )
        } else {
            format!(
                "🔤 Font analysis - ⚠ {}/{} font(s) need attention",
                analysis.summary.fonts_needing_action, analysis.summary.total_fonts
            )
        };

        ui.collapsing(header, |ui| {
            // High-level summary line.
            let summary_color = if analysis.summary.all_fonts_covered {
                palette.success
            } else {
                palette.warn
            };
            ui.colored_label(summary_color, analysis.one_line_summary());

            if !analysis.summary.all_fonts_covered {
                ui.horizontal(|ui| {
                    if analysis.summary.missing_digit_count > 0 {
                        ui.colored_label(
                            palette.warn,
                            format!("Digits: {}", analysis.summary.missing_digit_count),
                        );
                    }
                    if analysis.summary.missing_letter_count > 0 {
                        ui.colored_label(
                            palette.warn,
                            format!("Letters: {}", analysis.summary.missing_letter_count),
                        );
                    }
                    if analysis.summary.missing_other_count > 0 {
                        ui.colored_label(
                            palette.warn,
                            format!("Other: {}", analysis.summary.missing_other_count),
                        );
                    }
                });
            }

            ui.separator();

            if ui.button("🔄 Re-analyze").clicked() {
                if let Err(e) = self.job_tx.send(Job::AnalyzeFonts {
                    path: PathBuf::from(&self.input_path),
                }) {
                    tracing::error!("Runtime disconnected: {}", e);
                }
                self.in_flight += 1;
            }

            ui.separator();

            // Per-font breakdown.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .id_salt("font-analysis-list")
                .max_height(280.0)
                .show(ui, |ui| {
                    for (i, font) in analysis.fonts.iter().enumerate() {
                        let needs_action = !font.missing_chars.is_empty();
                        let row_color = if needs_action {
                            palette.warn
                        } else {
                            palette.success
                        };
                        let role_label = match font.usage_role {
                            crate::engine::font_analysis::UsageRole::Digits => "digits",
                            crate::engine::font_analysis::UsageRole::Letters => "letters",
                            crate::engine::font_analysis::UsageRole::Mixed => "mixed",
                            crate::engine::font_analysis::UsageRole::Punctuation => "punct",
                            crate::engine::font_analysis::UsageRole::Other => "other",
                        };
                        let header = format!(
                            "{} {} • {} • {} use(s) on {} page(s)",
                            if needs_action { "⚠" } else { "✅" },
                            font.base_name,
                            role_label,
                            font.occurrences,
                            font.pages_used_on.len(),
                        );
                        let id = ui.make_persistent_id(("font-analysis", i));
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            id,
                            false,
                        )
                        .show_header(ui, |ui| {
                            ui.colored_label(row_color, header);
                        })
                        .body(|ui| {
                            ui.label(font.fidelity_impact.as_str());
                            ui.label(font.creation_scope.as_str());
                            ui.small(format!(
                                "Standard-14: {} • Subset: {}",
                                if font.is_standard_14 { "yes" } else { "no" },
                                if font.is_subset { "yes" } else { "no" },
                            ));
                            // Truncate the used-character preview at 80 chars
                            // so a font with hundreds of glyphs doesn't dominate
                            // the panel.
                            let used_preview: String = if font.characters_used.chars().count() > 80
                            {
                                let head: String = font.characters_used.chars().take(80).collect();
                                format!("{head}...")
                            } else {
                                font.characters_used.clone()
                            };
                            ui.small(format!("Used characters: {used_preview}"));
                            if !font.missing_chars.is_empty() {
                                let missing_str = font.missing_chars.join(" ");
                                ui.colored_label(palette.warn, format!("Missing: {missing_str}"));
                                let bd = &font.missing_breakdown;
                                if !bd.digits.is_empty() {
                                    ui.small(format!("  Digits: {}", bd.digits.join(" ")));
                                }
                                if !bd.letters.is_empty() {
                                    ui.small(format!("  Letters: {}", bd.letters.join(" ")));
                                }
                                if !bd.other.is_empty() {
                                    ui.small(format!("  Other: {}", bd.other.join(" ")));
                                }
                            }
                            ui.small(format!(
                                "Sizes: {:.1}-{:.1}pt • Pages: {}",
                                font.size_range[0],
                                font.size_range[1],
                                font.pages_used_on
                                    .iter()
                                    .map(|p| (p + 1).to_string())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            ));
                        });
                    }
                });
        });
    }

    pub fn draw_workflow_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("🤖 Workflow (AI parse -> preview -> render -> verify)", |ui| {
            let stage = self.workflow_stage.clone();
            let p = self.settings.theme.palette();

            // Step indicator. Stage 13 / Item #1: each label is hoverable
            // so the user can read what the step actually does, and the
            // active step gets a strong color so the indicator never looks
            // muted at idle.
            let step = stage.step_index();
            ui.horizontal(|ui| {
                let descriptions = [
                    "Run Document AI + Gemini completeness check",
                    "Edit values inline; queued edits go to Preview",
                    "Recompute every running balance with your edits",
                    "Apply edits to the PDF (binary-level redact-and-replace)",
                    "Render & compare; loop until visual match passes",
                    "Re-parse with Document AI to confirm math integrity",
                ];
                for (i, name) in [
                    "Parse", "Edit", "Preview", "Render", "Verify", "Confirm",
                ]
                .iter()
                .enumerate()
                {
                    let active_step = (i + 1) as u8;
                    let (color, label) = if active_step < step {
                        (p.success, format!("✓ {}. {name}", i + 1))
                    } else if active_step == step.min(6) {
                        (p.accent, format!("► {}. {name}", i + 1))
                    } else {
                        (p.weak, format!("{}. {name}", i + 1))
                    };
                    ui.colored_label(color, label).on_hover_text(descriptions[i]);
                }
            });
            ui.label(format!("Status: {}", stage.label()));

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Parser Version:");
                egui::ComboBox::from_id_salt("parser_version_select")
                    .selected_text(self.selected_parser_version.split('-').nth(2).unwrap_or(&self.selected_parser_version))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.selected_parser_version,
                            crate::app::config::DEFAULT_DOCAI_PROCESSOR_VERSION.to_string(),
                            "v5.0 (Default)",
                        );
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v4.0-2023-07-31".to_string(), "v4.0");
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v3.0-2022-05-16".to_string(), "v3.0");
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v2.0-2021-12-10".to_string(), "v2.0");
                        ui.selectable_value(&mut self.selected_parser_version, "pretrained-bankstatement-v1.1-2021-08-13".to_string(), "v1.1");
                    });
                if ui.button("🔄 Parse").on_hover_text("Re-parse document with selected parser version").clicked() && !self.input_path.is_empty() {
                    if let Err(e) = self.dispatch_workflow_job(Job::WorkflowParseAndValidate {
                        input: PathBuf::from(&self.input_path),
                        version: Some(self.selected_parser_version.clone()),
                        parser_mode: self.settings.document_parser,
                        ai_provider: self.settings.ai_provider,
                        ignore_offline_fallback: false,
                    }) { tracing::error!("Runtime disconnected: {}", e); }
                    self.in_flight += 1;
                    self.workflow_edits.clear();
                    self.workflow_preview = None;
                    self.workflow_visual = None;
                    self.workflow_outcome = None;
                    self.font_cascade_reports.clear();
                    self.workflow_dirty = true;
                    self.toast(ToastKind::Info, "Parse triggered");
                }
            });

            ui.separator();



            if let Some(v) = &self.workflow_validation {
                ui.label(format!(
                    "Found {} txs • opening ${:.2} • closing ${:.2}",
                    v.transactions_found, v.opening_balance, v.closing_balance
                ));
                let bar_color = if v.is_acceptable() { p.success } else { p.warn };
                ui.colored_label(
                    bar_color,
                    format!("AI completeness: {:.0}%", v.completeness_score * 100.0),
                );
                if !v.completeness_notes.is_empty() {
                    ui.small(&v.completeness_notes);
                }
                if !v.missing_rows.is_empty() {
                    ui.colored_label(p.warn, format!("Possibly missing rows: {}", v.missing_rows.len()));
                    for m in v.missing_rows.iter().take(3) {
                        ui.small(format!("  • {m}"));
                    }
                }
            }

            ui.separator();

            // Stage 5 / Item #6 + #8: inline edit table with per-row revert.
            self.draw_workflow_edit_table(ui);

            ui.separator();

            // Natural Language Interface
            ui.label(egui::RichText::new("✨ Natural Language Edit (Beta)").strong());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.natural_language_prompt)
                    .hint_text("e.g. Change all Starbucks to Dunkin")
                    .desired_width(280.0));
                let can_edit = !self.natural_language_prompt.is_empty() && !self.workflow_transactions.is_empty();
                if ui.add_enabled(can_edit, egui::Button::new("Apply")).clicked() {
                    if let Err(e) = self.job_tx.send(crate::app::runtime::Job::NaturalLanguageEdit {
                        prompt: self.natural_language_prompt.clone(),
                        transactions: self.workflow_transactions.clone(),
                    }) {
                        tracing::error!("Runtime disconnected: {}", e);
                    }
                    self.in_flight += 1;
                    self.natural_language_prompt.clear();
                }
            });

            ui.separator();

            // Stage 3 button: balance preview
            let preview_enabled = self.workflow_validation.is_some();
            ui.label(format!("Pending edits queued: {}", self.workflow_edits.len()));
            if ui
                .add_enabled(preview_enabled, egui::Button::new("② Balance Out Preview"))
                .on_hover_text("Recompute every running balance with your edits and show the diff")
                .clicked()
            {
                if let Some(v) = &self.workflow_validation {
                    if let Err(e) = self.dispatch_workflow_job(Job::WorkflowPreview {
                        original_transactions: self.workflow_transactions.clone(),
                        edits: self.workflow_edits.clone(),
                        opening_balance: v.opening_balance,
                        expected_closing: if v.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                            Some(v.closing_balance)
                        } else {
                            None
                        },
                    }) { tracing::error!("Runtime disconnected: {}", e); }
                    self.in_flight += 1;
                }
            }

            if let Some(p) = &self.workflow_preview {
                let changed = p.rows.iter().filter(|r| r.will_change).count();
                let kind_color = if p.balanced { self.settings.theme.palette().success } else { self.settings.theme.palette().warn };
                ui.colored_label(
                    kind_color,
                    format!(
                        "{} row(s) will change • final imbalance ${:.2}",
                        changed, p.final_imbalance
                    ),
                );

                if !p.balanced && ui.button("🧠 Ask Local AI to Explain").clicked() {
                    let opening_balance = self
                        .workflow_validation
                        .as_ref()
                        .map(|v| {
                            v.opening_balance
                                .to_string()
                                .parse::<f64>()
                                .unwrap_or(0.0)
                        })
                        .unwrap_or(0.0);
                    let closing_balance = self
                        .workflow_validation
                        .as_ref()
                        .map(|v| {
                            v.closing_balance
                                .to_string()
                                .parse::<f64>()
                                .unwrap_or(0.0)
                        })
                        .unwrap_or(0.0);
                    let imbalance_f64 = p
                        .final_imbalance
                        .to_string()
                        .parse::<f64>()
                        .unwrap_or(0.0);

                    if let Err(e) = self.job_tx.send(crate::app::runtime::Job::ExplainImbalance {
                        transactions_json: serde_json::to_string(&self.workflow_transactions)
                            .unwrap_or_default(),
                        opening_balance,
                        closing_balance,
                        imbalance: imbalance_f64,
                    }) {
                        tracing::error!("Runtime disconnected: {}", e);
                    }
                    self.in_flight += 1;
                    self.ai_explanation = None;
                }

                let mut clear_explanation = false;
                if let Some(explanation) = &self.ai_explanation {
                    ui.add_space(4.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Local AI Forensics").strong().color(self.settings.theme.palette().success));
                        ui.label(explanation);
                        if ui.button("Dismiss").clicked() {
                            clear_explanation = true;
                        }
                    });
                }
                if clear_explanation {
                    self.ai_explanation = None;
                }

                if let Some(msg) = &p.auto_correction_message {
                    ui.small(msg);
                }
                // Compact diff list
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(120.0).show(ui, |ui| {
                    for r in p.rows.iter().filter(|r| r.will_change).take(20) {
                        // Char-aware truncation so multi-byte UTF-8 (CJK,
                        // accented Latin) doesn't panic on byte slicing.
                        let desc_short: String = if r.description.chars().count() > 24 {
                            let head: String = r.description.chars().take(24).collect();
                            format!("{head}...")
                        } else {
                            r.description.clone()
                        };
                        ui.small(format!(
                            "P{} L{} {} • bal {:?} -> {:?}",
                            r.page + 1,
                            r.line_on_page + 1,
                            desc_short,
                            r.old_running_balance,
                            r.new_running_balance,
                        ));
                    }
                });
            }

            ui.separator();

            // Stage 4-6: Quick (native) vs Deep (PyMuPDF) apply options.
            // Dual-engine safety: both engines stay loaded; if one fails the
            // other takes over. The user picks the fidelity tier per apply and
            // can re-run Deep if Quick doesn't suffice.
            let confirm_enabled = self.workflow_preview.is_some();
            let edit_count = self.workflow_edits.len().max(1);
            // Rough ETAs so the user can weigh speed vs fidelity. Native is ~1s
            // per edit; PyMuPDF Deep adds Pro per-segment work + deep font
            // replication overhead (~3s per edit plus a fixed warm-up).
            let _quick_eta = 2 + edit_count;
            let deep_eta = 5 + edit_count * 3;
            ui.label("3. Finalize Edits:");
            ui.horizontal(|ui| {
                let pro_btn_id = ui.id().with("pro_edit_btn");
                let hovered_pro = ui.ctx().data(|d| d.get_temp::<bool>(pro_btn_id)).unwrap_or(false);
                let anim_pro = ui.ctx().animate_bool_with_time(pro_btn_id, hovered_pro, 0.15);

                let pro_btn = egui::Button::new(
                    egui::RichText::new(format!("🎯 Perform Pro Edit • ~{deep_eta}s"))
                        .size(14.0 + anim_pro)
                ).fill(egui::Color32::from_rgb((20.0 * anim_pro) as u8, 80 + (anim_pro * 40.0) as u8, (40.0 * anim_pro) as u8));

                let resp_pro = ui.add_enabled(confirm_enabled, pro_btn)
                    .on_hover_text("High-fidelity PyMuPDF Pro apply with deep font replication.");

                if resp_pro.hovered() != hovered_pro {
                    ui.ctx().data_mut(|d| d.insert_temp(pro_btn_id, resp_pro.hovered()));
                    ui.ctx().request_repaint();
                }

                if resp_pro.clicked() {
                    self.dispatch_confirm_and_render(true, false);
                }

                let ai_btn_id = ui.id().with("verify_ai_btn");
                let hovered_ai = ui.ctx().data(|d| d.get_temp::<bool>(ai_btn_id)).unwrap_or(false);
                let anim_ai = ui.ctx().animate_bool_with_time(ai_btn_id, hovered_ai, 0.15);

                let ai_btn = egui::Button::new(
                    egui::RichText::new("🤖 Verify with AI")
                        .size(14.0 + anim_ai)
                ).fill(egui::Color32::from_rgb((80.0 * anim_ai) as u8, (30.0 * anim_ai) as u8, 120 + (anim_ai * 40.0) as u8));

                let resp_ai = ui.add_enabled(confirm_enabled, ai_btn)
                    .on_hover_text("Cross-check the edits and layout with Gemini Vision before finalizing.");

                if resp_ai.hovered() != hovered_ai {
                    ui.ctx().data_mut(|d| d.insert_temp(ai_btn_id, resp_ai.hovered()));
                    ui.ctx().request_repaint();
                }

                if resp_ai.clicked() {
                    self.toast(ToastKind::Info, "Dispatching AI verification...");
                    // We can dispatch a Job::Verify (or similar) here, but for now we'll trigger a background verification via AI.
                    let input = PathBuf::from(&self.input_path);
                    if let Err(e) = self.job_tx.send(crate::app::runtime::Job::AiCommand {
                        prompt: "Verify that all tabular edits align with the original styling and are mathematically sound.".to_string(),
                        path: input,
                    }) {
                        tracing::error!("Failed to dispatch AI verify: {}", e);
                    }
                }
            });
            ui.small(format!(
                "All edits are applied instantly via Native Rust. Use Pro Edit for final polish. {} edit(s) applied.",
                self.workflow_edits.len()
            ));

            if let Some(va) = &self.workflow_visual {
                let palette = self.settings.theme.palette();
                let c = if va.passed() { palette.success } else { palette.warn };
                ui.colored_label(
                    c,
                    format!(
                        "Visual {}/{} • diff {:.4} • intended-only {}",
                        va.attempt,
                        va.max_attempts,
                        va.diff_score,
                        if va.only_intended { "✓" } else { "✗" }
                    ),
                );
            }
            if let Some(o) = &self.workflow_outcome {
                ui.colored_label(self.settings.theme.palette().success, &o.completion_summary);
                ui.small(format!("Final PDF: {}", o.final_pdf.display()));
                ui.small(format!(
                    "Re-parsed transactions: {} • final imbalance ${:.2}",
                    o.transactions_re_parsed, o.final_imbalance
                ));
            }

            if let crate::engine::workflow::WorkflowStage::FontCoverageWarning { missing_chars } = &self.workflow_stage {
                ui.separator();
                let palette = self.settings.theme.palette();
                ui.colored_label(palette.warn, "Font Coverage Block");
                ui.label(format!("The replacement requires characters absent from the selected embedded or supplied font:\n{:?}", missing_chars));
                ui.label("Automatic typeface substitution is disabled because it would not preserve statement fidelity. Change the replacement text or supply a reviewed font that covers every character.");
                if ui.button("Return to Edit Review").clicked() {
                    let preview = self.workflow_preview.clone().unwrap_or_default();
                    self.apply_workflow_event(
                        crate::engine::workflow::WorkflowEvent::ResumePreview(preview),
                    );
                }
            }

            // Stage 12 / Item #3: surface cascade results so the user can
            // see exactly which tier(s) closed any font-coverage gap.
            if !self.font_cascade_reports.is_empty() {
                ui.separator();
                ui.label("🔧 Font cascade history:");
                let palette = self.settings.theme.palette();
                for report in &self.font_cascade_reports {
                    let color = if report.success { palette.success } else { palette.warn };
                    ui.colored_label(
                        color,
                        format!(
                            "Attempt {} on '{}': {}",
                            report.workflow_attempt,
                            report.original_font,
                            report.one_line_summary()
                        ),
                    );
                    if !report.synthesised.is_empty() {
                        ui.small(format!("  composite: {}", report.synthesised.join(", ")));
                    }
                    if !report.donor_extended.is_empty() {
                        ui.small(format!("  donor:     {}", report.donor_extended.join(", ")));
                    }
                    if !report.ai_extended.is_empty() {
                        ui.small(format!("  AI donor:  {}", report.ai_extended.join(", ")));
                    }
                    if !report.still_missing.is_empty() {
                        ui.colored_label(
                            palette.warn,
                            format!("  still missing: {}", report.still_missing.join(", ")),
                        );
                    }
                }
            }
        });
    }

    /// Apply the queued workflow edits to the PDF and render-validate them.
    ///
    /// `deep` selects the Deep fidelity tier (PyMuPDF Pro per-segment edit +
    /// deep font replication); when `false` the Quick (native) tier runs. Both
    /// tiers share the redundant-edit pruning so the apply loop stays tight, and
    /// both run under the dual-engine safety net so a single engine failure
    /// falls back to the other rather than aborting the edit.
    pub(crate) fn dispatch_confirm_and_render(&mut self, deep: bool, ignore_font_coverage: bool) {
        // Stage 2 / Item #7: drop edits whose typed value already matches the
        // cascade. Reduces visual noise (extra redactions) and shortens the
        // apply loop.
        let edits_to_apply = if let Some(p) = &self.workflow_preview {
            let (kept, dropped) =
                crate::engine::workflow::prune_redundant_edits(&self.workflow_edits, p);
            if !dropped.is_empty() {
                self.toast(
                    ToastKind::Info,
                    format!("Pruned {} redundant edit(s)", dropped.len()),
                );
            }
            kept
        } else {
            self.workflow_edits.clone()
        };
        self.toast(
            ToastKind::Info,
            if deep {
                "Applying with Deep (PyMuPDF) fidelity..."
            } else {
                "Applying with Quick (Native) fidelity..."
            },
        );
        if let Err(e) = self.dispatch_workflow_job(Job::WorkflowConfirmAndRender {
            input: PathBuf::from(&self.input_path),
            output: PathBuf::from(&self.output_path),
            edits: edits_to_apply,
            original_transactions: self.workflow_transactions.clone(),
            opening_balance: self
                .workflow_validation
                .as_ref()
                .map(|v| v.opening_balance)
                .unwrap_or_default(),
            expected_closing: self.workflow_validation.as_ref().and_then(|v| {
                if v.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                    Some(v.closing_balance)
                } else {
                    None
                }
            }),
            deep_font_replication: deep,
            max_visual_attempts: self.settings.max_visual_attempts,
            visual_threshold: self.settings.visual_diff_threshold,
            ignore_font_coverage,
            ignore_visual_fidelity: false,
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
    }

    /// Path of the on-disk autosave for the current workflow. One file per
    /// session - overwritten as edits change. Stage 5 / Item #9.
    pub fn workflow_draft_path() -> PathBuf {
        crate::app::paths::AppPaths::discover()
            .map(|paths| paths.audit_dir().join("workflow.json"))
            .unwrap_or_else(|_| PathBuf::from("audit").join("workflow.json"))
    }

    /// Delete the on-disk draft if it exists. Used after a successful
    /// `WorkflowComplete` and from the "Discard draft" menu. Errors are
    /// logged but never surfaced - the file may legitimately be missing.
    pub fn discard_workflow_draft_quiet() {
        let path = Self::workflow_draft_path();
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("[gui] removing workflow draft failed: {}", e);
            }
        }
    }

    /// Persist the current workflow state to `audit/workflow.json` if there
    /// is anything worth saving (validation has been done, OR there are
    /// queued edits) and the dirty flag is set. Debounced to at most one
    /// write per 1.5s. Failures are logged but never raised - losing an
    /// autosave is non-fatal.
    pub(crate) fn autosave_workflow_draft(&mut self) {
        if !self.workflow_dirty {
            return;
        }
        // Nothing to save until a parse has produced a baseline.
        if self.workflow_validation.is_none() && self.workflow_edits.is_empty() {
            self.workflow_dirty = false;
            return;
        }
        // Debounce: 1.5s between writes.
        if let Some(t) = self.workflow_last_save {
            if t.elapsed() < Duration::from_millis(1500) {
                return;
            }
        }
        let pdf = PathBuf::from(&self.input_path);
        if !pdf.exists() {
            self.workflow_dirty = false;
            return;
        }
        // Cache the PDF SHA-256 once per (input_path, file change) so the
        // autosave doesn't re-read multi-MB files every 1.5s. The cache key
        // is just the path; if the user opens a new PDF the cache is
        // cleared in `open_pdf`. Stage 6.
        let hash = match &self.workflow_input_hash {
            Some((cached_path, h)) if cached_path == &self.input_path => h.clone(),
            _ => {
                let bytes = match std::fs::read(&pdf) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("[gui] reading PDF for hash failed: {}", e);
                        self.workflow_dirty = false;
                        return;
                    }
                };
                let h = crate::engine::workflow::sha256_hex_of(&bytes);
                self.workflow_input_hash = Some((self.input_path.clone(), h.clone()));
                h
            }
        };
        let draft = crate::engine::workflow::WorkflowDraft::new_with_hash(
            &pdf,
            hash,
            self.workflow_validation.clone(),
            self.workflow_transactions.clone(),
            self.workflow_edits.clone(),
        );
        let path = self.active_workflow_draft_path();
        match draft.save_to_file(&path) {
            Ok(()) => {
                tracing::debug!("[gui] saved workflow draft to {}", path.display());
                self.workflow_dirty = false;
                self.workflow_last_save = Some(Instant::now());
            }
            Err(e) => {
                tracing::warn!("[gui] saving workflow draft failed: {}", e);
                // Leave dirty=true so we retry next frame.
            }
        }
    }

    /// Resume a workflow from `audit/workflow.json`, restoring validation,
    /// transactions and queued edits. Verifies the on-disk PDF still
    /// hashes to what the draft expects; if not, surfaces a warning toast
    /// but proceeds - the user might intentionally be loading a draft
    /// against a manually-saved copy.
    pub(crate) fn resume_workflow_draft(&mut self) {
        let path = self.active_workflow_draft_path();
        if !path.exists() {
            self.toast(ToastKind::Warn, "No workflow draft to resume.");
            return;
        }
        let draft = match crate::engine::workflow::WorkflowDraft::load_from_file(&path) {
            Ok(d) => d,
            Err(e) => {
                self.toast(ToastKind::Error, format!("Could not load draft: {e}"));
                return;
            }
        };

        // Stage 13 / Item #11: when the original PDF is missing, prompt the
        // user to locate it instead of orphaning the draft. If they
        // cancel, leave the draft untouched so they can retry later.
        let mut pdf_path = PathBuf::from(&draft.input_path);
        if !pdf_path.exists() {
            self.toast(
                ToastKind::Warn,
                format!("PDF missing: {} - please pick the file", pdf_path.display()),
            );
            match rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_title("Locate the PDF this draft was saved against")
                .pick_file()
            {
                Some(picked) => {
                    pdf_path = picked;
                }
                None => {
                    self.toast(
                        ToastKind::Info,
                        "Resume cancelled - draft kept; pick the PDF later.",
                    );
                    return;
                }
            }
        }

        let same = draft.matches_pdf(&pdf_path);
        // Restore session state.
        self.input_path = pdf_path.to_string_lossy().to_string();
        self.activate_run_workspace(&pdf_path);
        self.current_pdf_path = pdf_path.clone();
        self.workflow_validation = draft.validation.clone();
        self.workflow_transactions = draft.transactions.clone();
        self.workflow_edits = draft.edits.clone();
        self.workflow_preview = None;
        self.workflow_visual = None;
        self.workflow_outcome = None;
        self.apply_workflow_event(crate::engine::workflow::WorkflowEvent::Reset);
        if let Some(validation) = draft.validation.clone() {
            self.apply_workflow_event(crate::engine::workflow::WorkflowEvent::RestoreEditing(
                validation,
            ));
        }
        self.workflow_dirty = false;

        // Trigger a render of the PDF.
        if let Err(e) = self.job_tx.send(Job::LoadDocument {
            path: pdf_path.clone(),
            three_page_mode: self.settings.three_page_mode,
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;

        if same {
            self.toast(
                ToastKind::Success,
                format!(
                    "Resumed workflow draft - {} edits queued",
                    draft.edits.len()
                ),
            );
        } else {
            self.toast(
                ToastKind::Warn,
                "Draft loaded but the PDF has changed since it was saved.",
            );
        }
    }
}
