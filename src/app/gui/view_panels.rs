//! Chrome panels: status bar, navigation/left panel, batch panel, audit explorer, and toast notifications.
#![allow(unused_imports)]

use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::gui::state::{
    ActiveModal, ActiveWorkflow, AppSettings, AppView, MyApp, Theme, ToastKind,
};
use crate::app::runtime::Job;

impl MyApp {
    pub(crate) fn draw_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            // Global progress: a labeled bar with percentage shown whenever a
            // job is running (explicit Progress updates) or any job is in
            // flight (spinner fallback for jobs that don't stream progress).
            if let Some(p) = self.progress.clone() {
                let pct = (p.fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
                let elapsed = p.started_at.elapsed();
                let eta_str = if p.fraction > 0.01 && p.fraction < 1.0 {
                    let total_est = elapsed.as_secs_f64() / (p.fraction as f64);
                    let remaining = total_est - elapsed.as_secs_f64();
                    if remaining > 60.0 * 90.0 {
                        String::from(" (long running)")
                    } else if remaining > 60.0 {
                        format!(" (ETA: {:.0}m {:.0}s)", remaining / 60.0, remaining % 60.0)
                    } else {
                        format!(" (ETA: {:.0}s)", remaining)
                    }
                } else {
                    String::new()
                };
                ui.add(
                    egui::ProgressBar::new(p.fraction.clamp(0.0, 1.0))
                        .desired_width(ui.available_width())
                        .text(format!("{} - {}%{}", p.label, pct, eta_str)),
                );
                ui.add_space(2.0);
            } else if self.in_flight > 0 {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.small(format!(
                        "Working... ({} task{} in progress)",
                        self.in_flight,
                        if self.in_flight == 1 { "" } else { "s" }
                    ));
                });
                ui.add_space(2.0);
            }
            ui.horizontal(|ui| {
                if self.settings.remote_engine_url.is_empty() {
                    ui.colored_label(egui::Color32::LIGHT_GREEN, "🟢 Local");
                } else {
                    ui.colored_label(
                        egui::Color32::LIGHT_BLUE,
                        format!("🔵 Remote ({})", self.settings.remote_engine_url),
                    );
                }
                ui.separator();
                ui.small(&self.status);
                ui.separator();
                if self.total_pages > 0 {
                    ui.small(format!(
                        "Page {}/{}",
                        self.current_page + 1,
                        self.total_pages
                    ));
                    ui.separator();
                }
                ui.small(format!("DPI: {:.0}", self.current_page_dpi));
                ui.separator();
                ui.small(format!("Zoom: {:.0}%", self.zoom_factor * 100.0));
                // Telemetry Footer (Aligned Right)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(w) = &self.last_warning {
                        ui.colored_label(egui::Color32::YELLOW, format!("⚠ {w}"));
                        ui.separator();
                    }

                    ui.small(format!("RAM: {} MB", self.telemetry_ram_mb));
                    ui.separator();

                    let cpu_color = if self.telemetry_cpu > 80.0 {
                        egui::Color32::from_rgb(255, 80, 80)
                    } else if self.telemetry_cpu > 40.0 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::LIGHT_GREEN
                    };
                    ui.colored_label(cpu_color, format!("CPU: {:.1}%", self.telemetry_cpu));
                    ui.separator();

                    let engine_state = if self.stuck_detection.is_some() {
                        "STALLED"
                    } else if self.in_flight > 0 {
                        "BUSY"
                    } else {
                        "IDLE"
                    };
                    let engine_color = match engine_state {
                        "STALLED" => egui::Color32::from_rgb(255, 80, 80),
                        "BUSY" => egui::Color32::YELLOW,
                        _ => egui::Color32::LIGHT_GREEN,
                    };
                    ui.colored_label(engine_color, format!("ENG: {}", engine_state));
                    ui.separator();

                    let mut all_healthy = true;
                    if let Some(health) = &self.api_health {
                        for res in health {
                            if res.status
                                == crate::app::api_verification::VerificationStatus::Failed
                            {
                                all_healthy = false;
                            }
                        }
                    }
                    if self.api_health.is_some() {
                        if all_healthy {
                            ui.colored_label(egui::Color32::LIGHT_GREEN, "API: HEALTHY");
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(255, 80, 80), "API: DEGRADED");
                        }
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "API: UNKNOWN");
                    }
                });
            });
        });
    }

    #[allow(dead_code)]
    pub(crate) fn draw_left_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("left_panel")
            .width_range(180.0..=300.0)
            .show(ctx, |ui| {
                ui.heading("Navigation");
                ui.horizontal(|ui| {
                    if ui.button("◀").clicked() && self.current_page > 0 {
                        self.current_page -= 1;
                        self.request_render("current");
                    }
                    ui.label(format!(
                        "{} / {}",
                        self.current_page + 1,
                        self.total_pages.max(1)
                    ));
                    if ui.button("▶").clicked() && self.current_page + 1 < self.total_pages {
                        self.current_page += 1;
                        self.request_render("current");
                    }
                });

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for i in 0..self.total_pages {
                            let selected = i == self.current_page;
                            if ui
                                .selectable_label(selected, format!("Page {}", i + 1))
                                .clicked()
                            {
                                self.current_page = i;
                                self.request_render("current");
                            }
                        }
                    });

                ui.separator();
                ui.heading("Precision Content Editor");
                if let Some(block) = self.selected_block.clone() {
                    ui.small(format!(
                        "Font: {}",
                        if block.font.is_empty() {
                            "(unknown)"
                        } else {
                            &block.font
                        }
                    ));
                    ui.small(format!("Size: {:.1}", block.size));
                    ui.add_enabled(
                        false,
                        egui::TextEdit::multiline(&mut block.text.clone()).desired_rows(2),
                    );
                    ui.text_edit_multiline(&mut self.new_text);
                    if self.settings.advanced_mode {
                        ui.checkbox(
                            &mut self.settings.deep_font_replication,
                            "Deep Font Replication (AI)",
                        );
                    }
                } else {
                    ui.weak("Click any text on the canvas to edit.");
                }
            });
    }

    /// Generate a safe output path that never overwrites the input.
    pub fn safe_output_path(input: &std::path::Path, suffix: &str) -> std::path::PathBuf {
        let stem = input.file_stem().unwrap_or_default().to_string_lossy();
        let ext = input.extension().unwrap_or_default().to_string_lossy();
        let parent = input.parent().unwrap_or(std::path::Path::new("."));
        let mut candidate = parent.join(format!("{stem}_{suffix}.{ext}"));
        let mut counter = 1u32;
        while candidate.exists() {
            candidate = parent.join(format!("{stem}_{suffix}_{counter}.{ext}"));
            counter += 1;
        }
        candidate
    }

    #[allow(dead_code)]
    pub(crate) fn draw_batch_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Batch Processing Dashboard");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("📂 Select Directory").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.batch_folder_path = Some(path.clone());
                        self.batch_files.clear();
                        if let Ok(entries) = std::fs::read_dir(&path) {
                            for entry in entries.filter_map(|e| e.ok()) {
                                let p = entry.path();
                                if p.is_file() && p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) == Some("pdf".to_string()) {
                                    self.batch_files.push(p);
                                }
                            }
                        }
                    }
                }
                if let Some(path) = &self.batch_folder_path {
                    ui.label(format!("Selected: {}", path.display()));
                } else {
                    ui.label("Drag and drop a folder of statements here, or click to select a directory.");
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let has_files = !self.batch_files.is_empty();
                if ui.add_enabled(has_files, egui::Button::new("Extract All to JSON")).clicked() {
                    for file in &self.batch_files {
                        if let Err(e) = self.job_tx.send(Job::ExtractTransactions {
                            path: file.clone(),
                            parser_mode: self.settings.document_parser,
                        }) { tracing::error!("Runtime disconnected: {}", e); }
                        self.in_flight += 1;
                    }
                    self.toast(ToastKind::Info, format!("Queued {} extraction jobs", self.batch_files.len()));
                }
                if ui.add_enabled(has_files, egui::Button::new("Auto-Balance All")).clicked() {
                    for file in &self.batch_files {
                        let output = file.with_file_name(format!("{}_balanced.pdf", file.file_stem().unwrap_or_default().to_string_lossy()));
                        if let Err(e) = self.job_tx.send(Job::BalanceAndApplyAll {
                            input: file.clone(),
                            output,
                            auto_apply: true,
                        }) { tracing::error!("Runtime disconnected: {}", e); }
                        self.in_flight += 1;
                    }
                    self.toast(ToastKind::Info, format!("Queued {} balancing jobs", self.batch_files.len()));
                }
                if ui.add_enabled(has_files, egui::Button::new("Verify All against Originals")).clicked() {
                    let pairs = Self::pair_originals_and_edited(&self.batch_files);
                    if pairs.is_empty() {
                        self.toast(ToastKind::Warn, "No paired _original/_edited PDFs found in folder.");
                    } else {
                        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                        for (original, edited) in &pairs {
                            let stem = edited.file_stem().unwrap_or_default().to_string_lossy().to_string();
                            if let Err(e) = self.job_tx.send(Job::Verify {
                                original: original.clone(),
                                edited: edited.clone(),
                                output_dir: PathBuf::from("audit/verify/batch").join(&timestamp).join(&stem),
                                intended_edits: Vec::new(),
                                use_pdfrest: self.settings.verification_renderer == crate::app::config::VerificationMode::PdfRestCloud,
                                pdfrest_key: self.config.pdfrest_api_key.clone(),
                                auto_match_dpi: self.settings.auto_match_dpi,
                            }) { tracing::error!("Runtime disconnected: {}", e); }
                            self.in_flight += 1;
                        }
                        self.toast(ToastKind::Info, format!("Queued {} verification job(s)", pairs.len()));
                    }
                }
            });

            ui.add_space(10.0);

            if !self.batch_files.is_empty() {
                ui.heading(format!("{} PDF(s) found", self.batch_files.len()));
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for file in &self.batch_files {
                        ui.label(file.file_name().unwrap_or_default().to_string_lossy());
                    }
                });
            }
        });
    }

    #[allow(dead_code)]
    pub(crate) fn draw_audit_explorer_view(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Audit Explorer unavailable in v1");
            ui.label("Use the immutable exported audit and verification evidence. No simulated explorer is provided.");
        });
    }

    /// Stage 13 / Item #12: confirmation modals.
    pub(crate) fn draw_toasts(&mut self, ctx: &egui::Context) -> Option<String> {
        let mut clicked_id = None;
        // Drop expired
        let now = Instant::now();
        self.toasts.retain(|t| t.expires_at > now);
        if self.toasts.is_empty() {
            return None;
        }

        egui::Area::new("toasts".into())
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -32.0))
            .show(ctx, |ui| {
                ui.vertical_centered_justified(|ui| {
                    let p = self.settings.theme.palette();
                    for toast in self.toasts.iter().rev().take(5) {
                        let bg = match toast.kind {
                            ToastKind::Info => p.info,
                            ToastKind::Warn => p.warn,
                            ToastKind::Error => p.error,
                            ToastKind::Success => p.success,
                        };
                        let icon = match toast.kind {
                            ToastKind::Info => "ℹ",
                            ToastKind::Warn => "⚠",
                            ToastKind::Error => "✗",
                            ToastKind::Success => "✓",
                        };

                        // Remaining lifetime based alpha
                        let remaining = toast.expires_at.saturating_duration_since(now);
                        let base_alpha = (remaining.as_millis() as f32 / 6000.0).clamp(0.0, 1.0);

                        // Slide-in / slide-out animation
                        let anim_id = egui::Id::new("toast").with(&toast.text);
                        let target = if base_alpha > 0.05 { 1.0 } else { 0.0 };
                        let slide = ctx.animate_value_with_time(anim_id, target, 0.3);

                        let final_alpha = (base_alpha.min(slide) * 230.0) as u8;
                        if final_alpha == 0 {
                            continue;
                        }

                        let bg = egui::Color32::from_rgba_unmultiplied(
                            bg.r(),
                            bg.g(),
                            bg.b(),
                            final_alpha,
                        );
                        let fg = egui::Color32::from_white_alpha((255.0 * slide) as u8);

                        ui.horizontal(|ui| {
                            // Pushes the toast from the right to slide it in
                            ui.add_space((1.0 - slide) * 300.0);

                            egui::Frame::none()
                                .fill(bg)
                                .rounding(10.0)
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    egui::Color32::from_white_alpha((40.0 * slide) as u8),
                                ))
                                .inner_margin(egui::vec2(12.0, 8.0))
                                .shadow(egui::epaint::Shadow {
                                    offset: egui::vec2(0.0, 4.0 * slide),
                                    blur: 12.0 * slide,
                                    spread: 0.0,
                                    color: egui::Color32::from_black_alpha((60.0 * slide) as u8),
                                })
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(fg, icon);
                                        ui.colored_label(fg, &toast.text);
                                        if let Some(label) = &toast.action_label {
                                            ui.add_space(8.0);
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new(label).color(fg),
                                                    )
                                                    .fill(egui::Color32::from_black_alpha(100)),
                                                )
                                                .clicked()
                                            {
                                                clicked_id = toast.action_id.clone();
                                            }
                                        }
                                    });
                                });
                        });
                        ui.add_space(6.0 * slide);
                    }
                });
            });

        if let Some(id) = &clicked_id {
            self.toasts.retain(|t| t.action_id.as_ref() != Some(id));
        }

        clicked_id
    }
}
