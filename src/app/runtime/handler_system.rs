#![allow(unused_imports)]
use super::context::JobContext;
use super::jobs::Job;
use super::parser_chain::interactive_fallback_or_continue;
use super::parser_chain::{
    extraction_provider_order, wait_for_interactive_choice, InteractiveFallbackRouter,
};
use super::python_job::{geometry_statement_from_json, PythonJob, PythonJobResult};
use super::results::{JobResult, OperationDisposition};
use super::tracking::{block_on_from_blocking_context, ResultSink, TerminalTracker};
use super::{dispatch_python_job, parse_with_offline_fallback};
use crate::engine::segments::{GlobalEdit, SegmentManager, SegmentMap};
use crate::pdf::engine::PdfEngine;
use crate::pdf::ReplaceOutcome;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use tokio::sync::oneshot;
use uuid::Uuid;

#[allow(clippy::too_many_lines, clippy::cognitive_complexity, unused_variables)]
pub(crate) async fn handle(job: Job, ctx: &mut JobContext<'_>) {
    let python_tx_clone = ctx.python_tx_clone.clone();
    let result_tx_clone = ctx.result_tx_clone.clone();
    let engine_for_tokio = ctx.engine_for_tokio.clone();
    let config_for_tokio = ctx.config_for_tokio.clone();
    let wdog = ctx.wdog.clone();
    let history = ctx.history.clone();
    let audit_log = ctx.audit_log.clone();
    let cancellations_for_loop = ctx.cancellations_for_loop.clone();
    let api_semaphore = ctx.api_semaphore.clone();
    let segment_map = &mut *ctx.segment_map;
    let segment_manager = &mut *ctx.segment_manager;
    let fallback_router = ctx.fallback_router.clone();
    let parse_cache = ctx.parse_cache.clone();
    let config_holder = ctx.config_holder.clone();

    match job {
        Job::Ping => {
            let (reply_tx, reply_rx) = oneshot::channel();
            if python_tx_clone.send((PythonJob::Ping, reply_tx)).is_ok() {
                if let Ok(PythonJobResult::Pong) = reply_rx.await {
                    let _ = result_tx_clone.send(JobResult::Pong);
                }
            }
        }

        Job::CancelUfo => {
            crate::ai::ufo::UfoClient::cancel_task();
        }

        Job::UfoAutoEdit { path, context } => {
            let res_tx = result_tx_clone.clone();
            tokio::spawn(async move {
                if !path.exists() {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "ufo_dispatch".into(),
                        message: format!(
                            "UFO Auto-Edit failed: statement not found at {}",
                            path.display()
                        ),
                    });
                    return;
                }

                let _ = res_tx.send(JobResult::Progress {
                    label: "Delegating to BankFidelity UFO Orchestrator...".into(),
                    fraction: 0.5,
                });

                let request = format!(
                    "Automatically extract, verify, and fully correct the formatting of the bank statement located at: {}\n\n\
CRITICAL SELF-CORRECTION PROTOCOL:\n\
1. After making any modification using `modify_text` or `transfer_transactions`, you MUST immediately call the `verify_layout` tool.\n\
2. If `verify_layout` reports an SSIM drop below 0.999 or any layout shift, you MUST use `local_ai_chat` to consult the local Qwen model for correction strategies.\n\
3. Revert or adjust the edit until absolute sub-pixel perfection is restored before finishing the task.\n\
4. Do NOT use `typst_reconstruct` for routine edit-in-place recovery; it cannot preserve edit-in-place visual fidelity. Prefer `modify_text` + `verify_layout` (and segmented 3-page mode for long statements).\n\n\
Additional Context:\n{context}",
                    path.display()
                );

                let res_tx_cb = res_tx.clone();
                let result = tokio::task::spawn_blocking(move || {
                    crate::ai::ufo::UfoClient::dispatch_task(
                        &request,
                        Some(move |log_line: String| {
                            let _ = res_tx_cb.send(JobResult::UfoLog(log_line));
                        }),
                    )
                })
                .await
                .unwrap_or_else(|e| {
                    Err(crate::ai::ufo::UfoError::Unknown(format!(
                        "Tokio spawn_blocking panicked: {e}"
                    )))
                });

                match result {
                    Ok(val) => {
                        let _ = res_tx.send(JobResult::UfoAutoEditResult(
                            serde_json::to_value(val).unwrap_or(serde_json::Value::Null),
                        ));
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ufo_dispatch".into(),
                            message: format!("UFO Auto-Edit failed: {e}"),
                        });
                    }
                }
            });
        }

        Job::SubmitBugReport {
            description,
            include_logs,
            include_audit: _,
        } => {
            let res_tx = result_tx_clone.clone();
            let webhook_url = std::env::var("WEBHOOK_URL").unwrap_or_default();
            let log_dir = config_for_tokio.log_dir.clone();

            tokio::spawn(async move {
                if webhook_url.is_empty() {
                    tracing::error!("Cannot submit bug report: WEBHOOK_URL is not configured.");
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "SubmitBugReport".to_string(),
                        message: "Webhook URL not configured".to_string(),
                    });
                    return;
                }

                let mut payload = serde_json::json!({
                    "content": format!("**New Bug Report**\n\n```\n{}\n```", description)
                });

                // Include only a bounded, re-scrubbed tail from the newest managed
                // rolling log. Full logs, statement content, and credentials are never
                // attached automatically.
                if include_logs {
                    match crate::app::telemetry::support_log_tail(&log_dir, 50, 64 * 1024) {
                        Ok(tail) if !tail.is_empty() => {
                            payload["content"] = serde_json::Value::String(format!(
                                "{}\n\n**Scrubbed App Log (Tail)**\n```\n{}\n```",
                                payload["content"].as_str().unwrap_or_default(),
                                tail
                            ));
                        }
                        Ok(_) => {
                            tracing::info!("No managed log tail was available for the bug report")
                        }
                        Err(error) => tracing::warn!(
                            "Could not prepare the bounded support log tail: {error}"
                        ),
                    }
                }

                let client = reqwest::Client::new();
                match client.post(&webhook_url).json(&payload).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        let _ = res_tx.send(JobResult::BugReportSubmitted);
                    }
                    Ok(resp) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "SubmitBugReport".to_string(),
                            message: format!("Server returned {}", resp.status()),
                        });
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "SubmitBugReport".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            });
        }

        Job::Python(py_job, reply_tx) => {
            match py_job {
                PythonJob::FindTextBlockAtClick { .. } => {
                    let (int_tx, int_rx) = oneshot::channel();
                    dispatch_python_job(py_job, int_tx, &python_tx_clone);
                    tokio::spawn(async move {
                        if let Ok(res) = int_rx.await {
                            match res {
                                PythonJobResult::Error(_) => {
                                    // Benign no-op for click detection
                                }
                                _ => {
                                    let _ = reply_tx.send(res);
                                }
                            }
                        }
                    });
                }
                _ => {
                    dispatch_python_job(py_job, reply_tx, &python_tx_clone);
                }
            }
        }

        Job::AiCommand { prompt, path } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            let engine_ref = engine_for_tokio.clone();
            tokio::spawn(async move {
                // ── Reserved test hook: cascade simulation ──────────────────
                if prompt == "SIMULATE_CASCADE_EDITS" {
                    for i in 1..=100 {
                        let _ = res_tx.send(JobResult::Progress {
                            label: format!("Simulating cascade chunk {}", i),
                            fraction: (i as f32) / 100.0,
                        });
                        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                    }
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "cascade_test".into(),
                        message: "Cascade stress test completed successfully. 10,000 recalculations rendered.".into(),
                    });
                    return;
                }

                // ── NLP Router: two-pass parse then dispatch ─────────────────
                let _ = res_tx.send(JobResult::Progress {
                    label: format!("Parsing: \"{}\"…", &prompt[..prompt.len().min(60)]),
                    fraction: 0.1,
                });

                use crate::app::nlp_router::{parse as nlp_parse, NlpCommand};
                let cmd = nlp_parse(&prompt);
                tracing::info!("[AiCommand] NLP parsed: {:?}", cmd);

                let _ = res_tx.send(JobResult::Progress {
                    label: cmd.describe(),
                    fraction: 0.2,
                });

                match cmd {
                    // Fast-path commands: signal the GUI to re-dispatch the
                    // appropriate synchronous job on the main thread.
                    NlpCommand::Undo => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: "__DISPATCH:Undo".into(),
                        });
                    }
                    NlpCommand::Redo => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: "__DISPATCH:Redo".into(),
                        });
                    }
                    NlpCommand::Balance { auto_apply, target } => {
                        let t = target.map(|v| format!("{:.2}", v)).unwrap_or_default();
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:Balance:{}:{}", auto_apply, t),
                        });
                    }
                    NlpCommand::Verify { mode } => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:Verify:{}", mode),
                        });
                    }
                    NlpCommand::Extract { provider } => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:Extract:{}", provider),
                        });
                    }
                    NlpCommand::Transfer {
                        target_bank,
                        source_bank,
                    } => {
                        let src = source_bank.unwrap_or_default();
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:Transfer:{}:{}", target_bank, src),
                        });
                    }
                    NlpCommand::TypstReconstruct => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: "__DISPATCH:TypstReconstruct".into(),
                        });
                    }
                    NlpCommand::UfoAutomate { task_prompt } => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:UfoAutomate:{}", task_prompt),
                        });
                    }
                    NlpCommand::FontAnalysis => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: "__DISPATCH:FontAnalysis".into(),
                        });
                    }
                    NlpCommand::ClarificationRequired {
                        reason,
                        suggestions,
                        ..
                    } => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command".into(),
                            message: format!(
                                "Clarification required: {}. Suggestions: {}",
                                reason,
                                suggestions.join(", ")
                            ),
                        });
                    }
                    NlpCommand::AdjustDates { shift_days } => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:AdjustDates:{}", shift_days),
                        });
                    }
                    NlpCommand::Categorize { provider } => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:Categorize:{}", provider),
                        });
                    }
                    NlpCommand::Doctor => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: "__DISPATCH:Doctor".into(),
                        });
                    }
                    NlpCommand::ReloadConfig => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: "__DISPATCH:ReloadConfig".into(),
                        });
                    }
                    NlpCommand::StressTest { test_type } => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command_dispatch".into(),
                            message: format!("__DISPATCH:StressTest:{}", test_type),
                        });
                    }
                    // AI-assisted edit: FinancialNlpEngine deterministic first-pass, LLM fallback
                    NlpCommand::AiEdit {
                        instruction,
                        provider,
                    } => {
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Analysing financial intent…".into(),
                            fraction: 0.2,
                        });
                        // Step 1: Try deterministic FinancialNlpEngine
                        let txs: Vec<crate::engine::model::Transaction> = {
                            use crate::engine::offline_parser::parse_statement_offline;
                            parse_statement_offline(&path, engine_ref.clone())
                                .map(|stmt| stmt.transactions)
                                .unwrap_or_default()
                        };
                        use crate::engine::financial_nlp::{
                            apply_financial_intent, parse_financial_intent, FinancialIntent,
                        };
                        let intent = parse_financial_intent(&instruction);
                        if intent != FinancialIntent::Unknown {
                            let result = apply_financial_intent(intent, txs.clone());
                            let _ = res_tx.send(JobResult::Progress {
                                label: format!("Deterministic edit applied: {}", result.summary),
                                fraction: 1.0,
                            });
                            let _ = res_tx
                                .send(JobResult::NaturalLanguageEditReady(result.transactions));
                            return;
                        }
                        // Step 2: LLM fallback for complex or ambiguous intents
                        if provider == "local-llm" {
                            let client = crate::ai::local_llm::LocalLlmClient::new();
                            let _ = res_tx.send(JobResult::Progress {
                                label: format!("Sending to Local LLM ({}) for edit…", client.model),
                                fraction: 0.4,
                            });
                            match client.apply_natural_language_edit(&instruction, &txs).await {
                                Ok(updated) => {
                                    let _ = res_tx.send(JobResult::Progress {
                                        label: "Local AI edit ready — awaiting confirmation".into(),
                                        fraction: 1.0,
                                    });
                                    let _ =
                                        res_tx.send(JobResult::NaturalLanguageEditReady(updated));
                                }
                                Err(e) => {
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "ai_command".into(),
                                        message: format!("Local AI edit failed: {e}"),
                                    });
                                }
                            }
                        } else {
                            let _ = res_tx.send(JobResult::Progress {
                                label: "Processing AI natural language edit…".into(),
                                fraction: 0.4,
                            });
                            let ai_backend =
                                match crate::ai::backend::AiBackend::from_app_config_async(&cfg)
                                    .await
                                {
                                    Ok(c) => c,
                                    Err(e) => {
                                        let _ = res_tx.send(JobResult::Error {
                                        job_label: "ai_command".into(),
                                        message: format!("AI provider unavailable: {e}. Run 'verify-api-keys' to check your keys."),
                                    });
                                        return;
                                    }
                                };
                            match ai_backend
                                .apply_natural_language_edit(&instruction, &txs)
                                .await
                            {
                                Ok(updated) => {
                                    let _ = res_tx.send(JobResult::Progress {
                                        label: "AI edit ready — awaiting confirmation".into(),
                                        fraction: 1.0,
                                    });
                                    let _ =
                                        res_tx.send(JobResult::NaturalLanguageEditReady(updated));
                                }
                                Err(e) => {
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "ai_command".into(),
                                        message: format!("AI edit failed: {e}"),
                                    });
                                }
                            }
                        }
                    }
                    NlpCommand::Unknown { raw, suggestions } => {
                        let sugg_str = if suggestions.is_empty() {
                            String::new()
                        } else {
                            format!(" Suggestions: {}", suggestions.join(", "))
                        };
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "ai_command".into(),
                            message: format!(
                                "Command not recognised: \"{}{}\". Try: undo, balance, verify, extract, \
                                transfer to [bank], shift dates forward N days, or describe an edit.",
                                raw, sugg_str
                            ),
                        });
                    }
                }
            });
        }

        Job::CleanupTempFiles => {
            let res_tx = result_tx_clone.clone();
            tokio::task::spawn_blocking(move || {
                let now = std::time::SystemTime::now();
                let mut removed = 0usize;
                for dir in &["output", "audit"] {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            if let Ok(meta) = entry.metadata() {
                                if let Ok(modified) = meta.modified() {
                                    if let Ok(age) = now.duration_since(modified) {
                                        if age.as_secs() > 86400
                                            && meta.is_file()
                                            && std::fs::remove_file(entry.path()).is_ok()
                                        {
                                            removed += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Completion contract: after the useful work (the sweep) the
                // job must close with exactly one terminal result, so routed
                // `JobTicket` waiters, the `CancellationRegistry` drain, and
                // the GUI/CLI/server consumers all observe the same end of
                // life instead of hanging on a job that finishes silently.
                let _ = res_tx.send(JobResult::completed(
                    "cleanup_temp_files",
                    OperationDisposition::Succeeded,
                    None,
                    format!(
                        "Removed {removed} temporary file(s) older than 24h from output/ and audit/"
                    ),
                ));
            });
        }

        Job::Cancel { id } => {
            let cancelled = cancellations_for_loop.cancel(id);
            if cancelled {
                tracing::info!(job.id = id, "[runtime] cancellation requested");
                let _ = result_tx_clone.send(JobResult::Cancelled { id });
            } else {
                tracing::debug!(job.id = id, "[runtime] cancel for unknown job");
            }
        }

        Job::ReloadConfig => {
            let res_tx = result_tx_clone.clone();
            match config_holder.reload_from_env() {
                Ok(snapshot) => {
                    let new_cfg = snapshot.config();
                    let _ = res_tx.send(JobResult::ConfigReloaded {
                        generation: snapshot.generation(),
                        config: new_cfg.clone(),
                        document_ai_configured: new_cfg.document_ai.is_some(),
                        gemini_configured: new_cfg.gemini_api_key.is_some(),
                        pro_editing_available: new_cfg.pro_editing_available(),
                    });
                }
                Err(e) => {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "reload_config".into(),
                        message: format!("Could not reload configuration: {e}"),
                    });
                }
            }
        }

        Job::ValidateCredentials => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_holder.snapshot().config();

            tokio::spawn(async move {
                let _ = res_tx.send(JobResult::Progress {
                    label: "Validating AI Credentials...".into(),
                    fraction: 0.1,
                });

                let _gemini_res =
                    match crate::ai::backend::AiBackend::from_app_config_async(&cfg).await {
                        Ok(client) => client.ping().await.map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    };

                let _docai_res =
                    match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                        Ok(client) => client.ping().await.map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    };

                // We pass false for json_output because we just want the report returned
                let report = crate::app::api_verification::verify_all_api_keys(&cfg, false).await;
                let disposition = match report.exit_code() {
                    0 => OperationDisposition::Succeeded,
                    1 => OperationDisposition::Partial,
                    _ => OperationDisposition::Failed,
                };
                let summary = format!(
                    "Validated {} provider credential check(s): {:?}",
                    report.results.len(),
                    report.overall_status
                );
                // Useful payload first …
                let _ = res_tx.send(JobResult::ApiKeysVerified(report));

                let _ = res_tx.send(JobResult::Progress {
                    label: "Done".into(),
                    fraction: 1.0,
                });

                // … then exactly one terminal completion (shared contract):
                // routed `JobTicket` waiters and the `CancellationRegistry`
                // only release on this terminal, and every consumer maps the
                // payload-driven disposition the same way.
                let _ = res_tx.send(JobResult::completed(
                    "validate_credentials",
                    disposition,
                    None,
                    summary,
                ));
            });
        }

        _ => unreachable!("unhandled job in this domain handler"),
    }
}
