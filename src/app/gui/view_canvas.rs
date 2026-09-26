//! Central document canvas and rendering views.
#![allow(unused_imports)]

use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::gui::state::{ActiveModal, ActiveWorkflow, MyApp, TextBlock, ToastKind};
use crate::app::runtime::{Job, PythonJob};

impl MyApp {
    pub(crate) fn draw_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Toolbar above canvas
            ui.horizontal(|ui| {
                if ui.button("🔍-").clicked() {
                    self.zoom_factor = (self.zoom_factor * 0.85).clamp(0.1, 5.0);
                    self.fit_to_view = false;
                }
                if ui.button("🔍+").clicked() {
                    self.zoom_factor = (self.zoom_factor * 1.15).clamp(0.1, 5.0);
                    self.fit_to_view = false;
                }
                if ui.button("Fit").clicked() {
                    self.fit_to_view = true;
                }
                if ui.button("100%").clicked() {
                    self.zoom_factor = 1.0;
                    self.pan_offset = egui::Vec2::ZERO;
                    self.fit_to_view = false;
                }
                ui.separator();
                ui.checkbox(&mut self.show_curtain, "Curtain Diff");
                if self.show_curtain {
                    ui.add(egui::Slider::new(&mut self.curtain_ratio, 0.0..=1.0).text("split"));
                }
            });

            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                let (response, painter) = ui.allocate_painter(
                    ui.available_size(),
                    egui::Sense::drag().union(egui::Sense::click()),
                );

                // Zoom - Ctrl+wheel
                let zoom_scroll = ui.input(|i| {
                    if i.modifiers.command {
                        i.smooth_scroll_delta.y
                    } else {
                        0.0
                    }
                });
                if zoom_scroll != 0.0 {
                    self.zoom_factor = (self.zoom_factor + zoom_scroll * 0.002).clamp(0.1, 5.0);
                    self.fit_to_view = false;
                }

                // Pan - any drag (primary, middle, etc.)
                if response.dragged() {
                    self.pan_offset += response.drag_delta();
                    self.fit_to_view = false;
                }

                if let Some(texture) = self.current_page_texture.clone() {
                    let tex_size = texture.size_vec2();
                    if self.fit_to_view {
                        self.fit_zoom_to_view(response.rect.size(), tex_size);
                    }
                    let size = tex_size * self.zoom_factor;
                    let center = response.rect.center() + self.pan_offset;
                    let rect = egui::Rect::from_center_size(center, size);

                    painter.image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );

                    // Curtain diff: paint the "after" texture clipped to ratio
                    if self.show_curtain {
                        if let Some(after) = self.after_texture.clone() {
                            let split_x = rect.min.x + rect.width() * self.curtain_ratio;
                            let after_rect =
                                egui::Rect::from_min_max(egui::pos2(split_x, rect.min.y), rect.max);
                            let uv_min = egui::pos2(self.curtain_ratio, 0.0);
                            painter.image(
                                after.id(),
                                after_rect,
                                egui::Rect::from_min_max(uv_min, egui::pos2(1.0, 1.0)),
                                egui::Color32::WHITE,
                            );
                            painter.line_segment(
                                [
                                    egui::pos2(split_x, rect.min.y),
                                    egui::pos2(split_x, rect.max.y),
                                ],
                                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 200, 0)),
                            );
                        }

                        // Hotspot Highlighting: draw red/green boxes around modified regions
                        if let Some((w, h)) = self.current_page_size_pts {
                            for edit in &self.workflow_edits {
                                if edit.page == self.current_page {
                                    let [x0, y0, x1, y1] = edit.bbox;
                                    let sx0 = rect.min.x + (x0 / w) * size.x;
                                    let sy0 = rect.min.y + (y0 / h) * size.y;
                                    let sx1 = rect.min.x + (x1 / w) * size.x;
                                    let sy1 = rect.min.y + (y1 / h) * size.y;

                                    let item_rect = egui::Rect::from_min_max(
                                        egui::pos2(sx0, sy0),
                                        egui::pos2(sx1, sy1),
                                    );

                                    let split_x = rect.min.x + rect.width() * self.curtain_ratio;

                                    if sx0 < split_x {
                                        let mut r = item_rect;
                                        r.max.x = r.max.x.min(split_x);
                                        painter.rect_stroke(r, 2.0, egui::Stroke::new(2.0_f32, egui::Color32::from_rgba_premultiplied(255, 50, 50, 150)));
                                    }

                                    if sx1 > split_x {
                                        let mut r = item_rect;
                                        r.min.x = r.min.x.max(split_x);
                                        painter.rect_stroke(r, 2.0, egui::Stroke::new(2.0_f32, egui::Color32::from_rgba_premultiplied(50, 255, 50, 150)));
                                    }

                                    // Phase 2 - Stage 1: Smart Alignment Guides (on hover)
                                    if let Some(pos) = response.hover_pos() {
                                        if item_rect.contains(pos) {
                                            // Draw crosshair alignment lines matching Figma's smart guides
                                            let p = self.settings.theme.palette();
                                            let guide_color = p.accent.linear_multiply(0.4);

                                            // Horizontal guide through center
                                            painter.hline(response.rect.min.x..=response.rect.max.x, item_rect.center().y, egui::Stroke::new(1.0_f32, guide_color));

                                            // Vertical guide through center
                                            painter.vline(item_rect.center().x, response.rect.min.y..=response.rect.max.y, egui::Stroke::new(1.0_f32, guide_color));

                                            // Highlight the bounds
                                            painter.rect_stroke(item_rect, 0.0, egui::Stroke::new(2.0_f32, p.accent));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Click -> resolve text block via Python
                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            self.last_click_pos = Some(pos);
                            let relative = pos - rect.min;
                            let (x, y) = if let Some((w, h)) = self.current_page_size_pts {
                                (relative.x * w / size.x, relative.y * h / size.y)
                            } else {
                                (relative.x / self.zoom_factor, relative.y / self.zoom_factor)
                            };
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            if self
                                .job_tx
                                .send(Job::Python(
                                    PythonJob::FindTextBlockAtClick {
                                        pdf_path: self
                                            .current_pdf_path
                                            .to_string_lossy()
                                            .to_string(),
                                        page_num: self.current_page,
                                        x,
                                        y,
                                    },
                                    tx,
                                ))
                                .is_ok()
                            {
                                self.pending_python = Some(rx);
                                self.in_flight += 1;
                            }
                        }
                    }

                    // Selected bbox highlight
                    if let Some(block) = &self.selected_block {
                        if block.page == self.current_page {
                            let (sx, sy) = if let Some((w, h)) = self.current_page_size_pts {
                                (size.x / w, size.y / h)
                            } else {
                                (self.zoom_factor, self.zoom_factor)
                            };
                            let min = rect.min + egui::vec2(block.bbox[0] * sx, block.bbox[1] * sy);
                            let max = rect.min + egui::vec2(block.bbox[2] * sx, block.bbox[3] * sy);
                            painter.rect_stroke(
                                egui::Rect::from_min_max(min, max),
                                4.0,
                                egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 200, 0)),
                            );
                        }
                    }

                    // Stage 5 / Item #20: live diff overlay during preview.
                    // Translucent yellow over each `will_change` bbox on the
                    // current page; tooltip shows old -> new.
                    if let Some(preview) = self.workflow_preview.clone() {
                        let (sx, sy) = if let Some((w, h)) = self.current_page_size_pts {
                            (size.x / w, size.y / h)
                        } else {
                            (self.zoom_factor, self.zoom_factor)
                        };
                        let mouse = response.hover_pos();
                        for prow in preview
                            .rows
                            .iter()
                            .filter(|r| r.will_change && r.page == self.current_page)
                        {
                            // Find the underlying transaction so we can use its bbox.
                            let Some(tx) = self.workflow_transactions.iter().find(|t| {
                                t.page == prow.page && t.line_on_page == prow.line_on_page
                            }) else {
                                continue;
                            };
                            let Some(bbox) = tx.bbox else {
                                continue;
                            };
                            let min = rect.min + egui::vec2(bbox[0] * sx, bbox[1] * sy);
                            let max = rect.min + egui::vec2(bbox[2] * sx, bbox[3] * sy);
                            let cell = egui::Rect::from_min_max(min, max);
                            // Translucent yellow fill + amber border.
                            painter.rect_filled(
                                cell,
                                2.0,
                                egui::Color32::from_rgba_unmultiplied(255, 220, 0, 70),
                            );
                            painter.rect_stroke(
                                cell,
                                2.0,
                                egui::Stroke::new(
                                    1.0_f32,
                                    egui::Color32::from_rgb(220, 180, 0),
                                ),
                            );
                            // Hover tooltip with the diff text (Phase 2 - Stage 2: Advanced Hover Cards)
                            if let Some(m) = mouse {
                                if cell.contains(m) {
                                    let old_str = prow
                                        .old_running_balance
                                        .map(|v| format!("{v:.2}"))
                                        .unwrap_or_else(|| "-".into());
                                    let new_str = prow
                                        .new_running_balance
                                        .map(|v| format!("{v:.2}"))
                                        .unwrap_or_else(|| "-".into());

                                    let cell_resp = ui.allocate_rect(cell, egui::Sense::hover());
                                    cell_resp.on_hover_ui(|ui| {
                                        let p = self.settings.theme.palette();
                                        egui::Frame::none()
                                            .inner_margin(egui::vec2(12.0, 10.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new("🔍 Math Correction").color(p.accent).strong());
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("P{} L{}", prow.page + 1, prow.line_on_page + 1)).weak().size(12.0));
                                                    });
                                                });
                                                ui.add_space(6.0);

                                                // Display old vs new with clear coloring
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new(old_str).color(p.warn).strikethrough());
                                                    ui.label(egui::RichText::new(" ➔ ").color(p.weak));
                                                    ui.label(egui::RichText::new(new_str).color(p.success).strong());
                                                });

                                                ui.add_space(6.0);
                                                ui.small(egui::RichText::new("Balance automatically re-calculated by Engine").color(p.text.linear_multiply(0.7)));
                                            });
                                    });
                                }
                            }
                        }
                    }

                    // Minimap overlay (Phase 2 - Stage 1)
                    if self.zoom_factor > 1.05 {
                        let minimap_w = 120.0;
                        let minimap_h = minimap_w * (tex_size.y / tex_size.x);
                        let minimap_size = egui::vec2(minimap_w, minimap_h);

                        let minimap_rect = egui::Rect::from_min_size(
                            response.rect.max - minimap_size - egui::vec2(24.0, 24.0),
                            minimap_size,
                        );

                        // Background
                        painter.rect_filled(minimap_rect, 4.0, egui::Color32::from_black_alpha(180));
                        painter.rect_stroke(minimap_rect, 4.0, egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(30)));

                        // Render full page texture scaled down
                        painter.image(
                            texture.id(),
                            minimap_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );

                        // Indicator box showing the visible portion
                        let vis_min_uv = (response.rect.min - rect.min) / rect.size();
                        let vis_max_uv = (response.rect.max - rect.min) / rect.size();

                        let clamped_min = vis_min_uv.clamp(egui::vec2(0.0, 0.0), egui::vec2(1.0, 1.0));
                        let clamped_max = vis_max_uv.clamp(egui::vec2(0.0, 0.0), egui::vec2(1.0, 1.0));

                        let ind_rect = egui::Rect::from_min_max(
                            minimap_rect.min + clamped_min * minimap_rect.size(),
                            minimap_rect.min + clamped_max * minimap_rect.size(),
                        );

                        painter.rect_filled(ind_rect, 2.0, self.settings.theme.palette().accent.linear_multiply(0.2));
                        painter.rect_stroke(ind_rect, 2.0, egui::Stroke::new(1.5_f32, self.settings.theme.palette().accent));
                    }
                } else {
                    // Welcome / empty placeholder
                    self.draw_empty_canvas(ui, response.rect, &painter);
                }
            });
            let mut dock = egui::Area::new(egui::Id::new("floating_action_dock")).order(egui::Order::Foreground);
            if self.selected_block.is_some() {
                if let Some(pos) = self.last_click_pos {
                    dock = dock.current_pos(pos + egui::vec2(20.0, 20.0));
                } else {
                    dock = dock.anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0));
                }
            } else {
                dock = dock.anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0));
            }

            dock.show(ctx, |ui| {
                if self.selected_block.is_some() || !self.proposed_changes.is_empty() {
                    let p = self.settings.theme.palette();
                    egui::Frame::window(ui.style())
                        .fill(p.surface.linear_multiply(0.85))
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(0.0, 10.0),
                            blur: 30.0,
                            spread: 0.0,
                            color: egui::Color32::from_black_alpha(80),
                        })
                        .rounding(16.0)
                        .stroke(egui::Stroke::new(1.0_f32, p.text.linear_multiply(0.1)))
                        .inner_margin(egui::Margin::symmetric(20.0, 16.0))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                // Primary Contextual Action
                                if self.selected_block.is_some() {
                                    ui.horizontal(|ui| {
                                        let apply_btn = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("🎯 Apply Single Edit")
                                                    .color(p.bg).strong()
                                            )
                                            .fill(p.accent)
                                            .rounding(8.0)
                                            .min_size(egui::vec2(160.0, 36.0))
                                        );

                                        if apply_btn.on_hover_text("Replace the selected text and instantly verify math + fidelity.").clicked() {
                                            if let Some(block) = self.selected_block.clone() {
                                                let input = if self.current_pdf_path.exists() {
                                                    self.current_pdf_path.clone()
                                                } else {
                                                    std::path::PathBuf::from(&self.input_path)
                                                };
                                                let edit = crate::engine::workflow::UserEdit {
                                                    page: self.current_page,
                                                    line_on_page: 0,
                                                    bbox: block.bbox,
                                                    old_text: block.text.clone(),
                                                    new_text: self.new_text.clone(),
                                                    field: crate::engine::workflow::EditField::Description,
                                                };
                                                if let Err(e) = self.dispatch_workflow_job(Job::WorkflowConfirmAndRender {
                                                    input,
                                                    output: std::path::PathBuf::from(&self.output_path),
                                                    edits: vec![edit],
                                                    original_transactions: self.workflow_transactions.clone(),
                                                    opening_balance: self.workflow_validation.as_ref().map(|v| v.opening_balance).unwrap_or_default(),
                                                    expected_closing: self.workflow_validation.as_ref().and_then(|v| {
                                                        if v.closing_balance.abs() > rust_decimal::Decimal::ZERO { Some(v.closing_balance) } else { None }
                                                    }),
                                                    deep_font_replication: self.settings.deep_font_replication,
                                                    max_visual_attempts: self.settings.max_visual_attempts.min(3),
                                                    visual_threshold: self.settings.visual_diff_threshold.max(0.05),
                                                    ignore_font_coverage: false,
                                                    ignore_visual_fidelity: false,
                                                }) { tracing::error!("Runtime disconnected: {}", e); }
                                                self.in_flight += 1;
                                            }
                                        }

                                        ui.add_space(8.0);

                                    });

                                    ui.add_space(12.0);
                                    let mut rect = ui.min_rect();
                                    rect.max.y = rect.min.y + 1.0;
                                    ui.painter().rect_filled(rect, 0.0, p.text.linear_multiply(0.05));
                                    ui.add_space(12.0);
                                }

                                // Global Action Row
                                ui.horizontal(|ui| {
                                    let adjust_btn = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("⚖ Auto-Balance Statement")
                                                .color(p.panel).strong()
                                        )
                                        .fill(p.success)
                                        .rounding(8.0)
                                        .min_size(egui::vec2(200.0, 36.0))
                                    );
                                    if adjust_btn.on_hover_text("Computes minimal adjustments for the entire statement and applies them automatically.").clicked() {
                                        let input = if self.current_pdf_path.exists() { self.current_pdf_path.clone() } else { std::path::PathBuf::from(&self.input_path) };
                                        if input.as_os_str().is_empty() || !input.exists() {
                                            self.toast(ToastKind::Error, "Open a PDF first.");
                                        } else {
                                            if let Err(e) = self.job_tx.send(Job::BalanceAndApplyAll {
                                                input, output: std::path::PathBuf::from(&self.output_path), auto_apply: true,
                                            }) { tracing::error!("Runtime disconnected: {}", e); }
                                            self.in_flight += 1;
                                            self.status = "Auto-balancing entire statement...".into();
                                            self.toast(ToastKind::Info, "Auto-balancing entire statement...");
                                        }
                                    }

                                    ui.add_space(8.0);
                                    if ui.add(egui::Button::new(egui::RichText::new("📅 Dates").color(p.text)).fill(p.bg).rounding(8.0).min_size(egui::vec2(80.0, 36.0))).on_hover_text("Adjust all transaction dates").clicked() {
                                        self.active_modal = ActiveModal::DateAdjust;
                                    }

                                    ui.add_space(8.0);
                                    if ui.add(egui::Button::new(egui::RichText::new("🔄 Transfer").color(p.text)).fill(p.bg).rounding(8.0).min_size(egui::vec2(90.0, 36.0))).on_hover_text("Transfer from another PDF").clicked() {
                                        self.active_modal = ActiveModal::Transfer;
                                    }
                                });
                            });
                        });
                }
            });
        });
    }

    pub(crate) fn draw_empty_canvas(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        painter: &egui::Painter,
    ) {
        let p = self.settings.theme.palette();

        // --- 1. Background Gradient ---
        painter.rect_filled(rect, 0.0, p.bg);

        // Ambient glows in the background
        let center = rect.center();
        let glow_radius = 400.0;
        let glow_color = p.accent.linear_multiply(0.05);

        painter.circle_filled(center + egui::vec2(-200.0, -150.0), glow_radius, glow_color);
        painter.circle_filled(
            center + egui::vec2(250.0, 200.0),
            glow_radius * 0.8,
            p.success.linear_multiply(0.03),
        );

        // --- 2. Asynchronous Loading State ---
        if self.current_pdf_path.exists() {
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        egui::Frame::none()
                            .inner_margin(egui::Margin::same(32.0))
                            .rounding(egui::Rounding::same(24.0))
                            .fill(p.surface.linear_multiply(0.8))
                            .shadow(egui::epaint::Shadow {
                                offset: egui::vec2(0.0, 12.0),
                                blur: 24.0,
                                spread: 0.0,
                                color: egui::Color32::from_black_alpha(40),
                            })
                            .stroke(egui::Stroke::new(1.0_f32, p.surface.linear_multiply(0.5)))
                            .show(ui, |ui| {
                                // Phase 2 - Stage 2: Shimmering Skeleton Loader
                                let time = ui.input(|i| i.time);
                                let alpha = ((time * 4.0).sin() as f32 * 0.3 + 0.7) * 0.15;
                                let skel_color = p.text.linear_multiply(alpha);

                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(200.0, 80.0),
                                    egui::Sense::hover(),
                                );
                                let painter = ui.painter();
                                painter.rect_filled(
                                    egui::Rect::from_min_size(rect.min, egui::vec2(200.0, 24.0)),
                                    4.0,
                                    skel_color,
                                );
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        rect.min + egui::vec2(0.0, 40.0),
                                        egui::vec2(160.0, 14.0),
                                    ),
                                    4.0,
                                    skel_color,
                                );
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        rect.min + egui::vec2(0.0, 64.0),
                                        egui::vec2(120.0, 14.0),
                                    ),
                                    4.0,
                                    skel_color,
                                );
                                ui.ctx().request_repaint();

                                ui.add_space(16.0);
                                ui.label(
                                    egui::RichText::new("Rendering document...")
                                        .color(p.text)
                                        .size(18.0)
                                        .strong(),
                                );
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("Applying AI vision and structure mapping")
                                        .color(p.weak)
                                        .size(14.0),
                                );
                            });
                    });
                });
            });
            return;
        }

        // --- 3. Welcome Glass Panel ---
        let panel_width = 460.0;
        let panel_height = 420.0;
        let panel_rect =
            egui::Rect::from_center_size(center, egui::vec2(panel_width, panel_height));

        // Glassmorphism effect
        painter.rect(
            panel_rect,
            egui::Rounding::same(24.0),
            p.surface.linear_multiply(0.85),
            egui::Stroke::new(1.5_f32, p.text.linear_multiply(0.1)),
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(panel_rect), |ui| {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                // Icon
                ui.label(egui::RichText::new("✨").size(48.0));
                ui.add_space(16.0);

                // Title
                ui.label(
                    egui::RichText::new("Antigravity Statement Forensics")
                        .size(26.0)
                        .strong()
                        .color(p.text),
                );

                ui.add_space(8.0);

                // Subtitle
                ui.label(
                    egui::RichText::new(
                        "High-Fidelity Ledger Processing & AI-Driven Reconciliation",
                    )
                    .size(14.0)
                    .color(p.weak),
                );

                ui.add_space(40.0);

                // Primary Action Button
                let btn = egui::Button::new(
                    egui::RichText::new("⊕ Initialize Workspace Session")
                        .size(16.0)
                        .strong()
                        .color(p.bg),
                )
                .min_size(egui::vec2(320.0, 52.0))
                .rounding(egui::Rounding::same(12.0))
                .fill(p.accent);

                if ui
                    .add(btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PDF", &["pdf"])
                        .pick_file()
                    {
                        self.open_pdf(path);
                    }
                }

                ui.add_space(16.0);

                ui.label(
                    egui::RichText::new("or drag and drop a PDF file here")
                        .size(13.0)
                        .italics()
                        .color(p.weak.linear_multiply(0.7)),
                );

                ui.add_space(30.0);

                ui.horizontal(|ui| {
                    ui.add_space(70.0); // Center the secondary actions
                    if ui
                        .button(
                            egui::RichText::new("⟲ Restore Previous Session")
                                .size(13.0)
                                .color(p.text),
                        )
                        .clicked()
                    {
                        let auto = std::path::PathBuf::from("audit").join("history.json");
                        if auto.exists() {
                            if let Err(e) =
                                self.job_tx.send(crate::app::runtime::Job::LoadHistory {
                                    input: auto.clone(),
                                })
                            {
                                tracing::error!("Runtime disconnected: {}", e);
                            }
                            self.in_flight += 1;
                            self.toast(
                                ToastKind::Info,
                                format!("Resuming from {}", auto.display()),
                            );
                        } else {
                            self.toast(ToastKind::Warn, "No previous session found.");
                        }
                    }
                    ui.add_space(10.0);
                    if ui
                        .button(
                            egui::RichText::new("📝 Recover Editing Draft")
                                .size(13.0)
                                .color(p.text),
                        )
                        .clicked()
                    {
                        self.resume_workflow_draft();
                    }
                });
            });
        });
    }
}
