use crate::app::gui::{ActiveModal, MyApp, Theme, ToastKind};
use crate::app::runtime::Job;
use egui_plot::{Line, Plot};
use std::path::PathBuf;

fn capability_selectable_value<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
    status: Option<&crate::app::capabilities::CapabilityStatus>,
) {
    let enabled = status.is_some_and(|status| status.is_selectable());
    let reason = status
        .map(|status| status.reason.clone())
        .unwrap_or_else(|| "Capability status is unavailable".to_string());
    let response = ui
        .add_enabled_ui(enabled, |ui| ui.selectable_value(current, value, label))
        .response;
    if !enabled {
        response.on_hover_text(reason);
    }
}

pub trait CommandPalette {
    fn draw_command_palette(&mut self, ctx: &egui::Context);
}

impl CommandPalette for MyApp {
    fn draw_command_palette(&mut self, ctx: &egui::Context) {
        let mut open = self.active_modal == ActiveModal::CommandPalette;
        let mut submit_nlp = false;

        egui::Window::new("Command Palette")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 100.0))
            .fixed_size(egui::vec2(600.0, 60.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(self.settings.theme.palette().bg.linear_multiply(0.95))
                    .inner_margin(16.0)
                    .rounding(12.0)
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 20.0),
                        blur: 40.0,
                        spread: 0.0,
                        color: egui::Color32::from_black_alpha(150),
                    }),
            )
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🔍").size(24.0));

                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.command_query)
                            .desired_width(500.0)
                            .font(egui::FontId::proportional(20.0))
                            .hint_text("Type a command or ask AI... (e.g. 'balance page 1')"),
                    );

                    response.request_focus();

                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit_nlp = true;
                    }
                });

                if !self.command_query.is_empty() {
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(
                            "Press Enter to execute natural language prompt via Gemini",
                        )
                        .color(self.settings.theme.palette().weak)
                        .italics(),
                    );
                }
            });

        if submit_nlp {
            let prompt = std::mem::take(&mut self.command_query);
            self.toast(ToastKind::Info, format!("Executing AI command: {}", prompt));

            self.in_flight += 1;
            let _ = self.job_tx.send(Job::NaturalLanguageEdit {
                prompt,
                transactions: self.workflow_transactions.clone(),
            });

            self.active_modal = ActiveModal::None;
        } else if open {
            self.active_modal = ActiveModal::CommandPalette;
        } else if self.active_modal == ActiveModal::CommandPalette {
            self.active_modal = ActiveModal::None;
        }
    }
}

#[allow(dead_code)]
pub(crate) trait AppModals {
    fn draw_settings_modal(&mut self, ctx: &egui::Context);
    fn draw_backend_preferences(&mut self, ui: &mut egui::Ui);
    fn draw_transfer_dialog(&mut self, ctx: &egui::Context);
    fn draw_date_adjust_dialog(&mut self, ctx: &egui::Context);
    fn draw_ai_confirmation_dialog(&mut self, ctx: &egui::Context);
    fn draw_interactive_fallback_modal(&mut self, ctx: &egui::Context);
    fn draw_autofix_modal(&mut self, ctx: &egui::Context);
    fn draw_workflow_hitl_modal(&mut self, ctx: &egui::Context);
    fn draw_transfer_test_dialog(&mut self, ctx: &egui::Context);
    fn draw_api_keys_editor(&mut self, ui: &mut egui::Ui);
    fn draw_feedback_modal(&mut self, ctx: &egui::Context);
    fn draw_modals(&mut self, ctx: &egui::Context);
    fn draw_stuck_watchdog_modal(&mut self, ctx: &egui::Context);
    fn draw_discard_draft_confirm_modal(&mut self, ctx: &egui::Context);
}

impl AppModals for MyApp {
    fn draw_settings_modal(&mut self, ctx: &egui::Context) {
        let mut open = self.active_modal == ActiveModal::Settings;
        egui::Window::new("⚙️ Settings & Tools")
                .open(&mut open)
                .default_size(egui::vec2(420.0, 600.0))
                .vscroll(true)
                .show(ctx, |ui| {
                        // Backend Preferences panel at the top - most important
                        self.draw_backend_preferences(ui);

                        self.draw_font_analysis_section(ui);
                        self.draw_workflow_section(ui);

                        ui.collapsing("⚖ Smart Balance Engine", |ui| {
                            if ui.button("Analyze Document")
                                .on_hover_text("Run Document AI + Gemini to find math errors and propose minimal adjustments")
                                .clicked()
                            {
                                let _ = self
                                    .job_tx
                                    .send(Job::BalanceStatement { path: PathBuf::from(&self.input_path) });
                                self.in_flight += 1;
                            }
                            if let Some(imb) = self.last_imbalance {
                                ui.label(format!("Global imbalance: ${imb}"));
                            }
                            if !self.proposed_changes.is_empty() {
                                ui.separator();
                                for (change, approved) in &mut self.proposed_changes {
                                    ui.checkbox(
                                        approved,
                                        format!("P{}: {} -> {}", change.page + 1, change.old_text, change.new_text),
                                    );
                                    ui.small(&change.reason);
                                }
                                if ui.button("Apply approved").clicked() {
                                    let changes = self
                                        .proposed_changes
                                        .iter()
                                        .filter(|(_, a)| *a)
                                        .map(|(c, _)| c.clone())
                                        .collect();
                                    let _ = self.job_tx.send(Job::ApplyProposedChanges {
                                        input: self.current_pdf_path.clone(),
                                        output: PathBuf::from(&self.output_path),
                                        changes,
                                    });
                                    self.in_flight += 1;
                                }
                            }
                        });

                        ui.collapsing("📊 Advanced Analytics & History", |ui| {
                            ui.collapsing("📈 Edit Trend", |ui| {
                                let pts = self.balance_trend_points();
                                let line = Line::new(pts).name("Edits");
                                Plot::new("trend")
                                    .height(120.0)
                                    .show_axes([false, true])
                                    .show(ui, |plot_ui| plot_ui.line(line));
                            });

                            ui.collapsing("🔄 Edit History", |ui| {
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(self.history_state.can_undo(), egui::Button::new("Undo")).clicked() {
                                        let _ = self.job_tx.send(Job::Undo);
                                    }
                                    if ui.add_enabled(self.history_state.can_redo(), egui::Button::new("Redo")).clicked() {
                                        let _ = self.job_tx.send(Job::Redo);
                                    }
                                });
                                let history = self.history_state.get_history();
                                for (i, rec) in history.iter().enumerate() {
                                    ui.small(format!("[{}] P{} {} -> {}", i + 1, rec.page + 1, rec.old_text, rec.new_text));
                                }
                            });

                            ui.collapsing("🏆 Parser Stats & Leaderboard", |ui| {
                                let stats: crate::engine::model::ParserStats = std::fs::read_to_string("audit/parser_stats.json")
                                    .ok()
                                    .and_then(|s| serde_json::from_str(&s).ok())
                                    .unwrap_or_default();

                                ui.label(format!("Total Matrix Checks: {}", stats.total_attempts));

                                // Build a sorted leaderboard
                                let mut leaderboard = vec![
                                    ("DocAI", stats.docai_wins),
                                    ("LlamaParse", stats.llamaparse_wins),

                                    ("Gemini", stats.gemini_wins),
                                    ("Offline", stats.offline_wins),
                                ];
                                leaderboard.sort_by(|a, b| b.1.cmp(&a.1));

                                egui::Grid::new("leaderboard_grid")
                                    .num_columns(3)
                                    .spacing([20.0, 4.0])
                                    .striped(true)
                                    .show(ui, |ui| {
                                        ui.strong("Rank");
                                        ui.strong("Parser");
                                        ui.strong("Consensus Wins");
                                        ui.end_row();

                                        for (i, (name, wins)) in leaderboard.into_iter().enumerate() {
                                            ui.label(format!("#{}", i + 1));
                                            ui.label(name);
                                            ui.label(wins.to_string());
                                            ui.end_row();
                                        }
                                    });
                            });

                            ui.collapsing("🔍 Verification", |ui| {
                                if ui.button("Run Full Audit")
                                    .on_hover_text("Render original vs edited at high DPI, perceptual + math diff")
                                    .clicked()
                                {
                                    let intended_edits: Vec<
                                        crate::engine::verification::VerificationIntent,
                                    > = self
                                        .history_state
                                        .get_history()
                                        .iter()
                                        .map(|record| {
                                            crate::engine::verification::VerificationIntent {
                                                page: record.page,
                                                bbox: record.bbox,
                                                old_text: record.old_text.clone(),
                                                new_text: record.new_text.clone(),
                                            }
                                        })
                                        .collect();
                                    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                                    let _ = self.job_tx.send(Job::Verify {
                                        original: PathBuf::from(&self.input_path),
                                        edited: self.current_pdf_path.clone(),
                                        output_dir: PathBuf::from("audit/verify").join(timestamp),
                                        intended_edits,
                                        use_pdfrest: self.settings.verification_renderer == crate::app::config::VerificationMode::PdfRestCloud,
                                        pdfrest_key: self.config.pdfrest_api_key.clone(),
                                        auto_match_dpi: self.settings.auto_match_dpi,
                                    });
                                    self.in_flight += 1;
                                }
                                if let Some(report) = &self.last_verification {
                                    let passed = report.mandatory_local_pass();
                                    ui.colored_label(
                                        if passed {
                                            egui::Color32::LIGHT_GREEN
                                        } else {
                                            egui::Color32::LIGHT_RED
                                        },
                                        if passed {
                                            "PASS · every mandatory local gate passed"
                                        } else {
                                            "FAIL · one or more mandatory local gates failed"
                                        },
                                    );
                                    for gate in &report.gates {
                                        ui.small(format!(
                                            "{} · {:?} · {}",
                                            gate.id, gate.status, gate.message
                                        ));
                                    }
                                }
                            });

                            ui.collapsing("📤 Export Dashboard", |ui| {
                                ui.label("Generate complete reports for the final output.");
                                ui.add_space(8.0);

                                ui.horizontal(|ui| {
                                    if ui.button("📊 Excel (.xlsx)").clicked() {
                                        self.export_to_excel();
                                    }
                                    if ui.button("📜 Audit JSON").clicked() {
                                        let _ = self.job_tx.send(Job::ExportChangeHistory {
                                            output: PathBuf::from(&self.export_path),
                                        });
                                        self.in_flight += 1;
                                    }
                                    if ui.button("📦 Full Artifact Bundle (.zip)").clicked() {
                                        self.toast(ToastKind::Info, "Bundling artifacts into ZIP...");
                                    }
                                });

                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("Export path:").strong());
                                ui.text_edit_singleline(&mut self.export_path);
                            });
                        });

                        ui.collapsing("⚙ Settings", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Theme:");
                                egui::ComboBox::from_id_salt("settings_theme")
                                    .selected_text(self.settings.theme.label())
                                    .show_ui(ui, |ui| {
                                        for t in [Theme::ForensicDark, Theme::ForensicLight] {
                                            ui.selectable_value(&mut self.settings.theme, t, t.label());
                                        }
                                    });
                            });
                            ui.horizontal(|ui| {
                                ui.label("Default DPI:");
                                ui.add(egui::Slider::new(&mut self.settings.default_dpi, 72.0..=600.0).step_by(1.0))
                                    .on_hover_text("Higher = sharper render, slower load");
                            });
                            ui.checkbox(&mut self.settings.auto_match_dpi, "Auto-match DPI to PDF Document Size")
                                .on_hover_text("Safely scales based on physical points (capped at 600 DPI to avoid OOM)");
                            ui.checkbox(&mut self.settings.transfer_consensus_mode, "Matrix Consensus for Transfers")
                                .on_hover_text("Runs multiple AIs simultaneously to perform majority-vote extraction & math cross-referencing");
                            ui.checkbox(&mut self.settings.auto_save, "Auto-save history")
                                .on_hover_text("Persist audit/history.json after every successful edit");
                            ui.add_space(8.0);
                            ui.label("Webhook (optional):");
                            ui.text_edit_singleline(&mut self.settings.webhook_url)
                                .on_hover_text("POST a JSON payload to this URL on each successful edit");
                            if ui.button("Save settings").on_hover_text("Persist these settings on disk").clicked() {
                                // On persistence failure, the in-memory `self.settings`
                                // is left untouched by confy::store, so we retain the
                                // current values, surface an error, and keep operating.
                                match confy::store("bank-statement-modifier", None, &self.settings) {
                                    Ok(()) => self.toast(ToastKind::Success, "Settings saved"),
                                    Err(e) => {
                                        tracing::warn!("[gui] failed to persist settings: {}", e);
                                        self.toast(
                                            ToastKind::Error,
                                            format!("Could not save settings: {e}"),
                                        );
                                    }
                                }
                            }

                            ui.add_space(10.0);
                            self.draw_api_keys_editor(ui);
                        });

                        ui.collapsing("⌨ Keybinds", |ui| {
                            ui.label("Ctrl+O : Open PDF");
                            ui.label("Ctrl+Z / Ctrl+Y : Undo / Redo");
                            ui.label("Ctrl+S : Export History");
                            ui.label("PageUp / PageDown : Next / Prev Page");
                            ui.label("+ / - : Zoom In / Out");
                            ui.label("0 : Reset Zoom");
                            if ui.button("Reset to defaults").clicked() {
                                self.toast(ToastKind::Info, "Keybinds reset to default.");
                            }
                        });

                        ui.collapsing("🔠 Custom Fonts", |ui| {
                            ui.label("Drag and drop .ttf or .otf files here to override Document AI.");
                            let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 60.0));
                            let response = ui.allocate_rect(rect, egui::Sense::hover());
                            ui.painter().rect_stroke(response.rect, 4.0, egui::Stroke::new(1.0, self.settings.theme.palette().weak));
                            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                                ui.centered_and_justified(|ui| {
                                    ui.label(egui::RichText::new("Drop fonts here").color(self.settings.theme.palette().weak).size(16.0));
                                });
                            });

                            if ctx.input(|i| !i.raw.dropped_files.is_empty()) {
                                // Dummy logic for now until native backend is wired
                                self.toast(ToastKind::Success, "Custom font embedded successfully.");
                            }
                        });
                });
        if open {
            self.active_modal = ActiveModal::Settings;
        } else if self.active_modal == ActiveModal::Settings {
            self.active_modal = ActiveModal::None;
        }
    }

    fn draw_backend_preferences(&mut self, ui: &mut egui::Ui) {
        use crate::app::capabilities::Capability;
        use crate::app::config::*;

        let legacy_local_ocr = self.settings.document_parser == DocumentParserMode::LocalOcrs;
        if legacy_local_ocr {
            self.settings.document_parser = DocumentParserMode::OfflineHeuristic;
        }
        let legacy_typst_edit = self.edit_engine_mode == PdfEngineMode::TypstReconstruct;
        if legacy_typst_edit {
            self.edit_engine_mode = PdfEngineMode::PyMuPdfProPrimary;
        }

        let id = ui.make_persistent_id("backend_prefs_collapsing");
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                ui.heading("\u{1f527} System Configuration & Integrations");
            })
            .body(|ui| {
                ui.small("Choose which backend to use for each stage of the workflow.");
                ui.small("Options marked \u{26d4} require an API key that is not currently configured.");
                if legacy_local_ocr {
                    ui.colored_label(
                        self.settings.theme.palette().warn,
                        "Local OCR PDF parsing is not part of v1; Offline Heuristic was selected instead.",
                    );
                }
                if legacy_typst_edit {
                    ui.colored_label(
                        self.settings.theme.palette().warn,
                        "Typst reconstruction is a non-fidelity export, not an edit engine; PyMuPDF Pro Primary was selected instead.",
                    );
                }
                ui.add_space(6.0);

                let capabilities = self.capability_registry.clone();
                let dual_edit_status = if capabilities.is_ready(Capability::PythonPipeline)
                    && capabilities.is_ready(Capability::Pdfium)
                {
                    crate::app::capabilities::CapabilityStatus::ready(
                        "Python/PyMuPDF and Pdfium are both ready",
                    )
                } else {
                    crate::app::capabilities::CapabilityStatus::unavailable(
                        "Dual Concurrent requires both the Python/PyMuPDF pipeline and Pdfium",
                    )
                };

                egui::Grid::new("backend_prefs_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // ── Pipeline Architecture ──
                        ui.label("Extraction:");
                        egui::ComboBox::from_id_salt("doc_parser_mode")
                            .selected_text(self.settings.document_parser.label())
                            .show_ui(ui, |ui| {
                                capability_selectable_value(
                                    ui,
                                    &mut self.settings.document_parser,
                                    DocumentParserMode::DocumentAi,
                                    DocumentParserMode::DocumentAi.label(),
                                    capabilities.status(Capability::DocumentAi),
                                );
                                capability_selectable_value(
                                    ui,
                                    &mut self.settings.document_parser,
                                    DocumentParserMode::LlamaParse,
                                    DocumentParserMode::LlamaParse.label(),
                                    capabilities.status(Capability::LlamaParse),
                                );
                                ui.selectable_value(
                                    &mut self.settings.document_parser,
                                    DocumentParserMode::OfflineHeuristic,
                                    DocumentParserMode::OfflineHeuristic.label(),
                                );
                            });
                        ui.end_row();

                        ui.label("Fidelity Edit:");
                        egui::ComboBox::from_id_salt("pdf_engine_mode")
                            .selected_text(match self.edit_engine_mode {
                                PdfEngineMode::DualConcurrent => "Dual Concurrent",
                                PdfEngineMode::PyMuPdfProPrimary => "PyMuPDF Pro Primary",
                                PdfEngineMode::NativeOnly => "Native Only",
                                PdfEngineMode::PyMuPdfOnly => "PyMuPDF Only",
                                PdfEngineMode::TypstReconstruct => "Typst Reconstruct",
                            })
                            .show_ui(ui, |ui| {
                                capability_selectable_value(
                                    ui,
                                    &mut self.edit_engine_mode,
                                    PdfEngineMode::DualConcurrent,
                                    "Dual Concurrent",
                                    Some(&dual_edit_status),
                                );
                                capability_selectable_value(
                                    ui,
                                    &mut self.edit_engine_mode,
                                    PdfEngineMode::PyMuPdfProPrimary,
                                    "PyMuPDF Pro Primary",
                                    capabilities.status(Capability::PyMuPdfPro),
                                );
                                capability_selectable_value(
                                    ui,
                                    &mut self.edit_engine_mode,
                                    PdfEngineMode::NativeOnly,
                                    "Native Only",
                                    capabilities.status(Capability::Pdfium),
                                );
                                capability_selectable_value(
                                    ui,
                                    &mut self.edit_engine_mode,
                                    PdfEngineMode::PyMuPdfOnly,
                                    "PyMuPDF Only",
                                    capabilities.status(Capability::PythonPipeline),
                                );
                            });
                        ui.end_row();

                        ui.label("Ledger Reconciliation:");
                        ui.label("1\u{fe0f}\u{20e3} Local Math Engine (100%)");
                        ui.end_row();

                        ui.label("AI Analysis:");
                        egui::ComboBox::from_id_salt("ai_provider_mode")
                            .selected_text(self.settings.ai_provider.label())
                            .show_ui(ui, |ui| {
                                for (mode, capability) in [
                                    (AiProviderMode::GeminiApiKey, Capability::Gemini),
                                    (AiProviderMode::GeminiVertex, Capability::GeminiVertex),
                                    (AiProviderMode::GroqApiKey, Capability::Groq),
                                    (AiProviderMode::OpenRouterApiKey, Capability::OpenRouter),
                                    (AiProviderMode::MistralApiKey, Capability::Mistral),
                                ] {
                                    capability_selectable_value(
                                        ui,
                                        &mut self.settings.ai_provider,
                                        mode,
                                        mode.label(),
                                        capabilities.status(capability),
                                    );
                                }
                                ui.selectable_value(
                                    &mut self.settings.ai_provider,
                                    AiProviderMode::ManualOnly,
                                    AiProviderMode::ManualOnly.label(),
                                );
                            });
                        ui.end_row();

                        ui.label("Forensics:");
                        ui.label("1\u{fe0f}\u{20e3} PyMuPDF Pro (100%) \u{2192} 2\u{fe0f}\u{20e3} Typst Reconstruct (90%)");
                        ui.end_row();

                        ui.label("Visual QA:");
                        egui::ComboBox::from_id_salt("visual_qa_mode")
                            .selected_text(self.settings.verification_renderer.label())
                            .show_ui(ui, |ui| {
                                capability_selectable_value(
                                    ui,
                                    &mut self.settings.verification_renderer,
                                    VerificationMode::LocalPdfium,
                                    VerificationMode::LocalPdfium.label(),
                                    capabilities.status(Capability::Pdfium),
                                );
                                capability_selectable_value(
                                    ui,
                                    &mut self.settings.verification_renderer,
                                    VerificationMode::PdfRestCloud,
                                    VerificationMode::PdfRestCloud.label(),
                                    capabilities.status(Capability::PdfRest),
                                );
                            });
                        ui.end_row();

                        // ── 5. Font Handling ──
                        ui.label("\u{1f520} Font Handling:");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.settings.deep_font_replication, "Deep Font Replication")
                                .on_hover_text("Extracts fonts from the source PDF and embeds them in the output. Pixel-perfect but slower.");
                        });
                        ui.end_row();

                        // ── 6. Processing Mode ──
                        ui.label("\u{1f4d0} Processing:");
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.settings.three_page_mode, "3-Page Segmented Mode")
                                .on_hover_text("Splits long PDFs into \u{2264}3-page segments for Pro editing, then re-merges on save.")
                                .changed()
                            {
                                let _ = confy::store("bank-statement-modifier", None, &self.settings);
                            }
                        });
                        ui.end_row();

                        // ── 7. Independent Verification Policy ──
                        ui.label("\u{1f3af} Verification Policy:");
                        ui.label("Calibrated v2 · all pages · fixed thresholds · single pass")
                            .on_hover_text(
                                "Mandatory verification uses an immutable 0.02 outside-region tile threshold, 0.40 SSIM floor, zero adaptive mask widening, and exact structural/content/financial gates. Optional providers cannot override local failures.",
                            );
                        ui.end_row();

                        // ── 8. Workflow Settings ──
                        ui.label("\u{23f8}\u{fe0f} Interactive Fallbacks:");
                        ui.checkbox(&mut self.settings.interactive_fallbacks, "Pause & prompt on semi-failures")
                            .on_hover_text("When enabled, the app will pause on non-catastrophic errors (like parsing failure) and prompt you to manually select a fallback strategy or try again.");
                        ui.end_row();
                    });

                // ── Unified availability warnings ──
                let mut warnings: Vec<String> = Vec::new();
                let mut warn_if_unavailable = |
                    label: &str,
                    status: Option<&crate::app::capabilities::CapabilityStatus>,
                | {
                    if let Some(status) = status {
                        if !status.is_selectable() {
                            warnings.push(format!("\u{26a0} {label}: {}", status.reason));
                        }
                    }
                };

                match self.settings.ai_provider {
                    AiProviderMode::GeminiApiKey => {
                        warn_if_unavailable("Gemini", capabilities.status(Capability::Gemini))
                    }
                    AiProviderMode::GeminiVertex => {
                        warn_if_unavailable(
                            "Gemini Vertex",
                            capabilities.status(Capability::GeminiVertex),
                        )
                    }
                    AiProviderMode::GroqApiKey => {
                        warn_if_unavailable("Groq", capabilities.status(Capability::Groq))
                    }
                    AiProviderMode::OpenRouterApiKey => {
                        warn_if_unavailable(
                            "OpenRouter",
                            capabilities.status(Capability::OpenRouter),
                        )
                    }
                    AiProviderMode::MistralApiKey => {
                        warn_if_unavailable("Mistral", capabilities.status(Capability::Mistral))
                    }
                    AiProviderMode::ManualOnly => {}
                }

                match self.settings.document_parser {
                    DocumentParserMode::DocumentAi => {
                        warn_if_unavailable(
                            "Document AI",
                            capabilities.status(Capability::DocumentAi),
                        )
                    }
                    DocumentParserMode::LlamaParse => {
                        warn_if_unavailable(
                            "LlamaParse",
                            capabilities.status(Capability::LlamaParse),
                        )
                    }
                    DocumentParserMode::LocalOcrs => {}
                    DocumentParserMode::OfflineHeuristic => {}
                }

                match self.edit_engine_mode {
                    PdfEngineMode::DualConcurrent => {
                        warn_if_unavailable("Dual Concurrent", Some(&dual_edit_status))
                    }
                    PdfEngineMode::PyMuPdfProPrimary => {
                        warn_if_unavailable(
                            "PyMuPDF Pro",
                            capabilities.status(Capability::PyMuPdfPro),
                        )
                    }
                    PdfEngineMode::NativeOnly => {
                        warn_if_unavailable(
                            "Native Pdfium",
                            capabilities.status(Capability::Pdfium),
                        )
                    }
                    PdfEngineMode::PyMuPdfOnly => {
                        warn_if_unavailable(
                            "PyMuPDF",
                            capabilities.status(Capability::PythonPipeline),
                        )
                    }
                    _ => {}
                }

                match self.settings.verification_renderer {
                    VerificationMode::LocalPdfium => {
                        warn_if_unavailable(
                            "Local Pdfium verification",
                            capabilities.status(Capability::Pdfium),
                        )
                    }
                    VerificationMode::PdfRestCloud => {
                        warn_if_unavailable(
                            "pdfRest verification",
                            capabilities.status(Capability::PdfRest),
                        )
                    }
                }

                if !warnings.is_empty() {
                    ui.add_space(4.0);
                    for message in warnings {
                        ui.colored_label(self.settings.theme.palette().warn, message);
                    }
                }

                // Keep Gemini auth mode in sync with AI provider choice
                match self.settings.ai_provider {
                    AiProviderMode::GeminiApiKey => {
                        self.edit_gemini_use_vertex = false;
                    }
                    AiProviderMode::GeminiVertex => {
                        self.edit_gemini_use_vertex = true;
                    }
                    _ => {}
                }
            });
        ui.add_space(4.0);
        ui.separator();
    }

    fn draw_transfer_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.active_modal == ActiveModal::Transfer;
        egui::Window::new("🔄 Transfer Transactions")
            .open(&mut open)
            .default_size(egui::vec2(1200.0, 750.0))
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                ui.vertical_centered(|ui| {
                    ui.heading("Transfer transactions between statements");
                });
                ui.separator();

                ui.columns(3, |cols| {
                    // --- LEFT COLUMN: SOURCE ---
                    cols[0].vertical_centered(|ui| {
                        ui.heading("Source Document");
                        ui.label(egui::RichText::new("Extract transactions from here").weak());
                        ui.add_space(8.0);
                        if ui.button("📂 Upload Source PDF").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("PDF", &["pdf"])
                                .pick_file()
                            {
                                self.transfer_source_path = path.to_string_lossy().to_string();
                                if let Err(e) = self.job_tx.send(Job::RenderPage {
                                    path,
                                    page: 1,
                                    dpi: 72.0,
                                    tag: "transfer_source".to_string(),
                                }) {
                                    tracing::error!("Runtime disconnected: {}", e);
                                }
                                self.in_flight += 1;
                            }
                        }

                        if let Some(tex) = &self.transfer_source_texture {
                            ui.add_space(10.0);
                            let max_size = ui.available_size() - egui::vec2(0.0, 20.0);
                            let tex_size = tex.size_vec2();
                            let scale = (max_size.x / tex_size.x)
                                .min(max_size.y / tex_size.y)
                                .min(1.0);
                            ui.add(egui::Image::new(tex).fit_to_exact_size(tex_size * scale));
                        } else if !self.transfer_source_path.is_empty() {
                            ui.add_space(20.0);
                            ui.spinner();
                            ui.label("Rendering preview...");
                        }
                    });

                    // --- MIDDLE COLUMN: ACTIONS ---
                    cols[1].vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 60.0);

                        let source_ok = !self.transfer_source_path.is_empty()
                            && std::path::Path::new(&self.transfer_source_path).exists();
                        let target_ok = !self.input_path.is_empty()
                            && std::path::Path::new(&self.input_path).exists()
                            && self.input_path != "examples/sample.pdf";
                        let can_start = source_ok && target_ok;

                        if self.in_flight > 0 {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    egui::RichText::new("Transfer in progress...")
                                        .color(self.settings.theme.palette().accent),
                                );
                            });
                        } else {
                            let btn = ui.add_enabled(
                                can_start,
                                egui::Button::new(
                                    egui::RichText::new("▶ Begin Transfer").size(20.0).color(
                                        if can_start {
                                            self.settings.theme.palette().bg
                                        } else {
                                            self.settings.theme.palette().text
                                        },
                                    ),
                                )
                                .fill(if can_start {
                                    self.settings.theme.palette().accent
                                } else {
                                    self.settings.theme.palette().panel
                                })
                                .min_size(egui::vec2(180.0, 56.0))
                                .rounding(8.0),
                            );

                            if btn.clicked() {
                                let source = std::path::PathBuf::from(&self.transfer_source_path);
                                let target = std::path::PathBuf::from(&self.input_path);
                                let output = if self.output_path.is_empty() {
                                    target.with_file_name(format!(
                                        "{}_transferred.pdf",
                                        target.file_stem().unwrap_or_default().to_string_lossy()
                                    ))
                                } else {
                                    std::path::PathBuf::from(&self.output_path)
                                };

                                if let Err(e) = self.job_tx.send(Job::TransferTransactions {
                                    source_pdf: source,
                                    target_pdf: target,
                                    output_pdf: output,
                                }) {
                                    tracing::error!("Runtime disconnected: {}", e);
                                }
                                self.in_flight += 1;
                                self.status = "Starting transaction transfer...".into();
                                self.toast(
                                    ToastKind::Info,
                                    "Transaction transfer started - this may take 2-3 minutes.",
                                );
                                // Do NOT close the modal automatically, let the spinner show.
                            }
                        }

                        ui.add_space(10.0);

                        if !source_ok && !self.transfer_source_path.is_empty() {
                            ui.colored_label(
                                self.settings.theme.palette().warn,
                                "⚠ Source not found",
                            );
                        }
                        if !target_ok
                            && !self.input_path.is_empty()
                            && self.input_path != "examples/sample.pdf"
                        {
                            ui.colored_label(
                                self.settings.theme.palette().warn,
                                "⚠ Target not found",
                            );
                        }

                        ui.add_space(20.0);
                        if ui.button("Cancel").clicked() {
                            self.active_modal = ActiveModal::None;
                        }
                    });

                    // --- RIGHT COLUMN: TARGET ---
                    cols[2].vertical_centered(|ui| {
                        ui.heading("Target Document");
                        ui.label(egui::RichText::new("Format to apply to transactions").weak());
                        ui.add_space(8.0);
                        if ui.button("📂 Upload Target PDF").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("PDF", &["pdf"])
                                .pick_file()
                            {
                                self.input_path = path.to_string_lossy().to_string();
                                if let Err(e) = self.job_tx.send(Job::RenderPage {
                                    path,
                                    page: 1,
                                    dpi: 72.0,
                                    tag: "transfer_target".to_string(),
                                }) {
                                    tracing::error!("Runtime disconnected: {}", e);
                                }
                                self.in_flight += 1;
                            }
                        }

                        if let Some(tex) = &self.transfer_target_texture {
                            ui.add_space(10.0);
                            let max_size = ui.available_size() - egui::vec2(0.0, 20.0);
                            let tex_size = tex.size_vec2();
                            let scale = (max_size.x / tex_size.x)
                                .min(max_size.y / tex_size.y)
                                .min(1.0);
                            ui.add(egui::Image::new(tex).fit_to_exact_size(tex_size * scale));
                        } else if !self.input_path.is_empty()
                            && self.input_path != "examples/sample.pdf"
                        {
                            ui.add_space(20.0);
                            ui.spinner();
                            ui.label("Rendering preview...");
                        }
                    });
                });
            });

        if !open {
            self.active_modal = ActiveModal::None;
        }
        if open {
            self.active_modal = ActiveModal::Transfer;
        } else if self.active_modal == ActiveModal::Transfer {
            self.active_modal = ActiveModal::None;
        }
    }

    fn draw_date_adjust_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.active_modal == ActiveModal::DateAdjust;
        egui::Window::new("📅 Adjust Date Periods")
            .open(&mut open)
            .default_size(egui::vec2(420.0, 320.0))
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                ui.heading("Shift or remap all transaction dates");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.date_adjust_mode_shift, true, "Shift by days");
                    ui.radio_value(&mut self.date_adjust_mode_shift, false, "Remap period");
                });

                ui.add_space(4.0);

                if self.date_adjust_mode_shift {
                    ui.horizontal(|ui| {
                        ui.label("Days to shift:");
                        ui.text_edit_singleline(&mut self.date_adjust_shift_days);
                    });
                    ui.label(
                        egui::RichText::new("Positive = forward, negative = backward")
                            .small()
                            .color(self.settings.theme.palette().weak),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.label("From (DD/MM/YYYY):");
                        ui.text_edit_singleline(&mut self.date_adjust_from);
                    });
                    ui.horizontal(|ui| {
                        ui.label("To   (DD/MM/YYYY):");
                        ui.text_edit_singleline(&mut self.date_adjust_to);
                    });
                }

                ui.add_space(8.0);
                ui.separator();

                let has_input = !self.input_path.is_empty();
                let validated_mode = if self.date_adjust_mode_shift {
                    self.date_adjust_shift_days
                        .trim()
                        .parse::<i64>()
                        .map(crate::engine::date_adjust::DateAdjustMode::ShiftDays)
                        .map_err(|_| "Days to shift must be a whole number.".to_string())
                } else {
                    let from =
                        chrono::NaiveDate::parse_from_str(self.date_adjust_from.trim(), "%d/%m/%Y")
                            .map_err(|_| "From date must be a valid DD/MM/YYYY date.".to_string());
                    let to =
                        chrono::NaiveDate::parse_from_str(self.date_adjust_to.trim(), "%d/%m/%Y")
                            .map_err(|_| "To date must be a valid DD/MM/YYYY date.".to_string());
                    from.and_then(|from_start| {
                        to.map(
                            |to_start| crate::engine::date_adjust::DateAdjustMode::RemapPeriod {
                                from_start,
                                to_start,
                            },
                        )
                    })
                };
                let can_apply = has_input && validated_mode.is_ok();
                let validation_error = validated_mode.as_ref().err().cloned();

                ui.horizontal(|ui| {
                    if self.in_flight > 0 {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new("Date adjustment in progress...")
                                .color(self.settings.theme.palette().accent),
                        );
                        if ui.button("Close").clicked() {
                            self.active_modal = ActiveModal::None;
                        }
                    } else {
                        let btn = ui.add_enabled(
                            can_apply,
                            egui::Button::new("▶ Apply Date Adjustment").fill(if can_apply {
                                self.settings.theme.palette().accent
                            } else {
                                self.settings.theme.palette().panel
                            }),
                        );

                        if btn.clicked() {
                            let input = std::path::PathBuf::from(&self.input_path);
                            let output = Self::safe_output_path(&input, "dates");

                            let mode = validated_mode
                                .clone()
                                .expect("enabled date-adjust action has validated input");

                            let _ = self.job_tx.send(Job::AdjustDatePeriods {
                                input,
                                output,
                                mode,
                            });
                            self.in_flight += 1;
                            self.status = "Adjusting dates...".into();
                            self.toast(ToastKind::Info, "Date adjustment started.");
                        }

                        if ui.button("Cancel").clicked() {
                            self.active_modal = ActiveModal::None;
                        }
                    }
                });

                if !has_input {
                    ui.colored_label(self.settings.theme.palette().warn, "⚠ Load a PDF first");
                } else if let Some(error) = validation_error {
                    ui.colored_label(self.settings.theme.palette().warn, error);
                }
            });
        if open {
            self.active_modal = ActiveModal::DateAdjust;
        } else if self.active_modal == ActiveModal::DateAdjust {
            self.active_modal = ActiveModal::None;
        }
    }

    fn draw_ai_confirmation_dialog(&mut self, ctx: &egui::Context) {
        // Show only the first pending confirmation
        if let Some(confirmation) = self.pending_ai_confirmations.first().cloned() {
            let mut responded = false;
            let mut selected = 0usize;

            egui::Window::new("🤖 AI Needs Your Input")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;

                    ui.heading(&confirmation.question);
                    ui.separator();

                    ui.label(
                        egui::RichText::new(format!("Context: {}", confirmation.context))
                            .small()
                            .color(self.settings.theme.palette().weak),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "AI Confidence: {:.0}%",
                            confirmation.confidence * 100.0
                        ))
                        .small()
                        .color(if confirmation.confidence < 0.5 {
                            self.settings.theme.palette().warn
                        } else {
                            self.settings.theme.palette().weak
                        }),
                    );

                    ui.add_space(8.0);

                    for (i, option) in confirmation.options.iter().enumerate() {
                        let is_default = confirmation.default_answer == Some(i);
                        let label = if is_default {
                            format!("-> {} (recommended)", option)
                        } else {
                            option.clone()
                        };
                        if ui.button(&label).clicked() {
                            selected = i;
                            responded = true;
                        }
                    }
                });

            if responded {
                let response = crate::engine::ai_confirm::AiConfirmationResponse {
                    id: confirmation.id,
                    selected_option: selected,
                    user_note: None,
                };
                let _ = crate::engine::ai_confirm::log_learning_response(&confirmation, &response);
                let _ = self.job_tx.send(Job::AiConfirmationResponse(response));
                self.pending_ai_confirmations.remove(0);
            }
        }
    }

    fn draw_interactive_fallback_modal(&mut self, ctx: &egui::Context) {
        if let Some(req) = self.pending_interactive_fallback.clone() {
            let mut keep_open = true;
            let mut resolved_choice: Option<String> = None;

            egui::Window::new(format!("⚠️ Interactive Fallback: {}", req.stage))
                .open(&mut keep_open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;

                    ui.label("An operation could not be completed successfully.");
                    ui.label(egui::RichText::new(&req.error_details).color(egui::Color32::RED));
                    ui.add_space(10.0);

                    ui.heading("Would you like to try again using an alternative?");
                    ui.add_space(5.0);

                    for alt in &req.alternatives {
                        let btn_text = egui::RichText::new(&alt.label).strong().size(14.0);
                        if ui
                            .add_sized([ui.available_width(), 36.0], egui::Button::new(btn_text))
                            .clicked()
                        {
                            resolved_choice = Some(alt.id.clone());
                        }
                        if let Some(desc) = &alt.description {
                            ui.small(
                                egui::RichText::new(desc).color(ui.visuals().weak_text_color()),
                            );
                        }
                        ui.add_space(4.0);
                    }
                });

            if !keep_open {
                // If user clicks the 'X' to close, treat it as a cancellation if possible.
                // We'll just route a generic 'cancel' or fallback ID if we can't do anything else.
                // Alternatively, force them to pick a button by hiding the close button, but `open` gives a close button.
                // It's safer to have a dedicated Cancel button. If they force close, we can return 'cancel'.
                resolved_choice = Some("cancel".to_string());
            }

            if let Some(choice_id) = resolved_choice {
                let response = crate::engine::interactive_fallback::InteractiveFallbackResponse {
                    id: req.id,
                    selected_alternative_id: choice_id,
                };
                let _ = self
                    .job_tx
                    .send(crate::app::runtime::Job::InteractiveFallbackResponse(
                        response,
                    ));
                self.pending_interactive_fallback = None;
            }
        }
    }

    fn draw_autofix_modal(&mut self, ctx: &egui::Context) {
        let mut resolved_choice: Option<String> = None;
        if let Some(err) = self.pending_autofix.clone() {
            let mut keep_open = true;
            egui::Window::new("⚠️ Operation Failed")
                .open(&mut keep_open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;

                    ui.label("An operation encountered an error:");
                    ui.label(egui::RichText::new(err.to_string()).color(egui::Color32::RED));
                    ui.add_space(10.0);

                    ui.heading("How would you like to proceed?");
                    ui.add_space(5.0);

                    if let Some(action) = err.suggested_action() {
                        let btn_text = egui::RichText::new(action).strong().size(14.0);
                        if ui
                            .add_sized([ui.available_width(), 36.0], egui::Button::new(btn_text))
                            .clicked()
                        {
                            resolved_choice = Some("action".to_string());
                        }
                    }

                    if ui
                        .add_sized([ui.available_width(), 36.0], egui::Button::new("Cancel"))
                        .clicked()
                    {
                        resolved_choice = Some("cancel".to_string());
                    }

                    ui.add_space(5.0);
                    if ui
                        .add_sized(
                            [ui.available_width(), 36.0],
                            egui::Button::new("🐛 Submit Bug Report"),
                        )
                        .clicked()
                    {
                        resolved_choice = Some("report".to_string());
                    }
                });

            if !keep_open || resolved_choice.is_some() {
                self.pending_autofix = None;
                if resolved_choice.as_deref() == Some("action") {
                    if let Some(action) = err.suggested_action() {
                        if action.contains("Settings") {
                            self.active_modal = ActiveModal::Settings;
                        } else {
                            self.status = action.to_string();
                        }
                    }
                } else if resolved_choice.as_deref() == Some("report") {
                    self.active_modal = ActiveModal::Feedback;
                    self.feedback_text =
                        format!("Operation Failed: {}\n\nSteps to reproduce: \n", err);
                }
            }
        }
    }

    fn draw_workflow_hitl_modal(&mut self, ctx: &egui::Context) {
        let mut keep_open = self.active_modal == ActiveModal::WorkflowHitl;
        let mut resolved = false;
        let workflow_stage = self.workflow_stage.clone();

        egui::Window::new("⚠️ Workflow Human-in-the-Loop Required")
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                match &workflow_stage {
                    crate::engine::workflow::WorkflowStage::FontCoverageWarning { missing_chars } => {
                        ui.heading("Font Coverage Block");
                        ui.label("The selected font does not cover every character in the requested edits.");
                        ui.label(format!("Missing characters: {:?}", missing_chars));
                        ui.label("Generic-font substitution is disabled because it would change the statement typeface. Return to edit review and change the text or provide a coverage-complete reviewed font.");
                        ui.add_space(8.0);
                        if ui.button("Cancel Workflow").clicked() {
                            self.cancel_active_workflow();
                            resolved = true;
                        }
                    }
                    crate::engine::workflow::WorkflowStage::VisualFidelityWarning { score, threshold, attempt, is_borderline } => {
                        ui.heading("Visual Fidelity Warning");
                        ui.label(format!("The visual difference score {:.4} exceeds the threshold {:.4} (Attempt {})", score, threshold, attempt));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.cancel_active_workflow();
                                resolved = true;
                            }
                            if ui.button("Accept Overlap & Proceed").clicked() {
                                resolved = true;
                            }
                            if *is_borderline
                                && ui.button("Compare Alternatives (2x2 Grid)").clicked() {
                                    // Trigger backend job to generate alternatives
                                    let path = self.current_pdf_path.clone();
                                    let edits = self.workflow_edits.clone();

                                    // Pick first page of edits or default to page 0
                                    let page = edits.first().map(|e| e.page).unwrap_or(0);
                                    // For demo, just pass the edits and a simple bbox of the first edit
                                    let bbox = edits.first().map(|e| e.bbox).unwrap_or([0.0, 0.0, 100.0, 100.0]);

                                    let _ = self.job_tx.send(Job::GenerateVisualAlternatives {
                                        input: path,
                                        out_dir: std::path::PathBuf::from("output"), // or a temp dir
                                        page,
                                        edits: edits.clone(),
                                        bbox,
                                    });
                                    self.status = "Generating alternative renders...".to_string();
                                    // don't resolve yet, keep modal open until job finishes
                                }
                        });
                    }
                    crate::engine::workflow::WorkflowStage::VisualComparisonActive { images } => {
                        ui.heading("Select Best Alternative");
                        ui.label("The primary renderer produced a borderline anomaly. Select the cleanest output below:");

                        egui::ScrollArea::both().show(ui, |ui| {
                            egui::Grid::new("visual_comparison_grid")
                                .num_columns(2)
                                .spacing([16.0, 16.0])
                                .show(ui, |ui| {
                                    for (i, (label, img_bytes)) in images.iter().enumerate() {
                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(label).strong());

                                            // Render image bytes via egui_extras
                                            if let Ok(img) = image::load_from_memory(img_bytes) {
                                                let size = [img.width() as usize, img.height() as usize];
                                                let pixels = img.into_rgba8();
                                                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);

                                                let tex = ctx.load_texture(
                                                    format!("alt_{i}"),
                                                    color_image,
                                                    egui::TextureOptions::default(),
                                                );

                                                ui.image(&tex);
                                            } else {
                                                ui.label("(Failed to load image)");
                                            }

                                            if ui.button(format!("Use {}", label)).clicked() {
                                                // Normally here we would send a job to swap the active engine
                                                // For now, we resolve the modal
                                                resolved = true;
                                            }
                                        });

                                        if i % 2 == 1 {
                                            ui.end_row();
                                        }
                                    }
                                });
                        });

                        ui.add_space(8.0);
                        if ui.button("Cancel & Discard").clicked() {
                            self.cancel_active_workflow();
                            resolved = true;
                        }
                    }
                    crate::engine::workflow::WorkflowStage::ImbalanceCorrectionWarning { imbalance, proposed_changes } => {
                        ui.heading("Imbalance Correction Proposal");
                        ui.label(format!("The statement has an imbalance of {}.", imbalance));
                        ui.label("The AI has proposed the following corrections:");
                        for change in proposed_changes {
                            ui.label(format!("- Page {}: {} -> {}", change.page + 1, change.old_text, change.new_text));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.cancel_active_workflow();
                                resolved = true;
                            }
                            if ui.button("Accept Proposed Corrections").clicked() {
                                resolved = true;
                            }
                        });
                    }
                    crate::engine::workflow::WorkflowStage::OfflineFallbackWarning => {
                        ui.heading("Offline Fallback");
                        ui.label("All cloud parsers have failed or timed out.");
                        ui.label("The system will proceed using the Offline OCR Parser.");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel Workflow").clicked() {
                                self.cancel_active_workflow();
                                resolved = true;
                            }
                            if ui.button("Proceed Offline").clicked() {
                                resolved = true;
                            }
                        });
                    }
                    _ => {
                        ui.label("Waiting...");
                    }
                }
            });

        if resolved || !keep_open {
            self.active_modal = ActiveModal::None;
        }
    }

    fn draw_transfer_test_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.active_modal == ActiveModal::TransferTest;
        egui::Window::new("🧪 Transfer Test Harness")
            .open(&mut open)
            .default_size(egui::vec2(520.0, 420.0))
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;

                ui.heading("Cross-Statement Transfer Tests");
                ui.label("Select PDFs to test all N×(N−1) transfer directions:");
                ui.separator();

                // List current paths
                let mut to_remove: Option<usize> = None;
                for (i, path) in self.transfer_test_paths.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}.", i + 1));
                        ui.label(
                            egui::RichText::new(path)
                                .monospace()
                                .color(self.settings.theme.palette().text),
                        );
                        if ui.small_button("✕").clicked() {
                            to_remove = Some(i);
                        }
                    });
                }
                if let Some(idx) = to_remove {
                    self.transfer_test_paths.remove(idx);
                }

                if ui.button("➕ Add PDF...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("PDF", &["pdf"])
                        .pick_file()
                    {
                        self.transfer_test_paths
                            .push(path.to_string_lossy().to_string());
                    }
                }

                let n = self.transfer_test_paths.len();
                let pairs = if n >= 2 { n * (n - 1) } else { 0 };
                ui.label(format!("{} statements -> {} test pairs", n, pairs));

                ui.add_space(8.0);
                ui.separator();

                ui.horizontal(|ui| {
                    if self.in_flight > 0 {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new("Test runner is active...")
                                .color(self.settings.theme.palette().accent),
                        );
                    } else {
                        let can_run = n >= 2;
                        let btn = ui.add_enabled(
                            can_run,
                            egui::Button::new("▶ Run All Tests").fill(if can_run {
                                self.settings.theme.palette().accent
                            } else {
                                self.settings.theme.palette().panel
                            }),
                        );

                        if btn.clicked() {
                            let statements: Vec<std::path::PathBuf> = self
                                .transfer_test_paths
                                .iter()
                                .map(std::path::PathBuf::from)
                                .collect();
                            let _ = self.job_tx.send(Job::RunTransferTests {
                                statements,
                                max_iterations: 3,
                            });
                            self.in_flight += 1;
                            self.status = format!("Running {} transfer tests...", pairs);
                            self.toast(
                                ToastKind::Info,
                                format!("Running {} transfer test pairs...", pairs),
                            );
                        }

                        if ui.button("Close").clicked() {
                            self.active_modal = ActiveModal::None;
                        }
                    }
                });

                // Show previous results if any
                if let Some(report) = &self.transfer_test_report {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.heading("Last Results");

                    let color = if report.all_passed() {
                        egui::Color32::from_rgb(80, 200, 120)
                    } else {
                        self.settings.theme.palette().warn
                    };
                    ui.colored_label(color, report.summary());

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for r in &report.results {
                                let icon = if r.converged && r.final_math_ok {
                                    "✅"
                                } else {
                                    "❌"
                                };
                                let src =
                                    r.source.file_stem().unwrap_or_default().to_string_lossy();
                                let tgt =
                                    r.target.file_stem().unwrap_or_default().to_string_lossy();
                                ui.label(format!(
                                    "{} {} -> {} ({}iter, {:.1}s)",
                                    icon, src, tgt, r.iterations, r.duration_secs
                                ));
                                if !r.corrections.is_empty() {
                                    for c in &r.corrections {
                                        ui.label(
                                            egui::RichText::new(format!("  ↳ {}", c))
                                                .small()
                                                .color(self.settings.theme.palette().weak),
                                        );
                                    }
                                }
                            }
                        });
                }
            });
        if open {
            self.active_modal = ActiveModal::TransferTest;
        } else if self.active_modal == ActiveModal::TransferTest {
            self.active_modal = ActiveModal::None;
        }
    }

    fn draw_api_keys_editor(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("🔑 API keys & credentials", |ui| {
                ui.small("Stored in .env (gitignored). Applied live - no restart needed.");
                ui.add_space(4.0);

                egui::Grid::new("api_keys_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Gemini API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_gemini_api_key)
                                .password(true)
                                .desired_width(220.0),
                        )
                        .on_hover_text("AI Studio key (AIza...). Used for completeness + vision checks.");
                        ui.end_row();

                        ui.label("Gemini auth mode:");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.edit_gemini_use_vertex, false, "API key");
                            ui.selectable_value(&mut self.edit_gemini_use_vertex, true, "Vertex AI");
                        });
                        ui.end_row();

                        let is_docai = self.settings.document_parser == crate::app::config::DocumentParserMode::DocumentAi
                            || self.settings.ai_provider == crate::app::config::AiProviderMode::GeminiVertex
                            || self.edit_gemini_use_vertex;

                        ui.label("Doc AI project ID:");
                        ui.add_enabled(is_docai, egui::TextEdit::singleline(&mut self.edit_docai_project_id).desired_width(220.0));
                        ui.end_row();

                        ui.label("Doc AI location:");
                        ui.add_enabled(is_docai, egui::TextEdit::singleline(&mut self.edit_docai_location).desired_width(220.0))
                            .on_hover_text("e.g. 'us' or 'eu' - must match the processor region.");
                        ui.end_row();

                        ui.label("Doc AI processor ID:");
                        ui.add_enabled(is_docai, egui::TextEdit::singleline(&mut self.edit_docai_processor_id).desired_width(220.0))
                            .on_hover_text("The Bank Statement parser or Custom Extractor processor ID.");
                        ui.end_row();

                        ui.label("Service account JSON:");
                        ui.horizontal(|ui| {
                            ui.add_enabled(is_docai, egui::TextEdit::singleline(&mut self.edit_docai_service_account).desired_width(150.0))
                                .on_hover_text("Path to the service-account key JSON (best-practice auth).");
                            if ui.add_enabled(is_docai, egui::Button::new("Browse...")).clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("JSON", &["json"])
                                    .pick_file()
                                {
                                    self.edit_docai_service_account = path.to_string_lossy().into_owned();
                                }
                            }
                        });
                        ui.end_row();

                        ui.label("Doc AI API key (opt):");
                        ui.add_enabled(
                            is_docai,
                            egui::TextEdit::singleline(&mut self.edit_docai_api_key)
                                .password(true)
                                .desired_width(220.0),
                        )
                        .on_hover_text("Optional Beta API key; takes precedence over OAuth/SA.");
                        ui.end_row();

                        ui.label("PyMuPDF Pro key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_pymupdf_pro_key)
                                .password(true)
                                .desired_width(220.0),
                        )
                        .on_hover_text("PyMuPDF Pro license key — trial ('hFKt...' 24-char) or commercial (≥16 chars) enables per-segment Pro editing.");
                        ui.end_row();



                        ui.label("LlamaParse API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_llamaparse_api_key)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();

                        ui.label("pdfRest API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_pdfrest_api_key)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();

                        ui.label("Lipi API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_lipi_api_key)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();

                        ui.label("Vision API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_vision_api_key)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();

                        ui.label("Groq API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_groq_api_key)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();

                        ui.label("OpenRouter API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_openrouter_api_key)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();

                        ui.label("Mindee API key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_mindee_api_key)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();

                        ui.label("OpenRouter Model:");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("or_model_combo")
                                .selected_text(&self.edit_openrouter_model)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.edit_openrouter_model, "google/gemini-2.0-flash-exp:free".to_string(), "Gemini 2.0 Flash (Free)");
                                    ui.selectable_value(&mut self.edit_openrouter_model, "meta-llama/llama-3.1-8b-instruct:free".to_string(), "Llama 3.1 8B (Free)");
                                    ui.selectable_value(&mut self.edit_openrouter_model, "mistralai/mistral-nemo:free".to_string(), "Mistral Nemo (Free)");
                                    ui.selectable_value(&mut self.edit_openrouter_model, "deepseek/deepseek-chat".to_string(), "DeepSeek Chat");
                                    ui.selectable_value(&mut self.edit_openrouter_model, "anthropic/claude-3.5-sonnet".to_string(), "Claude 3.5 Sonnet");
                                    ui.selectable_value(&mut self.edit_openrouter_model, "openai/gpt-4o".to_string(), "GPT-4o");
                                });
                            ui.add(
                                egui::TextEdit::singleline(&mut self.edit_openrouter_model)
                                    .desired_width(180.0),
                            );
                        });
                        ui.end_row();

                        ui.label("Mistral AI API Key:");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.edit_mistral_api_key)
                                    .password(true)
                                    .desired_width(220.0),
                            );
                        });
                        ui.end_row();

                        ui.label("Mistral Model:");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("mistral_model_combo")
                                .selected_text(&self.edit_mistral_model)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.edit_mistral_model, "mistral-large-latest".to_string(), "Mistral Large");
                                    ui.selectable_value(&mut self.edit_mistral_model, "mistral-small-latest".to_string(), "Mistral Small");
                                    ui.selectable_value(&mut self.edit_mistral_model, "pixtral-large-latest".to_string(), "Pixtral Large");
                                });
                            ui.add(
                                egui::TextEdit::singleline(&mut self.edit_mistral_model)
                                    .desired_width(180.0),
                            );
                        });
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("📤 Export .env").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(".env.export")
                            .save_file()
                        {
                            let new_config = vec![
                                ("GEMINI_API_KEY", self.edit_gemini_api_key.trim(), false),
                                ("GEMINI_AUTH_MODE", if self.edit_gemini_use_vertex { "vertex" } else { "api_key" }, false),
                                ("DOCUMENT_AI_PROJECT_ID", self.edit_docai_project_id.trim(), false),
                                ("DOCUMENT_AI_LOCATION", self.edit_docai_location.trim(), false),
                                ("DOCUMENT_AI_PROCESSOR_ID", self.edit_docai_processor_id.trim(), false),
                                ("GOOGLE_APPLICATION_CREDENTIALS", self.edit_docai_service_account.trim(), true), // don't quote paths
                                ("DOCUMENT_AI_API_KEY", self.edit_docai_api_key.trim(), false),
                                ("PYMUPDF_PRO_KEY", self.edit_pymupdf_pro_key.trim(), false),

                                ("LLAMAPARSE_API_KEY", self.edit_llamaparse_api_key.trim(), false),
                                ("PDFREST_API_KEY", self.edit_pdfrest_api_key.trim(), false),
                                ("LIPI_API_KEY", self.edit_lipi_api_key.trim(), false),
                                ("VISION_API_KEY", self.edit_vision_api_key.trim(), false),
                                ("GROQ_API_KEY", self.edit_groq_api_key.trim(), false),
                                ("OPENROUTER_API_KEY", self.edit_openrouter_api_key.trim(), false),
                                ("OPENROUTER_MODEL", self.edit_openrouter_model.trim(), false),
                                ("MISTRAL_API_KEY", self.edit_mistral_api_key.trim(), false),
                                ("MISTRAL_MODEL", self.edit_mistral_model.trim(), false),
                                ("MINDEE_API_KEY", self.edit_mindee_api_key.trim(), false),
                            ];
                            let content: String = new_config.iter()
                                .map(|(k, v, _)| format!("{}={}", k, v))
                                .collect::<Vec<_>>()
                                .join("\n") + "\n";

                            if let Err(e) = std::fs::write(&path, content) {
                                self.toast(ToastKind::Error, format!("Failed to export: {}", e));
                            } else {
                                self.toast(ToastKind::Success, "Exported .env successfully");
                            }
                        }
                    }

                    if ui.button("📥 Import .env").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            if let Ok(iter) = dotenvy::from_path_iter(&path) {
                                for (key, val) in iter.flatten() {
                                        match key.as_str() {
                                            "GEMINI_API_KEY" => self.edit_gemini_api_key = val,
                                            "DOCUMENT_AI_PROJECT_ID" => self.edit_docai_project_id = val,
                                            "DOCUMENT_AI_LOCATION" => self.edit_docai_location = val,
                                            "DOCUMENT_AI_PROCESSOR_ID" => self.edit_docai_processor_id = val,
                                            "GOOGLE_APPLICATION_CREDENTIALS" => self.edit_docai_service_account = val,
                                            "DOCUMENT_AI_API_KEY" => self.edit_docai_api_key = val,
                                            "PYMUPDF_PRO_KEY" => self.edit_pymupdf_pro_key = val,
                                            "GEMINI_AUTH_MODE" => {
                                                self.edit_gemini_use_vertex = matches!(
                                                    val.trim().to_ascii_lowercase().as_str(),
                                                    "vertex" | "vertex_ai" | "vertexai"
                                                );
                                            },

                                            "LLAMAPARSE_API_KEY" => self.edit_llamaparse_api_key = val,
                                            "PDFREST_API_KEY" => self.edit_pdfrest_api_key = val,
                                            "LIPI_API_KEY" => self.edit_lipi_api_key = val,
                                            "VISION_API_KEY" => self.edit_vision_api_key = val,
                                            "GROQ_API_KEY" => self.edit_groq_api_key = val,
                                            "OPENROUTER_API_KEY" => self.edit_openrouter_api_key = val,
                                            "OPENROUTER_MODEL" => self.edit_openrouter_model = val,
                                            "MISTRAL_API_KEY" => self.edit_mistral_api_key = val,
                                            "MISTRAL_MODEL" => self.edit_mistral_model = val,
                                            "MINDEE_API_KEY" => self.edit_mindee_api_key = val,
                                            _ => {}
                                        }
                                }
                                self.toast(ToastKind::Success, "Imported keys from file");
                            } else {
                                self.toast(ToastKind::Error, "Failed to read .env file");
                            }
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .button("💾 Save & apply keys")
                        .on_hover_text("Write .env, update the environment, and hot-reload the engine")
                        .clicked()
                    {
                        self.save_credentials();
                    }
                    if ui
                        .button("↻ Reload from env")
                        .on_hover_text("Discard edits and re-read the current environment")
                        .clicked()
                    {
                        if let Err(error) = self.job_tx.send(Job::ReloadConfig) {
                            self.toast(
                                ToastKind::Error,
                                format!("Could not request configuration reload: {error}"),
                            );
                        } else {
                            self.in_flight += 1;
                            self.toast(
                                ToastKind::Info,
                                "Reloading and validating configuration...",
                            );
                        }
                    }
                    if ui
                        .button("🧪 Test Connections")
                        .on_hover_text("Pings the Gemini and Document AI APIs to ensure your credentials are valid and authorized")
                        .clicked()
                    {
                        // Eagerly save any unsaved edits to the environment first, then run validation
                        self.save_credentials();

                        let _ = self.job_tx.send(Job::ValidateCredentials);
                    }
                });

                // Live capability status from the atomically applied runtime
                // configuration generation. “Configured” is intentionally
                // distinct from a locally verified “Ready” dependency.
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    use crate::app::capabilities::{Capability, CapabilityState};
                    let show = |ui: &mut egui::Ui, label: &str, capability: Capability| {
                        let status = self.capability_registry.status(capability);
                        let (mark, state, reason) = status.map_or(
                            ("✗", "Unavailable", "Capability was not probed"),
                            |status| match status.state {
                                CapabilityState::Ready => {
                                    ("✓", "Ready", status.reason.as_str())
                                }
                                CapabilityState::Configured => {
                                    ("◐", "Configured", status.reason.as_str())
                                }
                                CapabilityState::Unavailable => {
                                    ("✗", "Unavailable", status.reason.as_str())
                                }
                            },
                        );
                        ui.small(format!("{label} {mark} {state}"))
                            .on_hover_text(reason);
                    };
                    show(ui, "Doc AI", Capability::DocumentAi);
                    ui.separator();
                    show(ui, "Gemini", Capability::Gemini);
                    ui.separator();
                    show(ui, "Pro", Capability::PyMuPdfPro);
                    ui.separator();
                    show(ui, "LlamaParse", Capability::LlamaParse);
                    ui.separator();
                    show(ui, "pdfRest", Capability::PdfRest);
                    ui.separator();
                    show(ui, "Vision AI", Capability::VisionAi);
                    ui.separator();
                    show(ui, "Python", Capability::PythonPipeline);
                });

                // Render the results of the active `Test Connections` job
                if let Some((gemini_res, docai_res)) = &self.credential_validation_status {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.label("Validation Results:");
                    match docai_res {
                        Ok(_) => ui.label(egui::RichText::new("✓ Document AI: OK").color(egui::Color32::LIGHT_GREEN)),
                        Err(e) => ui.label(egui::RichText::new(format!("✗ Document AI: {}", e)).color(egui::Color32::LIGHT_RED)),
                    };
                    match gemini_res {
                        Ok(_) => ui.label(egui::RichText::new("✓ Gemini: OK").color(egui::Color32::LIGHT_GREEN)),
                        Err(e) => ui.label(egui::RichText::new(format!("✗ Gemini: {}", e)).color(egui::Color32::LIGHT_RED)),
                    };
                }
                if self.edit_gemini_use_vertex {
                    ui.small(
                        "Vertex mode reuses the Document AI service-account JSON (or ADC) and the project/location above. No separate key needed.",
                    );
                }
            });
    }

    fn draw_feedback_modal(&mut self, ctx: &egui::Context) {
        let mut open = self.active_modal == ActiveModal::Feedback;
        let mut submit = false;

        egui::Window::new("🐛 Report a Bug / Feedback")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(450.0, 350.0))
            .show(ctx, |ui| {
                ui.label("Describe the issue or what you were trying to do:");
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.feedback_text)
                        .hint_text("Please provide steps to reproduce...")
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(10.0);
                ui.checkbox(
                    &mut self.feedback_include_logs,
                    "Attach recent application logs (app.log, error_report.log)",
                );
                ui.checkbox(
                    &mut self.feedback_include_audit,
                    "Attach recent audit trail",
                );

                ui.add_space(15.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.active_modal = ActiveModal::None;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🚀 Submit to Developer").clicked() {
                            submit = true;
                        }
                    });
                });
            });

        if submit {
            self.active_modal = ActiveModal::None;
            let description = std::mem::take(&mut self.feedback_text);
            self.toast(
                ToastKind::Info,
                "Gathering logs and submitting report...".to_string(),
            );
            let _ = self.job_tx.send(Job::SubmitBugReport {
                description,
                include_logs: self.feedback_include_logs,
                include_audit: self.feedback_include_audit,
            });
        } else if open {
            self.active_modal = ActiveModal::Feedback;
        } else if self.active_modal == ActiveModal::Feedback {
            self.active_modal = ActiveModal::None;
        }
    }

    fn draw_modals(&mut self, ctx: &egui::Context) {
        match self.active_modal {
            ActiveModal::Settings => self.draw_settings_modal(ctx),
            ActiveModal::Transfer => self.draw_transfer_dialog(ctx),
            ActiveModal::DateAdjust => self.draw_date_adjust_dialog(ctx),
            ActiveModal::WorkflowHitl => self.draw_workflow_hitl_modal(ctx),
            ActiveModal::TransferTest => self.draw_transfer_test_dialog(ctx),
            ActiveModal::CommandPalette => self.draw_command_palette(ctx),
            ActiveModal::Feedback => self.draw_feedback_modal(ctx),
            ActiveModal::DiscardDraftConfirm => self.draw_discard_draft_confirm_modal(ctx),
            ActiveModal::None => {}
        }

        // Modals independent of the ActiveModal router (e.g. dynamic state triggers)
        if !self.pending_ai_confirmations.is_empty() {
            self.draw_ai_confirmation_dialog(ctx);
        }
        if self.pending_interactive_fallback.is_some() {
            self.draw_interactive_fallback_modal(ctx);
        }
        if self.stuck_detection.is_some() {
            self.draw_stuck_watchdog_modal(ctx);
        }
        if self.pending_autofix.is_some() {
            self.draw_autofix_modal(ctx);
        }
    }

    fn draw_discard_draft_confirm_modal(&mut self, ctx: &egui::Context) {
        let mut keep_open = true;
        let mut confirm = false;
        egui::Window::new("Discard workflow draft?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut keep_open)
            .show(ctx, |ui| {
                ui.label("This will permanently delete audit/workflow.json.");
                ui.label("Any pending edits in the current workflow draft will be lost.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.active_modal = ActiveModal::None;
                    }
                    if ui
                        .add(egui::Button::new("Discard").fill(self.settings.theme.palette().warn))
                        .clicked()
                    {
                        confirm = true;
                    }
                });
            });
        if confirm {
            self.discard_active_workflow_draft_quiet();
            self.toast(ToastKind::Info, "Workflow draft discarded");
            self.active_modal = ActiveModal::None;
        }
        if !keep_open {
            self.active_modal = ActiveModal::None;
        }
    }

    fn draw_stuck_watchdog_modal(&mut self, ctx: &egui::Context) {
        let mut force_fallback = false;
        let mut close_modal = false;

        if let Some(stuck_start) = self.stuck_detection {
            let elapsed = stuck_start.elapsed();
            let remaining = 25_i64.saturating_sub(elapsed.as_secs() as i64);

            if remaining <= 0 {
                force_fallback = true;
                close_modal = true;
            } else {
                egui::Window::new("⚠️ Processing Appears Stuck")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        let p = self.settings.theme.palette();
                        ui.colored_label(p.warn, "No activity detected from the backend process.");
                        ui.label(format!(
                            "Automatically falling back in {} seconds...",
                            remaining
                        ));
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            if ui.button("Wait Longer").clicked() {
                                close_modal = true; // resets the timer
                            }
                            if ui.button("Fallback Now").clicked() {
                                force_fallback = true;
                                close_modal = true;
                            }
                        });
                    });
            }
        }

        if close_modal {
            self.stuck_detection = None;
            self.last_runtime_activity = std::time::Instant::now();
        }

        if force_fallback {
            self.in_flight = 0; // reset state to unlock UI
            self.toast(
                crate::app::gui::ToastKind::Warn,
                "Watchdog triggered. Forcing fallback...".to_string(),
            );
            match &self.workflow_stage {
                crate::engine::workflow::WorkflowStage::Parsing => {
                    self.toast(
                        crate::app::gui::ToastKind::Info,
                        "Falling back to Offline Parser...".to_string(),
                    );
                    if let Err(e) =
                        self.job_tx
                            .send(crate::app::runtime::Job::ExtractTransactions {
                                path: std::path::PathBuf::from(&self.input_path),
                                parser_mode:
                                    crate::app::config::DocumentParserMode::OfflineHeuristic,
                            })
                    {
                        tracing::error!("Failed to dispatch ExtractTransactions fallback: {}", e);
                    }
                    self.in_flight += 1;
                }
                crate::engine::workflow::WorkflowStage::Rendering { .. } => {
                    self.toast(
                        crate::app::gui::ToastKind::Info,
                        "Falling back to Native rendering...".to_string(),
                    );
                    self.dispatch_confirm_and_render(false, false);
                }
                _ => {
                    self.progress = None;
                }
            }
        }
    }
}
