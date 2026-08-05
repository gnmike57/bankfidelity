import re

with open('src/app/modals.rs', 'r') as f:
    content = f.read()

# Fix draw_transfer_dialog
old_transfer = """                        let btn = ui.add_enabled(
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
                            self.active_modal = ActiveModal::None;
                        }"""

new_transfer = """                        if self.in_flight > 0 {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(egui::RichText::new("Transfer in progress...").color(self.settings.theme.palette().accent));
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
                        }"""

if old_transfer in content:
    content = content.replace(old_transfer, new_transfer)
    print("Patched draw_transfer_dialog")
else:
    print("Old transfer code not found")


# Fix draw_date_adjust_dialog
old_date = """                    if ui.button("▶ Apply Range Adjustment").clicked() {
                        let input = std::path::PathBuf::from(&self.input_path);
                        let output = if self.output_path.is_empty() {
                            input.with_file_name(format!(
                                "{}_dated.pdf",
                                input.file_stem().unwrap_or_default().to_string_lossy()
                            ))
                        } else {
                            std::path::PathBuf::from(&self.output_path)
                        };

                        let mode = crate::engine::transfer::DateAdjustmentMode::Range(
                            self.date_adjust_start.clone(),
                            self.date_adjust_end.clone(),
                        );

                        let _ = self.job_tx.send(Job::AdjustDatePeriods {
                            input,
                            output,
                            mode,
                        });
                        self.in_flight += 1;
                        self.status = "Adjusting dates...".into();
                        self.toast(ToastKind::Info, "Date adjustment started.");
                        self.active_modal = ActiveModal::None;
                    }"""

new_date = """                    if self.in_flight > 0 {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Processing date adjustments...");
                        });
                    } else {
                        if ui.button("▶ Apply Range Adjustment").clicked() {
                            let input = std::path::PathBuf::from(&self.input_path);
                            let output = if self.output_path.is_empty() {
                                input.with_file_name(format!(
                                    "{}_dated.pdf",
                                    input.file_stem().unwrap_or_default().to_string_lossy()
                                ))
                            } else {
                                std::path::PathBuf::from(&self.output_path)
                            };

                            let mode = crate::engine::transfer::DateAdjustmentMode::Range(
                                self.date_adjust_start.clone(),
                                self.date_adjust_end.clone(),
                            );

                            let _ = self.job_tx.send(Job::AdjustDatePeriods {
                                input,
                                output,
                                mode,
                            });
                            self.in_flight += 1;
                            self.status = "Adjusting dates...".into();
                            self.toast(ToastKind::Info, "Date adjustment started.");
                        }
                    }"""

if old_date in content:
    content = content.replace(old_date, new_date)
    print("Patched draw_date_adjust_dialog")
else:
    print("Old date code not found")


# Fix draw_transfer_test_dialog
old_test = """                    if ui.button("▶ Run All Pair Tests").clicked() {
                        let statements: Vec<(std::path::PathBuf, std::path::PathBuf)> =
                            self.transfer_test_pairs
                            .iter()
                            .filter_map(|(s, t)| {
                                if s.is_empty() || t.is_empty() { None }
                                else { Some((std::path::PathBuf::from(s), std::path::PathBuf::from(t))) }
                            })
                            .collect();
                        let pairs = statements.len();
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
                        self.active_modal = ActiveModal::None;
                    }"""

new_test = """                    if self.in_flight > 0 {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Test runner is active...");
                        });
                    } else {
                        if ui.button("▶ Run All Pair Tests").clicked() {
                            let statements: Vec<(std::path::PathBuf, std::path::PathBuf)> =
                                self.transfer_test_pairs
                                .iter()
                                .filter_map(|(s, t)| {
                                    if s.is_empty() || t.is_empty() { None }
                                    else { Some((std::path::PathBuf::from(s), std::path::PathBuf::from(t))) }
                                })
                                .collect();
                            let pairs = statements.len();
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
                    }"""

if old_test in content:
    content = content.replace(old_test, new_test)
    print("Patched draw_transfer_test_dialog")
else:
    print("Old test code not found")

with open('src/app/modals.rs', 'w') as f:
    f.write(content)

