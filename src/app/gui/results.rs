//! Result handling for asynchronous background runtime jobs.
#![allow(unused_imports)]

use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::gui::state::{ActiveModal, MyApp, ProgressState, ToastKind};
use crate::app::runtime::{Job, JobId, JobResult, PythonJobResult};
use crate::engine::history::ChangeHistory;
use crate::engine::verification::VerificationReport;

impl MyApp {
    pub(crate) fn handle_job_result(&mut self, ctx: &egui::Context, res: JobResult) {
        match res {
            JobResult::UfoLog(line) => {
                self.ufo_logs.push(line);
            }
            JobResult::UfoAutoEditResult(val) => {
                self.is_ufo_running = false;
                self.progress = None;
                // in_flight freed once in the drain loop via ends_gui_tracked_job
                if self.ufo_user_cancelled {
                    self.ufo_user_cancelled = false;
                    return;
                }

                let status = val["status"].as_str().unwrap_or("unknown");
                let task_id = val["task_id"].as_str().unwrap_or("unknown");
                if status == "success" {
                    self.toast(
                        ToastKind::Success,
                        format!("UFO Auto-Edit Complete (Task: {task_id})"),
                    );
                } else {
                    let message = val["message"]
                        .as_str()
                        .or_else(|| val["output"].as_str())
                        .unwrap_or("UFO task finished without success");
                    self.toast(
                        ToastKind::Error,
                        format!("UFO Auto-Edit failed (Task: {task_id}): {message}"),
                    );
                }
            }

            JobResult::McpRenderComplete { .. } => {
                self.progress = None;
            }
            JobResult::ImbalanceExplained { explanation } => {
                self.progress = None;
                self.ai_explanation = Some(explanation);
            }
            JobResult::WatchdogEvent(event) => {
                match event {
                    crate::app::watchdog::WatchdogEvent::StallDetected(_timeout) => {
                        self.stuck_detection = Some(std::time::Instant::now());
                    }
                    crate::app::watchdog::WatchdogEvent::FallbackTriggered => {
                        // The modal in modals.rs will handle the actual fallback triggering
                        // because it monitors stuck_detection. We can just leave it or force it.
                        self.stuck_detection =
                            Some(std::time::Instant::now() - std::time::Duration::from_secs(30));
                    }
                    crate::app::watchdog::WatchdogEvent::Recovered => {
                        self.stuck_detection = None;
                    }
                    crate::app::watchdog::WatchdogEvent::Telemetry { cpu_usage, ram_mb } => {
                        self.telemetry_cpu = cpu_usage;
                        self.telemetry_ram_mb = ram_mb;
                    }
                }
            }
            JobResult::ApiKeysVerified(report) => {
                use crate::app::api_verification::VerificationStatus;
                for res in &report.results {
                    if res.status == VerificationStatus::Failed {
                        let msg = res.error_message.as_deref().unwrap_or_default();
                        if msg.contains("429") {
                            self.toast(
                                ToastKind::Error,
                                format!(
                                    "{} quota exceeded (429). Temporarily disabled.",
                                    res.service
                                ),
                            );
                            self.api_availability.disable_service(&res.service);
                        } else if msg.contains("401") || msg.contains("403") {
                            self.toast(
                                ToastKind::Error,
                                format!("{} auth failed. Check your API key.", res.service),
                            );
                            self.api_availability.disable_service(&res.service);
                        }
                    }
                }
                self.api_health = Some(report.results);
            }
            JobResult::BugReportSubmitted => {
                self.toast(
                    ToastKind::Success,
                    "Bug report submitted successfully! Thank you.".to_string(),
                );
            }
            JobResult::DocumentLoaded { total_pages, .. } => {
                self.total_pages = total_pages;
                self.current_page = 0;
                self.current_pdf_path = PathBuf::from(&self.input_path);
                self.previous_pdf_path = None;
                self.update_recent_files(self.input_path.clone());
                self.status = format!("Loaded {total_pages} page(s)");
                self.toast(ToastKind::Success, format!("Loaded {total_pages} pages"));
                self.request_render("current");
                // Keep the original open_pdf in_flight wait open across the
                // auto-chained parse (DocumentLoaded is non-ending). Do not +1
                // again here or the counter permanently drifts.
                self.workflow_edits.clear();
                self.workflow_preview = None;
                self.workflow_visual = None;
                self.workflow_outcome = None;
                self.font_cascade_reports.clear();
                self.workflow_dirty = true;
                if let Err(e) = self.dispatch_workflow_job(Job::WorkflowParseAndValidate {
                    input: PathBuf::from(&self.input_path),
                    version: Some(self.selected_parser_version.clone()),
                    parser_mode: self.settings.document_parser,
                    ai_provider: self.settings.ai_provider,
                    ignore_offline_fallback: false,
                }) {
                    tracing::error!("Runtime disconnected: {}", e);
                    // Parse never started: free the open_pdf wait slot.
                    self.in_flight = self.in_flight.saturating_sub(1);
                }
            }
            JobResult::HistoryUpdated { history } => {
                self.history_state = history;
                let idx = self.history_state.current_index();
                self.previous_pdf_path = Some(self.current_pdf_path.clone());
                self.current_pdf_path = if idx > 0 {
                    self.history_state.get_history()[idx - 1]
                        .snapshot_path
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| PathBuf::from(&self.input_path))
                } else {
                    PathBuf::from(&self.input_path)
                };
                self.status = "History synchronized".into();
                self.request_render("current");
            }
            JobResult::PageRendered {
                png_bytes,
                tag,
                width_pts,
                height_pts,
                ..
            } => {
                let texture = self.load_texture_from_bytes(ctx, &tag, &png_bytes);
                match tag.as_str() {
                    "current" => {
                        self.current_page_texture = texture;
                        self.current_page_size_pts = Some((width_pts, height_pts));
                    }
                    "before" => self.before_texture = texture,
                    "after" => self.after_texture = texture,
                    "transfer_source" => self.transfer_source_texture = texture,
                    "transfer_target" => self.transfer_target_texture = texture,
                    _ => {}
                }
            }
            JobResult::ChangeApplied {
                record,
                requires_visual_review,
            } => {
                self.toast(
                    if requires_visual_review {
                        ToastKind::Warn
                    } else {
                        ToastKind::Success
                    },
                    format!("Edit applied: {} -> {}", record.old_text, record.new_text),
                );
                if requires_visual_review {
                    self.last_warning = Some("Review required: complex background.".into());
                }
                self.status = "Change applied".into();
                self.request_render("current");
                self.request_render("before");
                self.request_render("after");
            }
            JobResult::BalanceProposed { imbalance, changes } => {
                self.last_imbalance = Some(imbalance);
                self.proposed_changes = changes.into_iter().map(|c| (c, true)).collect();
                if self.proposed_changes.is_empty() {
                    self.status = "Statement is already perfectly balanced.".into();
                    self.toast(ToastKind::Success, "Statement is already balanced.");
                } else {
                    self.status = format!(
                        "Proposed {} adjustments for ${:.2} imbalance",
                        self.proposed_changes.len(),
                        imbalance
                    );
                    self.toast(
                        ToastKind::Info,
                        format!("{} adjustments proposed", self.proposed_changes.len()),
                    );
                }
            }
            JobResult::ProposedChangesApplied {
                changes_applied,
                failures,
            } => {
                if failures.is_empty() {
                    self.toast(
                        ToastKind::Success,
                        format!("Applied {changes_applied} changes"),
                    );
                } else {
                    self.toast(
                        ToastKind::Warn,
                        format!("Applied {changes_applied} ({} failures)", failures.len()),
                    );
                }
                // Statement may have changed on disk; refresh the views.
                self.request_render("current");
                self.request_render("before");
                self.request_render("after");
            }
            JobResult::ConfigReloaded {
                generation,
                config,
                document_ai_configured,
                gemini_configured,
                pro_editing_available,
            } => {
                self.config_status = Some((
                    document_ai_configured,
                    gemini_configured,
                    pro_editing_available,
                ));
                let mut parts = Vec::new();
                parts.push(format!(
                    "Document AI {}",
                    if document_ai_configured { "✓" } else { "✗" }
                ));
                parts.push(format!(
                    "Gemini {}",
                    if gemini_configured { "✓" } else { "✗" }
                ));
                parts.push(format!(
                    "Pro editing {}",
                    if pro_editing_available { "✓" } else { "✗" }
                ));
                let summary = parts.join(" Â· ");
                self.status = format!("Configuration generation {generation} applied: {summary}");
                // Refresh every GUI consumer from the exact immutable runtime
                // generation instead of independently re-reading process state.
                self.sync_credential_editors_from_config(&config);
                let fresh_avail = config.detect_availability();
                fresh_avail.log_summary();
                self.api_availability = fresh_avail;
                self.capability_registry =
                    crate::app::capabilities::CapabilityRegistry::probe(&config, &self.app_paths);
                self.config = config;
                self.toast(
                    if document_ai_configured && gemini_configured {
                        ToastKind::Success
                    } else {
                        ToastKind::Warn
                    },
                    format!("Credentials reloaded - {summary}"),
                );
            }
            JobResult::TransactionsExtracted(txs) => {
                self.toast(
                    ToastKind::Success,
                    format!("Extracted {} transactions", txs.len()),
                );
                self.workflow_transactions = txs;
                self.workflow_dirty = true;
                self.last_runtime_activity = std::time::Instant::now();
            }
            JobResult::NaturalLanguageEditReady(txs) => {
                self.toast(
                    ToastKind::Success,
                    format!("Applied AI edit to {} transactions", txs.len()),
                );
                self.workflow_transactions = txs;
                self.workflow_dirty = true;
                self.last_runtime_activity = std::time::Instant::now();
            }
            JobResult::CategorizationReady(txs) => {
                self.toast(
                    ToastKind::Success,
                    format!("Categorized {} transactions", txs.len()),
                );
                self.workflow_transactions = txs;
                self.workflow_dirty = true;
                self.last_runtime_activity = std::time::Instant::now();
            }
            JobResult::FontCompleted(_) => {
                self.toast(ToastKind::Success, "Font completion finished");
            }
            JobResult::ChangeHistoryExported { path } => {
                self.toast(
                    ToastKind::Success,
                    format!("History exported: {}", path.display()),
                );
            }
            JobResult::ReconstructComplete { output_path } => {
                self.toast(
                    ToastKind::Success,
                    format!(
                        "Typst Reconstruction Complete! Output saved to: {:?}",
                        output_path
                    ),
                );
            }
            JobResult::VerificationReport(report) => {
                self.last_verification = Some(report.clone());
                let passed = report.mandatory_local_pass();
                let failed_gates = report
                    .gates
                    .iter()
                    .filter(|gate| {
                        gate.mandatory
                            && gate.status
                                != crate::engine::verification::VerificationGateStatus::Passed
                    })
                    .map(|gate| gate.id.as_str())
                    .collect::<Vec<_>>();
                let summary = if passed {
                    "Verification PASS: every mandatory local gate passed".to_string()
                } else {
                    format!("Verification FAIL: {}", failed_gates.join(", "))
                };
                self.status = summary.clone();
                self.toast(
                    if passed {
                        ToastKind::Success
                    } else {
                        ToastKind::Error
                    },
                    summary,
                );
            }
            JobResult::Progress { label, fraction } => {
                if fraction >= 1.0 {
                    self.progress = None;
                } else {
                    let started_at = match &self.progress {
                        Some(p) if p.label == label => p.started_at,
                        _ => std::time::Instant::now(),
                    };
                    self.progress = Some(ProgressState {
                        label,
                        fraction,
                        started_at,
                    });
                }
            }
            JobResult::Error { job_label, message } => {
                if job_label == "ufo_dispatch" {
                    self.is_ufo_running = false;
                    // User cancel already toasted; process kill surfaces as Error.
                    // Free in_flight only via the drain ends_gui_tracked_job path.
                    if self.ufo_user_cancelled {
                        self.ufo_user_cancelled = false;
                        self.progress = None;
                        return;
                    }
                }
                // in_flight decremented once in the drain loop via ends_gui_tracked_job
                self.progress = None;
                if matches!(
                    job_label.as_str(),
                    "workflow_parse_and_validate"
                        | "workflow_preview"
                        | "workflow_confirm_and_render"
                        | "ai_fix_visual_fidelity"
                ) {
                    self.active_workflow_job_id = None;
                }

                if message.starts_with("__DISPATCH:") {
                    let parts: Vec<&str> = message.split(':').collect();
                    if parts.len() >= 2 {
                        match parts[1] {
                            "Undo" => {
                                let _ = self.job_tx.send(crate::app::runtime::Job::Undo);
                            }
                            "Redo" => {
                                let _ = self.job_tx.send(crate::app::runtime::Job::Redo);
                            }
                            "Balance" => {
                                let auto_apply = parts
                                    .get(2)
                                    .map(|v| *v == "true" || *v == "1")
                                    .unwrap_or(false);
                                let path = std::path::PathBuf::from(&self.input_path);
                                if auto_apply {
                                    let output = path.with_extension("balanced.pdf");
                                    let _ = self.job_tx.send(
                                        crate::app::runtime::Job::BalanceAndApplyAll {
                                            input: path,
                                            output,
                                            auto_apply: true,
                                        },
                                    );
                                } else {
                                    let _ = self
                                        .job_tx
                                        .send(crate::app::runtime::Job::BalanceStatement { path });
                                }
                            }
                            "Verify" => {
                                let _ = self
                                    .job_tx
                                    .send(crate::app::runtime::Job::ValidateCredentials);
                            }
                            "Extract" => {
                                let _ = self.job_tx.send(
                                    crate::app::runtime::Job::ExtractTransactions {
                                        path: std::path::PathBuf::from(&self.input_path),
                                        parser_mode:
                                            crate::app::config::DocumentParserMode::from_env(),
                                    },
                                );
                            }
                            "Transfer" => {
                                let target_bank = parts.get(2).copied().unwrap_or("");
                                let source = self.transfer_source_path.trim();
                                let target = self.input_path.trim();
                                let ready = !source.is_empty()
                                    && !target.is_empty()
                                    && target != "examples/sample.pdf"
                                    && source != target;
                                if ready {
                                    let target_pdf = std::path::PathBuf::from(target);
                                    let output_pdf = if self.output_path.is_empty() {
                                        target_pdf.with_file_name(format!(
                                            "{}_transferred.pdf",
                                            target_pdf
                                                .file_stem()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                        ))
                                    } else {
                                        std::path::PathBuf::from(&self.output_path)
                                    };
                                    let _ = self.job_tx.send(
                                        crate::app::runtime::Job::TransferTransactions {
                                            source_pdf: std::path::PathBuf::from(source),
                                            target_pdf,
                                            output_pdf,
                                        },
                                    );
                                    self.toast(
                                        ToastKind::Info,
                                        format!(
                                            "Transfer started{}…",
                                            if target_bank.is_empty() {
                                                String::new()
                                            } else {
                                                format!(" (target bank: {target_bank})")
                                            }
                                        ),
                                    );
                                } else {
                                    self.toast(
                                        ToastKind::Warn,
                                        "Transfer requires a source statement and a different target PDF selected in the Transfer workflow.",
                                    );
                                }
                            }
                            "AdjustDates" => {
                                let _ = self.job_tx.send(
                                    crate::app::runtime::Job::NaturalLanguageEdit {
                                        prompt: format!(
                                            "Shift dates by {}",
                                            parts.get(2).unwrap_or(&"0")
                                        ),
                                        transactions: self.workflow_transactions.clone(),
                                    },
                                );
                            }
                            "Categorize" => {
                                let _ = self.job_tx.send(
                                    crate::app::runtime::Job::CategorizeTransactions {
                                        transactions: self.workflow_transactions.clone(),
                                    },
                                );
                            }
                            "Doctor" => {
                                let _ = self
                                    .job_tx
                                    .send(crate::app::runtime::Job::ValidateCredentials);
                            }
                            "ReloadConfig" => {
                                let _ = self.job_tx.send(crate::app::runtime::Job::ReloadConfig);
                            }
                            "StressTest" => {
                                let _ =
                                    self.job_tx
                                        .send(crate::app::runtime::Job::RunTransferTests {
                                            statements: vec![],
                                            max_iterations: 1,
                                        });
                            }
                            _ => {}
                        }
                    }
                    return;
                }

                // Autofix interception for ALL errors
                let err = crate::app::error::AppError::parse_msg(&message)
                    .unwrap_or_else(|| crate::app::error::AppError::Unknown(message.clone()));

                match &err {
                    crate::app::error::AppError::ApiFailure(m) => {
                        let suggestion = err.suggested_action().unwrap_or("");
                        self.toast(
                            ToastKind::Error,
                            format!("API Error: {}\n{}", m, suggestion),
                        );
                    }
                    _ => {
                        self.pending_autofix = Some(err);
                    }
                }

                tracing::error!("[gui] runtime error in '{}': {}", job_label, message);

                // Write comprehensive error sink
                let dir = std::path::PathBuf::from("audit/error_reports");
                let _ = std::fs::create_dir_all(&dir);
                let filename = format!("report_{}.json", chrono::Utc::now().format("%Y%m%d%H%M%S"));
                let report = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "kind": "JobError",
                    "job_label": job_label,
                    "message": message,
                    "input_path": self.input_path,
                });
                let _ = std::fs::write(
                    dir.join(filename),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
            }
            JobResult::Pong => {
                self.toast(ToastKind::Info, "pong");
            }
            JobResult::FontAnalysisReady(analysis) => {
                let line = analysis.one_line_summary();
                let kind = if analysis.summary.all_fonts_covered {
                    ToastKind::Success
                } else {
                    ToastKind::Warn
                };
                self.toast(kind, line);
                self.font_analysis = Some(analysis);
            }
            JobResult::FontCascadeUsed(report) => {
                let summary = report.one_line_summary();
                let kind = if report.success {
                    ToastKind::Success
                } else {
                    ToastKind::Warn
                };
                self.toast(kind, summary);
                self.font_cascade_reports.push(report);
            }
            JobResult::Cancelled { id } => {
                self.progress = None;
                if self.active_workflow_job_id == Some(id) {
                    self.active_workflow_job_id = None;
                }
                self.toast(ToastKind::Info, format!("Cancelled job #{id}"));
                self.status = format!("Cancelled job #{id}");
            }
            JobResult::TimedOut { id, job_label } => {
                self.progress = None;
                if self.active_workflow_job_id == Some(id) {
                    self.active_workflow_job_id = None;
                }
                self.toast(
                    ToastKind::Error,
                    format!("{job_label} timed out (job #{id})"),
                );
                self.status = format!("Timed out: {job_label}");
            }

            // ---- Multi-stage workflow ----------------------------------
            JobResult::WorkflowStageChanged { stage } => {
                let opens_modal = matches!(
                    &stage,
                    crate::engine::workflow::WorkflowStage::VisualFidelityWarning { .. }
                        | crate::engine::workflow::WorkflowStage::ImbalanceCorrectionWarning { .. }
                        | crate::engine::workflow::WorkflowStage::FontCoverageWarning { .. }
                        | crate::engine::workflow::WorkflowStage::OfflineFallbackWarning
                );
                let event = crate::engine::workflow::WorkflowEvent::from_stage(stage);
                if self.apply_workflow_event(event) && opens_modal {
                    self.active_modal = ActiveModal::WorkflowHitl;
                }
            }
            JobResult::VisualAlternativesReady(images) => {
                let event = crate::engine::workflow::WorkflowEvent::ShowVisualComparison { images };
                if self.apply_workflow_event(event) {
                    self.active_modal = ActiveModal::WorkflowHitl;
                }
            }
            JobResult::WorkflowParseValidated {
                validation,
                transactions,
            } => {
                let count = validation.transactions_found;
                let score = validation.completeness_score;
                self.workflow_validation = Some(validation);
                self.workflow_transactions = transactions;
                // Stage 13 / Item #4: stale cell-buffer entries from a
                // prior parse can still appear in the inline edit table
                // because they are keyed by (page, line_on_page, field).
                // Re-parsing may produce new line_on_page indices for the
                // same transactions; clear the buffers so the table
                // re-initialises from the fresh values.
                self.workflow_cell_buffers.clear();
                self.workflow_edits.clear();
                self.workflow_dirty = true;
                self.toast(
                    if score >= 0.85 {
                        ToastKind::Success
                    } else {
                        ToastKind::Warn
                    },
                    format!(
                        "Parsed {count} transactions • completeness {:.0}%",
                        score * 100.0
                    ),
                );
            }
            JobResult::WorkflowPreviewBuilt(preview) => {
                let kind = if preview.balanced {
                    ToastKind::Success
                } else {
                    ToastKind::Warn
                };
                self.toast(
                    kind,
                    format!(
                        "Preview ready • {} rows will change • imbalance ${:.2}",
                        preview.rows.iter().filter(|r| r.will_change).count(),
                        preview.final_imbalance
                    ),
                );
                self.workflow_preview = Some(preview);
            }
            JobResult::WorkflowVisualAttempt(attempt) => {
                self.toast(
                    if attempt.passed() {
                        ToastKind::Success
                    } else {
                        ToastKind::Info
                    },
                    format!(
                        "Visual attempt {}/{} • diff {:.4}",
                        attempt.attempt, attempt.max_attempts, attempt.diff_score
                    ),
                );
                self.workflow_visual = Some(attempt);
            }
            JobResult::WorkflowComplete(outcome) => {
                self.progress = None;
                self.active_workflow_job_id = None;
                self.toast(ToastKind::Success, outcome.completion_summary.clone());
                self.workflow_outcome = Some(outcome);
                // Stage 6: workflow finished cleanly - clear the in-flight
                // edit queue and remove the autosaved draft so the next
                // session starts fresh. Resume-draft now correctly reports
                // "no draft to resume" until new edits accumulate.
                self.workflow_edits.clear();
                self.workflow_cell_buffers.clear();
                self.workflow_dirty = false;
                self.discard_active_workflow_draft_quiet();
            }
            JobResult::WorkflowFailed(failure) => {
                self.progress = None;
                self.active_workflow_job_id = None;
                let msg = match &failure {
                    crate::engine::workflow::WorkflowFailure::ParseFailed(s) => {
                        format!("Parse failed: {s}")
                    }
                    crate::engine::workflow::WorkflowFailure::Incomplete { score, .. } => {
                        format!("Parse rejected as incomplete (score {score:.2})")
                    }
                    crate::engine::workflow::WorkflowFailure::FontCoverageFailed {
                        missing_chars,
                    } => {
                        format!("Font coverage missing chars: {missing_chars:?}")
                    }
                    crate::engine::workflow::WorkflowFailure::VisualNotConverged {
                        last_score,
                        attempts,
                    } => {
                        format!("Visual didn't converge after {attempts} tries; last diff {last_score:.4}")
                    }
                    crate::engine::workflow::WorkflowFailure::FinalMathInvalid { imbalance } => {
                        format!("Final math invalid: imbalance ${imbalance:.2}")
                    }
                    crate::engine::workflow::WorkflowFailure::FidelityCheckFailed(s) => {
                        format!("AI Fidelity Check Failed: {s}")
                    }
                    crate::engine::workflow::WorkflowFailure::Other(s) => s.clone(),
                };

                // Autofix interception
                if let Some(err) = crate::app::error::AppError::parse_msg(&msg) {
                    if err.suggested_action().is_some() {
                        self.pending_autofix = Some(err);
                    }
                }

                self.toast(ToastKind::Error, &msg);
                self.apply_workflow_event(crate::engine::workflow::WorkflowEvent::Fail(failure));

                let dir = std::path::PathBuf::from("audit/error_reports");
                let _ = std::fs::create_dir_all(&dir);
                let filename = format!("report_{}.json", chrono::Utc::now().format("%Y%m%d%H%M%S"));
                let report = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "kind": "WorkflowFailed",
                    "message": msg,
                    "input_path": self.input_path,
                });
                let _ = std::fs::write(
                    dir.join(filename),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
            }
            JobResult::JobCompleted {
                job_label,
                disposition,
                artifact,
                message,
            } => {
                use crate::app::runtime::OperationDisposition;
                self.progress = None;
                let (kind, prefix) = match disposition {
                    OperationDisposition::Succeeded => (ToastKind::Success, "Completed"),
                    OperationDisposition::NoOp => (ToastKind::Info, "No changes"),
                    OperationDisposition::Partial => (ToastKind::Warn, "Partially completed"),
                    OperationDisposition::Failed => (ToastKind::Error, "Failed"),
                    OperationDisposition::Cancelled => (ToastKind::Warn, "Cancelled"),
                    OperationDisposition::TimedOut => (ToastKind::Error, "Timed out"),
                };
                let artifact_note = artifact
                    .as_ref()
                    .map(|path| format!(" -> {}", path.display()))
                    .unwrap_or_default();
                let status = format!("{prefix} [{job_label}]: {message}{artifact_note}");
                self.status = status.clone();
                self.toast(kind, status);
            }
            JobResult::TransferComplete(result) => {
                self.progress = None;
                let msg = format!(
                    "✓ Transfer complete: {} txns -> output, math: {}, visual: {} (AI Layout: {}), ({:.1}s)",
                    result.source_tx_count,
                    if result.math_verified { "✓" } else { "✗" },
                    if result.visual_verified { "✓" } else { "✗" },
                    if result.visual_proof_path.is_some() { "APPROVED" } else { "N/A" },
                    result.total_duration_secs,
                );
                self.status = msg.clone();
                self.toast(ToastKind::Success, &msg);

                // Auto-load the output PDF
                let output_path = result.output_path.clone();
                if output_path.exists() {
                    self.open_pdf(output_path);

                    // Auto-load the source PDF as a side-by-side (Curtain Diff) layer
                    if !self.transfer_source_path.is_empty()
                        && self
                            .job_tx
                            .send(Job::RenderPage {
                                path: std::path::PathBuf::from(self.transfer_source_path.clone()),
                                page: 0,
                                dpi: self.settings.default_dpi,
                                tag: "after".to_string(), // Reuse 'after' texture slot for side-by-side
                            })
                            .is_ok()
                    {
                        self.in_flight += 1;
                        self.show_curtain = true;
                        self.curtain_ratio = 0.5; // Split 50/50 down the middle
                        self.toast(ToastKind::Info, "Loading side-by-side comparison...");
                    }
                }
            }
            JobResult::TransferFailed { stage, message } => {
                self.progress = None;
                let msg = format!("Transfer failed at {stage}: {message}");
                self.status = msg.clone();
                self.toast(ToastKind::Error, &msg);

                // Write error report
                let dir = std::path::PathBuf::from("audit/error_reports");
                let _ = std::fs::create_dir_all(&dir);
                let filename = format!(
                    "transfer_{}.json",
                    chrono::Utc::now().format("%Y%m%d%H%M%S")
                );
                let report = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "kind": "TransferFailed",
                    "stage": stage,
                    "message": message,
                    "input_path": self.input_path,
                });
                let _ = std::fs::write(
                    dir.join(filename),
                    serde_json::to_string_pretty(&report).unwrap_or_default(),
                );
            }
            JobResult::DatesAdjusted {
                records,
                output_path,
            } => {
                self.progress = None;
                let msg = format!(
                    "📅 Adjusted {} dates -> {}",
                    records.len(),
                    output_path.display()
                );
                self.status = msg.clone();
                self.toast(ToastKind::Success, &msg);
                // Auto-load the output
                if output_path.exists() {
                    self.open_pdf(output_path);
                }
            }
            JobResult::AiConfirmationNeeded(confirmation) => {
                self.pending_ai_confirmations.push(confirmation);
            }
            JobResult::InteractiveFallbackRequired(req) => {
                self.pending_interactive_fallback = Some(req);
            }
            JobResult::TransferTestsComplete(report) => {
                self.progress = None;
                let msg = report.summary();
                self.status = msg.clone();
                if report.all_passed() {
                    self.toast(ToastKind::Success, &msg);
                } else {
                    self.toast(ToastKind::Error, &msg);
                }
                self.transfer_test_report = Some(report);
            }

            // -- Document AI Version Management --
            JobResult::DocAiVersionsListed(versions) => {
                self.docai_versions = versions;
                self.docai_versions_loading = false;
                self.toast(
                    ToastKind::Info,
                    format!("Found {} processor versions", self.docai_versions.len()),
                );
            }
            JobResult::DocAiVersionOperationStarted {
                operation_name,
                description,
            } => {
                self.docai_training_status = Some(description.clone());
                self.docai_active_operation = Some(operation_name);
                self.toast(ToastKind::Info, &description);
            }
            JobResult::DocAiVersionError(msg) => {
                self.docai_versions_loading = false;
                self.docai_training_status = Some(format!("❌ {msg}"));
                self.toast(ToastKind::Error, &msg);
            }
            JobResult::NuclearFallbackRequired(msg) => {
                self.status = format!(
                    "Fidelity workflow stopped without replacing the document: {}",
                    msg
                );
                let toast_msg = self.status.clone();
                self.toast(ToastKind::Error, toast_msg);
            }
        }
    }
}
