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
        Job::AiConfirmationResponse(response) => {
            // Log the response as learning data
            tracing::info!(
                "[AI_CONFIRM] User responded to confirmation {}",
                response.id
            );
            // The actual wiring to pause/resume happens via channels in the pipeline.
            // For now, log it to the learning file.
            let placeholder_confirmation = crate::engine::ai_confirm::AiConfirmation {
                id: response.id,
                stage: "user_response".to_string(),
                question: String::new(),
                options: vec![],
                context: String::new(),
                confidence: 0.0,
                default_answer: None,
            };
            let _ = crate::engine::ai_confirm::log_learning_response(
                &placeholder_confirmation,
                &response,
            );
        }

        Job::InteractiveFallbackResponse(response) => {
            let id = response.id;
            let router = fallback_router.clone();
            tokio::spawn(async move {
                let mut map = router.lock().await;
                if let Some(tx) = map.remove(&id) {
                    let _ = tx.send(response.selected_alternative_id);
                }
            });
        }

        Job::WorkflowParseAndValidate {
            input,
            version,
            parser_mode,
            ai_provider,
            ignore_offline_fallback,
        } => {
            let res_tx = TerminalTracker::new(result_tx_clone.clone(), "WorkflowParseAndValidate");
            let mut cfg_override = (*config_for_tokio).clone();
            cfg_override.ai_provider = ai_provider;
            let cfg = std::sync::Arc::new(cfg_override);
            let engine_for_tokio = engine_for_tokio.clone();
            let router = fallback_router.clone();
            tokio::spawn(async move {
                let _ = res_tx.send(JobResult::WorkflowStageChanged {
                    stage: crate::engine::workflow::WorkflowStage::Parsing,
                });

                // ---€ Tier 1: Determine parsing strategy -------------------€
                use crate::app::config::DocumentParserMode;

                let mut current_parser_mode = parser_mode;
                let mut stmt = loop {
                    match current_parser_mode {
                        DocumentParserMode::Reducto => {
                            match crate::ai::reducto::ReductoClient::from_app_config(&cfg) {
                                Ok(client) => match client.parse_statement(&input).await {
                                    Ok(s) => break s,
                                    Err(e) => {
                                        if let Some(next) = interactive_fallback_or_continue!(
                                            cfg,
                                            router,
                                            res_tx,
                                            format!("Reducto parse failed: {e}"),
                                            Some(DocumentParserMode::LlamaParse),
                                            ignore_offline_fallback
                                        ) {
                                            current_parser_mode = next;
                                            continue;
                                        } else {
                                            let _ = res_tx.send(JobResult::WorkflowFailed(crate::engine::workflow::WorkflowFailure::FidelityCheckFailed(format!("Reducto error: {e}"))));
                                            return;
                                        }
                                    }
                                },
                                Err(_) => {
                                    if let Some(next) = interactive_fallback_or_continue!(
                                        cfg,
                                        router,
                                        res_tx,
                                        "Reducto client init failed".to_string(),
                                        Some(DocumentParserMode::LlamaParse),
                                        ignore_offline_fallback
                                    ) {
                                        current_parser_mode = next;
                                        continue;
                                    } else {
                                        let _ = res_tx.send(JobResult::WorkflowFailed(crate::engine::workflow::WorkflowFailure::FidelityCheckFailed("Reducto client init failed".to_string())));
                                        return;
                                    }
                                }
                            }
                        }
                        DocumentParserMode::DocumentAi => {
                            let _ = res_tx.send(JobResult::Progress {
                                label: "Parsing with Document AI".into(),
                                fraction: 0.2,
                            });
                            match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                                Ok(client) => {
                                    let doc_ai: std::sync::Arc<
                                        crate::ai::document_ai::DocumentAiClient,
                                    > = std::sync::Arc::new(client);
                                    let page_count = {
                                        let p = input.clone();
                                        tokio::task::spawn_blocking(move || -> usize {
                                                        use pdfium_render::prelude::Pdfium;
                                                        let lib_dir = crate::pdf::native_engine::pdfium_resolver::resolve().unwrap_or_default();
                                                        let bindings = if lib_dir.as_os_str().is_empty() {
                                                            Pdfium::bind_to_system_library()
                                                        } else {
                                                            let lib_path = Pdfium::pdfium_platform_library_name_at_path(lib_dir.to_string_lossy().as_ref());
                                                            Pdfium::bind_to_library(lib_path).or_else(|_| Pdfium::bind_to_system_library())
                                                        };
                                                        match bindings { Ok(b) => Pdfium::new(b).load_pdf_from_file(&p, None).map(|d| d.pages().len() as usize).unwrap_or(0), Err(_) => 0 }
                                                    }).await.unwrap_or(0)
                                    };
                                    let final_version = version.clone().unwrap_or_else(|| {
                                        cfg.document_ai
                                            .as_ref()
                                            .map(|d| d.effective_default_version().to_string())
                                            .unwrap_or_else(|| {
                                                crate::app::config::DEFAULT_DOCAI_PROCESSOR_VERSION
                                                    .to_string()
                                            })
                                    });
                                    match doc_ai
                                        .parse_smart_batch(&input, Some(&final_version), page_count)
                                        .await
                                    {
                                        Ok(s) => {
                                            let mut retail_sum = s.opening_balance;
                                            let mut formal_sum = s.opening_balance;
                                            for tx in &s.transactions {
                                                retail_sum +=
                                                    tx.debit.unwrap_or(rust_decimal::Decimal::ZERO)
                                                        - tx.credit
                                                            .unwrap_or(rust_decimal::Decimal::ZERO);
                                                formal_sum += tx
                                                    .credit
                                                    .unwrap_or(rust_decimal::Decimal::ZERO)
                                                    - tx.debit
                                                        .unwrap_or(rust_decimal::Decimal::ZERO);
                                            }
                                            let expected = s.closing_balance;
                                            let retail_diff = (retail_sum - expected).abs();
                                            let formal_diff = (formal_sum - expected).abs();
                                            let one_cent = rust_decimal_macros::dec!(0.01);
                                            if !s.transactions.is_empty()
                                                && s.opening_balance != rust_decimal::Decimal::ZERO
                                                && retail_diff > one_cent
                                                && formal_diff > one_cent
                                            {
                                                if let Some(next) = interactive_fallback_or_continue!(
                                                    cfg,
                                                    router,
                                                    res_tx,
                                                    "AI Fidelity Math Check Failed",
                                                    Some(DocumentParserMode::LlamaParse),
                                                    ignore_offline_fallback
                                                ) {
                                                    current_parser_mode = next;
                                                    continue;
                                                } else {
                                                    let _ = res_tx.send(JobResult::WorkflowFailed(crate::engine::workflow::WorkflowFailure::FidelityCheckFailed("Math check failed".into())));
                                                    return;
                                                }
                                            }
                                            break s;
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "[workflow] Document AI parse failed: {e}"
                                            );
                                            if let Some(next) = interactive_fallback_or_continue!(
                                                cfg,
                                                router,
                                                res_tx,
                                                format!("Document AI parse failed: {e}"),
                                                Some(DocumentParserMode::LlamaParse),
                                                ignore_offline_fallback
                                            ) {
                                                current_parser_mode = next;
                                                continue;
                                            } else {
                                                let _ = res_tx.send(JobResult::WorkflowFailed(crate::engine::workflow::WorkflowFailure::ParseFailed("Cancelled".into())));
                                                return;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("[workflow] Document AI not configured: {e}");
                                    if let Some(next) = interactive_fallback_or_continue!(
                                        cfg,
                                        router,
                                        res_tx,
                                        format!("Document AI not configured: {e}"),
                                        Some(DocumentParserMode::LlamaParse),
                                        ignore_offline_fallback
                                    ) {
                                        current_parser_mode = next;
                                        continue;
                                    } else {
                                        let _ = res_tx.send(JobResult::WorkflowFailed(
                                            crate::engine::workflow::WorkflowFailure::ParseFailed(
                                                "Cancelled".into(),
                                            ),
                                        ));
                                        return;
                                    }
                                }
                            }
                        }

                        DocumentParserMode::LlamaParse => {
                            let _ = res_tx.send(JobResult::Progress {
                                label: "Parsing with LlamaParse...".into(),
                                fraction: 0.2,
                            });
                            match crate::ai::llamaparse::LlamaParseClient::from_app_config(&cfg) {
                                Ok(client) => match client.parse_statement(&input).await {
                                    Ok(s) => break s,
                                    Err(e) => {
                                        tracing::warn!("[workflow] LlamaParse parse failed: {e}");
                                        if let Some(next) = interactive_fallback_or_continue!(
                                            cfg,
                                            router,
                                            res_tx,
                                            format!("LlamaParse parse failed: {e}"),
                                            Some(DocumentParserMode::DocumentAi),
                                            ignore_offline_fallback
                                        ) {
                                            current_parser_mode = next;
                                            continue;
                                        } else {
                                            let _ = res_tx.send(JobResult::WorkflowFailed(crate::engine::workflow::WorkflowFailure::ParseFailed("Cancelled".into())));
                                            return;
                                        }
                                    }
                                },
                                Err(e) => {
                                    tracing::warn!("[workflow] LlamaParse not configured: {e}");
                                    if let Some(next) = interactive_fallback_or_continue!(
                                        cfg,
                                        router,
                                        res_tx,
                                        format!("LlamaParse not configured: {e}"),
                                        Some(DocumentParserMode::DocumentAi),
                                        ignore_offline_fallback
                                    ) {
                                        current_parser_mode = next;
                                        continue;
                                    } else {
                                        let _ = res_tx.send(JobResult::WorkflowFailed(
                                            crate::engine::workflow::WorkflowFailure::ParseFailed(
                                                "Cancelled".into(),
                                            ),
                                        ));
                                        return;
                                    }
                                }
                            }
                        }
                        DocumentParserMode::OfflineHeuristic => {
                            let _ = res_tx.send(JobResult::Progress {
                                label: "Parsing with Offline Parser...".into(),
                                fraction: 0.35,
                            });
                            let eng = engine_for_tokio.clone();
                            let path = input.clone();
                            match tokio::task::spawn_blocking(move || {
                                crate::engine::offline_parser::parse_statement_offline(&path, eng)
                            })
                            .await
                            {
                                Ok(Ok(s)) => break s,
                                Ok(Err(e)) => {
                                    tracing::warn!("[workflow] Offline parser failed: {e}");
                                    if let Some(next) = interactive_fallback_or_continue!(
                                        cfg,
                                        router,
                                        res_tx,
                                        format!("Offline parser failed: {e}"),
                                        None::<DocumentParserMode>,
                                        ignore_offline_fallback
                                    ) {
                                        current_parser_mode = next;
                                        continue;
                                    } else {
                                        let _ = res_tx.send(JobResult::WorkflowFailed(
                                            crate::engine::workflow::WorkflowFailure::ParseFailed(
                                                e,
                                            ),
                                        ));
                                        return;
                                    }
                                }
                                Err(e) => {
                                    let e_str = e.to_string();
                                    tracing::warn!("[workflow] Offline parser panicked: {e}");
                                    if let Some(next) = interactive_fallback_or_continue!(
                                        cfg,
                                        router,
                                        res_tx,
                                        format!("Offline parser panicked: {e}"),
                                        None::<DocumentParserMode>,
                                        ignore_offline_fallback
                                    ) {
                                        current_parser_mode = next;
                                        continue;
                                    } else {
                                        let _ = res_tx.send(JobResult::WorkflowFailed(
                                            crate::engine::workflow::WorkflowFailure::ParseFailed(
                                                e_str,
                                            ),
                                        ));
                                        return;
                                    }
                                }
                            }
                        }
                        DocumentParserMode::LocalOcrs => {
                            let _ = res_tx.send(JobResult::WorkflowFailed(
                                crate::engine::workflow::WorkflowFailure::ParseFailed(
                                    "Local OCR PDF parsing is not supported in v1. Use Offline Heuristic for text-layer PDFs; scanned-PDF OCR remains disabled until its model and page-geometry contract is qualified."
                                        .into(),
                                ),
                            ));
                            return;
                        }
                    }
                };
                stmt.ensure_canonical_metadata();

                use crate::app::config::AiProviderMode;

                let deterministic_issues = crate::engine::workflow::deterministic_parse_issues(
                    stmt.total_pages,
                    &stmt.transactions,
                    stmt.opening_balance,
                    stmt.closing_balance,
                );
                let deterministic_score = if deterministic_issues.is_empty() {
                    1.0
                } else {
                    0.0
                };

                let (score, notes, mut missing, _math_ok) = match ai_provider {
                    AiProviderMode::ManualOnly => {
                        let _ = res_tx.send(JobResult::Progress {
                            label: "AI validation skipped (Manual Only mode)".into(),
                            fraction: 0.7,
                        });
                        (
                            deterministic_score,
                            "Optional AI validation skipped (Manual Only mode).".into(),
                            vec![],
                            false,
                        )
                    }
                    _ => {
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Asking Gemini to validate completeness".into(),
                            fraction: 0.7,
                        });

                        let gemini_init_and_validate = async {
                            let g =
                                crate::ai::backend::AiBackend::from_app_config_async(&cfg).await?;
                            g.validate_parse_completeness(
                                &stmt.transactions,
                                crate::engine::model::dec_to_f64(stmt.opening_balance),
                                crate::engine::model::dec_to_f64(stmt.closing_balance),
                                stmt.total_pages,
                            )
                            .await
                        };

                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            gemini_init_and_validate,
                        )
                        .await
                        {
                            Ok(Ok(r)) => (
                                r.completeness_score.min(deterministic_score),
                                r.notes,
                                r.missing_rows,
                                r.math_consistent,
                            ),
                            Ok(Err(e)) => {
                                tracing::warn!(
                                    "[workflow] Gemini validation failed: {e}; continuing"
                                );
                                let _ = res_tx.send(JobResult::Progress {
                                    label: format!("AI validation skipped: {e}"),
                                    fraction: 0.7,
                                });
                                (
                                    deterministic_score,
                                    format!("Optional AI validation skipped: {e}"),
                                    vec![],
                                    false,
                                )
                            }
                            Err(_elapsed) => {
                                tracing::warn!("[workflow] Gemini validation timed out after 30s; continuing without AI validation");
                                let _ = res_tx.send(JobResult::Progress {
                                    label: "AI validation timed out after 30s".into(),
                                    fraction: 0.7,
                                });
                                (
                                    deterministic_score,
                                    "Optional AI validation timed out; deterministic validation used."
                                        .into(),
                                    vec![],
                                    false,
                                )
                            }
                        }
                    }
                };

                missing.extend(deterministic_issues);
                let validation = crate::engine::workflow::ParseValidation {
                    total_pages: stmt.total_pages,
                    transactions_found: stmt.transactions.len(),
                    opening_balance: stmt.opening_balance,
                    closing_balance: stmt.closing_balance,
                    account_number: stmt.account_number.clone(),
                    completeness_score: score,
                    completeness_notes: notes,
                    missing_rows: missing,
                };

                // Cross-check against the deterministic template extractor
                let template_row_count = {
                    let eng = engine_for_tokio.clone();
                    let path = input.clone();
                    let templates_dir = crate::app::paths::resolve_asset_path("bank_templates");
                    tokio::task::spawn_blocking(move || {
                        let provider = crate::extractors::BankTemplateProvider::new(
                            templates_dir.as_path(),
                            eng,
                        );
                        use crate::extractors::GeometryProvider;
                        provider
                            .extract_line_geometry(&path)
                            .map(|g| g.len())
                            .unwrap_or(0)
                    })
                    .await
                    .unwrap_or(0)
                };
                let validation = crate::engine::workflow::cross_validate_with_template(
                    validation,
                    template_row_count,
                );

                let txs = stmt.transactions.clone();
                let _ = res_tx.send(JobResult::WorkflowParseValidated {
                    validation: validation.clone(),
                    transactions: txs,
                });
                if !validation.is_acceptable() {
                    let _ = res_tx.send(JobResult::WorkflowFailed(
                        crate::engine::workflow::WorkflowFailure::Incomplete {
                            score: validation.completeness_score,
                            notes: if validation.missing_rows.is_empty() {
                                validation.completeness_notes.clone()
                            } else {
                                format!(
                                    "{} {}",
                                    validation.completeness_notes,
                                    validation.missing_rows.join("; ")
                                )
                            },
                        },
                    ));
                    return;
                }
                let _ = res_tx.send(JobResult::WorkflowStageChanged {
                    stage: crate::engine::workflow::WorkflowStage::Editing(validation),
                });
                let _ = res_tx.send(JobResult::completed(
                    "workflow_parse_and_validate",
                    OperationDisposition::Succeeded,
                    None,
                    "Statement parsing and completeness validation completed",
                ));
            });
        }

        Job::WorkflowPreview {
            original_transactions,
            edits,
            opening_balance,
            expected_closing,
        } => {
            let res_tx = result_tx_clone.clone();
            tokio::task::spawn_blocking(move || {
                match crate::engine::workflow::build_preview(
                    &original_transactions,
                    &edits,
                    opening_balance,
                    expected_closing,
                ) {
                    Ok(p) => {
                        let _ = res_tx.send(JobResult::WorkflowPreviewBuilt(p.clone()));
                        let _ = res_tx.send(JobResult::WorkflowStageChanged {
                            stage: crate::engine::workflow::WorkflowStage::Previewing(p),
                        });
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(format!(
                                "preview build failed: {e}"
                            )),
                        ));
                    }
                }
            });
        }

        Job::WorkflowConfirmAndRender {
            input,
            output,
            edits,
            deep_font_replication,
            max_visual_attempts: _,
            visual_threshold: _,
            original_transactions,
            opening_balance,
            expected_closing,
            ignore_font_coverage,
            ignore_visual_fidelity,
        } => {
            let res_tx = TerminalTracker::new(result_tx_clone.clone(), "WorkflowConfirmAndRender");
            let eng = engine_for_tokio.clone();
            let py_tx = python_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            let map_opt = segment_map.clone();
            let mgr_opt = segment_manager
                .as_ref()
                .map(|m| m.temp_path().to_path_buf());

            tokio::spawn(async move {
                // Spec §6.2: ignore_visual_fidelity is an explicit, audit-logged override for downstream visual gates.
                // Unlike ignore_font_coverage (which hard-fails on unresolved glyph coverage to prevent silent font corruption),
                // ignore_visual_fidelity permits publishing output when visual verification gates fail or are unavailable,
                // while recording an audit log entry and ensuring the evidence package disposition remains unverified (Failed).
                if original_transactions.is_empty() {
                    let _ = res_tx.send(JobResult::WorkflowFailed(
                        crate::engine::workflow::WorkflowFailure::Other(
                            "confirm-and-render requires parsed transactions for deterministic math validation"
                                .into(),
                        ),
                    ));
                    return;
                }
                let expected_closing = match expected_closing.or_else(|| {
                    original_transactions
                        .last()
                        .and_then(|transaction| transaction.running_balance)
                }) {
                    Some(balance) => balance.round_dp(2),
                    None => {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(
                                "confirm-and-render requires a verified closing balance".into(),
                            ),
                        ));
                        return;
                    }
                };
                let pre_render_preview = match crate::engine::workflow::build_preview(
                    &original_transactions,
                    &edits,
                    opening_balance,
                    Some(expected_closing),
                ) {
                    Ok(preview) => preview,
                    Err(error) => {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(format!(
                                "pre-render math validation failed: {error}"
                            )),
                        ));
                        return;
                    }
                };
                if !pre_render_preview.balanced {
                    let _ = res_tx.send(JobResult::WorkflowFailed(
                        crate::engine::workflow::WorkflowFailure::FinalMathInvalid {
                            imbalance: pre_render_preview.final_imbalance,
                        },
                    ));
                    return;
                }
                let edits = match crate::engine::workflow::materialize_preview_edits(
                    &original_transactions,
                    &edits,
                    &pre_render_preview,
                ) {
                    Ok(materialized) => materialized,
                    Err(error) => {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(format!(
                                "preview/render edit-set mismatch: {error}"
                            )),
                        ));
                        return;
                    }
                };
                if edits.is_empty() {
                    let _ = res_tx.send(JobResult::WorkflowFailed(
                        crate::engine::workflow::WorkflowFailure::Other(
                            "confirm-and-render requires at least one exact edit".into(),
                        ),
                    ));
                    return;
                }

                let post_edit_transactions: Vec<_> = original_transactions
                    .iter()
                    .zip(&pre_render_preview.rows)
                    .map(|(original, preview)| {
                        let mut transaction = original.clone();
                        transaction.date = preview.date.clone();
                        transaction.raw_text = preview.description.clone();
                        transaction.debit = preview.debit;
                        transaction.credit = preview.credit;
                        transaction.running_balance = preview.new_running_balance;
                        transaction
                    })
                    .collect();
                let requested_output = output.clone();
                let output_parent = requested_output
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let staged_workflow_output = match crate::app::commit::staging_path(
                    output_parent,
                    ".dcpp-workflow-",
                    ".pdf",
                ) {
                    Ok(path) => path,
                    Err(error) => {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(format!(
                                "workflow output staging failed: {error}"
                            )),
                        ));
                        return;
                    }
                };
                let output = staged_workflow_output.to_path_buf();
                let attempt: u32 = 1;
                let mut visual_attempts: u32 = 0;
                // Stage 13 / Item #5: per-workflow timestamp so
                // scratch files from different runs don't
                // collide. We append both the timestamp and
                // the attempt number to the scratch filename.
                let workflow_stamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
                let mut last_score: f64 = 1.0;
                let mut last_intended = false;
                let verified_reparsed_count: usize;
                let verified_final_imbalance: rust_decimal::Decimal;
                let _ = (&last_score, &last_intended); // initial values used below the loop on early exit
                let intended_edits: Vec<crate::engine::verification::VerificationIntent> = edits
                    .iter()
                    .map(|edit| crate::engine::verification::VerificationIntent {
                        page: edit.page,
                        bbox: edit.bbox,
                        old_text: edit.old_text.clone(),
                        new_text: edit.new_text.clone(),
                    })
                    .collect();

                {
                    let _ = res_tx.send(JobResult::WorkflowStageChanged {
                        stage: crate::engine::workflow::WorkflowStage::Rendering { attempt },
                    });
                    let _ = res_tx.send(JobResult::Progress {
                        label: "Rendering verified output".into(),
                        fraction: 0.1 + (attempt as f32) * 0.05,
                    });

                    // Stage 3 / Item #14: apply all edits in a single
                    // open/save pass. Much faster than the previous
                    // N-roundtrip serial loop. We still pre-flight the
                    // row-drift guard from Stage 2 / Item #1 once per
                    // edit before sending the batch.
                    let mut all_ok = true;
                    let mut last_failure: Option<crate::engine::workflow::WorkflowFailure> = None;

                    // Automatic deep-font generation is not an approved fidelity
                    // operation. Retain the request field for compatibility but
                    // reject it before creating or mutating any output.
                    let font_path: Option<PathBuf> = None;
                    if deep_font_replication {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(
                                "Automatic glyph synthesis and donor-font substitution are disabled. Use replacement text covered by the original font or a separately reviewed coverage-complete supplied font."
                                    .into(),
                            ),
                        ));
                        return;
                    }

                    // --- Pre-flight Font Coverage Check ---
                    // A supplied or replicated font must be parseable and cover every
                    // replacement glyph. The legacy override is retained only for
                    // wire compatibility; it now converts unresolved coverage into an
                    // explicit terminal failure rather than undisclosed substitution.
                    if let Some(ref fp) = font_path {
                        let bytes = match std::fs::read(fp) {
                            Ok(bytes) => bytes,
                            Err(error) => {
                                let _ = res_tx.send(JobResult::WorkflowFailed(
                                    crate::engine::workflow::WorkflowFailure::Other(format!(
                                        "Could not read supplied font {}: {error}",
                                        fp.display()
                                    )),
                                ));
                                return;
                            }
                        };
                        let all_new_text = edits
                            .iter()
                            .map(|edit| edit.new_text.as_str())
                            .collect::<String>();
                        let missing = match crate::engine::font_replication::check_glyph_coverage(
                            &bytes,
                            &all_new_text,
                        ) {
                            Ok((_, missing)) => missing,
                            Err(error) => {
                                let _ = res_tx.send(JobResult::WorkflowFailed(
                                    crate::engine::workflow::WorkflowFailure::Other(format!(
                                        "Could not validate supplied font coverage: {error}"
                                    )),
                                ));
                                return;
                            }
                        };
                        if !missing.is_empty() {
                            tracing::warn!(
                                "[font_coverage] Missing characters detected: {:?}",
                                missing
                            );
                            // Note: ignore_font_coverage converts unresolved glyph coverage into an explicit terminal failure
                            // rather than undisclosed substitution. This differs from ignore_visual_fidelity, which is an audit-logged
                            // override for post-render visual verification gates with unverified evidence marking.
                            if ignore_font_coverage {
                                let _ = res_tx.send(JobResult::WorkflowFailed(
                                    crate::engine::workflow::WorkflowFailure::FontCoverageFailed {
                                        missing_chars: missing
                                            .iter()
                                            .map(char::to_string)
                                            .collect(),
                                    },
                                ));
                            } else {
                                let _ = res_tx.send(JobResult::WorkflowStageChanged {
                                    stage: crate::engine::workflow::WorkflowStage::FontCoverageWarning {
                                        missing_chars: missing,
                                    },
                                });
                            }
                            return;
                        }
                    }

                    // Stable-target guard (pre-flight).
                    //
                    // Every edit must resolve to exactly one source span by BOTH
                    // normalized old-text identity and >=50% canonical-rectangle
                    // overlap. Image-only pages, coordinate drift, zero matches,
                    // and duplicate matches are unsupported automatic-edit cases;
                    // they must stop before any scratch output is created.
                    {
                        let eng_for_guard = eng.clone();
                        let input_for_guard = input.clone();
                        let edits_for_guard = edits.clone();
                        let map_for_guard = map_opt.clone();

                        let target_issues = tokio::task::spawn_blocking(move || {
                            let mut issues = Vec::new();
                            for (index, edit) in edits_for_guard.iter().enumerate() {
                                let (check_path, check_page) = if let Some(ref map) = map_for_guard {
                                    map.resolve(edit.page)
                                        .map(|(segment_index, local_page)| {
                                            (map.segments[segment_index].path.clone(), local_page)
                                        })
                                        .unwrap_or((input_for_guard.clone(), edit.page))
                                } else {
                                    (input_for_guard.clone(), edit.page)
                                };
                                let blocks = match eng_for_guard
                                    .get_text_blocks(&check_path, check_page)
                                {
                                    Ok(blocks) => blocks,
                                    Err(error) => {
                                        issues.push(format!(
                                            "edit {index} page {}: text extraction failed: {error}",
                                            edit.page
                                        ));
                                        continue;
                                    }
                                };
                                let expected_identity = edit
                                    .old_text
                                    .split_whitespace()
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                let mut identity_overlaps = blocks
                                    .iter()
                                    .filter(|block| block.page == check_page)
                                    .filter_map(|block| {
                                        let observed_identity = block
                                            .text
                                            .split_whitespace()
                                            .collect::<Vec<_>>()
                                            .join(" ");
                                        (observed_identity == expected_identity).then(|| {
                                            (
                                                block,
                                                crate::pdf::bbox_overlap_fraction(
                                                    edit.bbox,
                                                    block.bbox,
                                                ),
                                            )
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                identity_overlaps.sort_by(|left, right| {
                                    right
                                        .1
                                        .partial_cmp(&left.1)
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                });
                                let exact_matches = identity_overlaps
                                    .iter()
                                    .filter(|(_, overlap)| *overlap >= 0.5)
                                    .count();
                                if exact_matches != 1 {
                                    let best_overlap = identity_overlaps
                                        .first()
                                        .map(|(_, overlap)| *overlap)
                                        .unwrap_or(0.0);
                                    issues.push(format!(
                                        "edit {index} page {}: expected exactly one stable target for {:?}, found {exact_matches} (best overlap {:.1}%)",
                                        edit.page,
                                        edit.old_text,
                                        best_overlap * 100.0
                                    ));
                                }
                            }
                            issues
                        })
                        .await
                        .unwrap_or_else(|error| {
                            vec![format!("stable-target preflight panicked: {error}")]
                        });

                        if !target_issues.is_empty() {
                            let detail = target_issues
                                .iter()
                                .take(10)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("; ");
                            let _ = res_tx.send(JobResult::WorkflowFailed(
                                crate::engine::workflow::WorkflowFailure::Other(format!(
                                    "Stable target validation failed before PDF mutation: {detail}"
                                )),
                            ));
                            return;
                        }
                    }

                    // Build the batch JSON. Stage 8 / Item #12:
                    // for numeric fields, reformat the user's
                    // typed value to match the original cell's
                    // format pattern (currency symbol, thousand
                    // separators, decimal separator, negative
                    // style). Date / Description fields go
                    // through unchanged.
                    use crate::engine::number_format::format_like;
                    use crate::engine::workflow::EditField;
                    use rust_decimal::Decimal;
                    use std::str::FromStr;
                    let edits_json = match serde_json::to_string(
                        &edits
                            .iter()
                            .map(|e| {
                                let formatted = match e.field {
                                    EditField::Debit
                                    | EditField::Credit
                                    | EditField::RunningBalance => {
                                        // Parse the typed value (loose: strip non-digit/sign/dot).
                                        let cleaned: String = e
                                            .new_text
                                            .chars()
                                            .filter(|c| {
                                                c.is_ascii_digit() || *c == '-' || *c == '.'
                                            })
                                            .collect();
                                        match Decimal::from_str(&cleaned) {
                                            Ok(v) => format_like(v, &e.old_text),
                                            Err(_) => e.new_text.clone(),
                                        }
                                    }
                                    _ => e.new_text.clone(),
                                };
                                serde_json::json!({
                                    "page": e.page,
                                    "rect": e.bbox,
                                    "old_text": e.old_text,
                                    "new_text": formatted,
                                })
                            })
                            .collect::<Vec<_>>(),
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = res_tx.send(JobResult::WorkflowFailed(
                                crate::engine::workflow::WorkflowFailure::Other(format!(
                                    "edits serialize failed: {e}"
                                )),
                            ));
                            return;
                        }
                    };

                    let scratch =
                        output.with_extension(format!("{workflow_stamp}.attempt{attempt}.pdf"));
                    if let Some(parent) = scratch.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    // Stage 13 / Item #5: defensively clear a
                    // stale scratch file from any previous run
                    // before we hand off to the editor. On
                    // Windows the file may be locked by an
                    // open PDF viewer; if that happens we
                    // surface a clean error rather than letting
                    // PyMuPDF write a corrupted output.
                    if scratch.exists() {
                        if let Err(e) = std::fs::remove_file(&scratch) {
                            let _ = res_tx.send(JobResult::WorkflowFailed(
                                crate::engine::workflow::WorkflowFailure::Other(format!(
                                    "scratch file {} is locked: {e}",
                                    scratch.display()
                                )),
                            ));
                            return;
                        }
                    }

                    // Stage 14a / Item #20: idempotent re-apply.
                    // Hash (input_pdf_sha256 || edit_set) and
                    // skip the apply when an identical run
                    // already produced an output we can reuse.
                    let edit_hash = {
                        let pdf_hash = std::fs::read(&input)
                            .ok()
                            .map(|b| crate::engine::workflow::sha256_hex_of(&b))
                            .unwrap_or_default();
                        crate::engine::workflow::edit_set_hash(&pdf_hash, &edits)
                    };
                    let cached_output = std::path::PathBuf::from("audit")
                        .join("apply_cache")
                        .join(format!("{edit_hash}.pdf"));

                    let apply_result: Result<
                        PythonJobResult,
                        tokio::sync::oneshot::error::RecvError,
                    >;

                    if cfg.engine_mode == crate::app::config::PdfEngineMode::TypstReconstruct {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(
                                "Typst reconstruction is a non-fidelity export and cannot be used to finalize an edit-in-place workflow. Select PyMuPDF Pro Primary, PyMuPDF Only, Native Only, or Dual Concurrent."
                                    .into(),
                            ),
                        ));
                        return;
                    } else if let Some(ref map) = map_opt {
                        // 3-page mode: segmented batch apply.
                        // Caching is bypassed in this mode for simplicity.
                        let mut final_paths = Vec::new();
                        let mut ok = true;
                        let mut error_msg = String::new();
                        let mut segment_applied = 0usize;

                        let global_edits: Vec<GlobalEdit> = edits
                            .iter()
                            .map(|e| GlobalEdit {
                                page: e.page,
                                bbox: e.bbox,
                                old_text: e.old_text.clone(),
                                new_text: e.new_text.clone(),
                                description: format!("Workflow Edit ({:?})", e.field),
                                deep_font_replication: false,
                            })
                            .collect();

                        // Invalid maps and out-of-range edits abort before any
                        // engine call, leaving all source segments unchanged.
                        if let Err(error) = map.validate_structure() {
                            ok = false;
                            error_msg = format!("Invalid segment map: {error}");
                        }
                        let grouped = if ok {
                            match map.group_edits_by_segment(&global_edits) {
                                Ok(groups) => groups,
                                Err(error) => {
                                    ok = false;
                                    error_msg = error.to_string();
                                    std::collections::BTreeMap::new()
                                }
                            }
                        } else {
                            std::collections::BTreeMap::new()
                        };

                        for (i, seg) in map.segments.iter().enumerate() {
                            if !ok {
                                break;
                            }
                            let segment_edits = grouped.get(&i).cloned().unwrap_or_default();
                            if !segment_edits.is_empty() {
                                #[allow(clippy::expect_used)]
                                let temp_seg_out = mgr_opt
                                    .as_ref()
                                    .expect("segment manager initialized when map exists")
                                    .join(format!(
                                        "seg_{}_batch_{}_{}.pdf",
                                        i,
                                        workflow_stamp,
                                        Uuid::new_v4()
                                    ));

                                use crate::engine::number_format::format_like;
                                use rust_decimal::Decimal;
                                use std::str::FromStr;

                                let edits_json = serde_json::to_string(
                                    &segment_edits
                                        .iter()
                                        .map(|e| {
                                            let formatted = if e
                                                .old_text
                                                .chars()
                                                .any(|c| c == '$' || c == ',' || c == '.')
                                            {
                                                let cleaned: String = e
                                                    .new_text
                                                    .chars()
                                                    .filter(|c| {
                                                        c.is_ascii_digit() || *c == '-' || *c == '.'
                                                    })
                                                    .collect();
                                                Decimal::from_str(&cleaned)
                                                    .map(|v| format_like(v, &e.old_text))
                                                    .unwrap_or_else(|_| e.new_text.clone())
                                            } else {
                                                e.new_text.clone()
                                            };
                                            serde_json::json!({
                                                "page": e.local_page,
                                                "rect": e.bbox,
                                                "old_text": e.old_text,
                                                "new_text": formatted,
                                            })
                                        })
                                        .collect::<Vec<_>>(),
                                )
                                .unwrap_or_default();

                                let (tx, rx) = oneshot::channel();
                                let _ = py_tx.send((
                                    PythonJob::ApplyManyEdits {
                                        pdf_path: seg.path.to_string_lossy().to_string(),
                                        output_path: temp_seg_out.to_string_lossy().to_string(),
                                        edits_json,
                                        font_path: font_path
                                            .as_ref()
                                            .map(|p| p.to_string_lossy().to_string()),
                                        strict_fidelity: false,
                                    },
                                    tx,
                                ));

                                let expected = segment_edits.len();
                                match rx.await {
                                    Ok(PythonJobResult::ApplyReport(report))
                                        if report.success
                                            && report.requested == expected
                                            && report.matched == expected
                                            && report.placed == expected
                                            && report.failed == 0
                                            && report.review_flags.is_empty()
                                            && temp_seg_out.is_file() =>
                                    {
                                        match crate::engine::segments::validate_segment_replacement(
                                            &seg.path,
                                            &temp_seg_out,
                                            seg.page_count,
                                        ) {
                                            Ok(()) => {
                                                segment_applied += report.placed;
                                                final_paths.push(temp_seg_out);
                                            }
                                            Err(validation_error) => {
                                                let _ = std::fs::remove_file(&temp_seg_out);
                                                ok = false;
                                                error_msg = format!(
                                                    "segment {i} output failed page membership validation: {validation_error}"
                                                );
                                                break;
                                            }
                                        }
                                    }
                                    Ok(PythonJobResult::ApplyReport(report)) => {
                                        let _ = std::fs::remove_file(&temp_seg_out);
                                        ok = false;
                                        error_msg = format!(
                                            "segment {i} exact apply failed: requested {}, matched {}, placed {}, failed {}, expected {}: {}",
                                            report.requested,
                                            report.matched,
                                            report.placed,
                                            report.failed,
                                            expected,
                                            report.warnings.join("; ")
                                        );
                                        break;
                                    }
                                    Ok(PythonJobResult::Error(error)) => {
                                        ok = false;
                                        error_msg = error;
                                        break;
                                    }
                                    other => {
                                        ok = false;
                                        error_msg = format!(
                                            "Python actor returned unexpected segment result: {other:?}"
                                        );
                                        break;
                                    }
                                }
                            } else {
                                final_paths.push(seg.path.clone());
                            }
                        }

                        if ok && segment_applied == edits.len() {
                            let expected_pages = map.total_pages;
                            match crate::engine::pdf_split_merge::merge_pdfs(&final_paths, &scratch)
                            {
                                Ok(merged_pages) if merged_pages == expected_pages => {
                                    apply_result = Ok(PythonJobResult::Success);
                                }
                                Ok(merged_pages) => {
                                    apply_result = Ok(PythonJobResult::Error(format!(
                                        "Merge produced {merged_pages}/{expected_pages} pages"
                                    )));
                                }
                                Err(error) => {
                                    apply_result = Ok(PythonJobResult::Error(format!(
                                        "Merge failed: {error}"
                                    )));
                                }
                            }
                        } else {
                            if ok {
                                error_msg = format!(
                                    "Segmented apply placed {segment_applied}/{} edits",
                                    edits.len()
                                );
                            }
                            apply_result = Ok(PythonJobResult::Error(error_msg));
                        }
                    } else {
                        let (tx, rx) = oneshot::channel();
                        let _ = py_tx.send((
                            PythonJob::ApplyManyEdits {
                                pdf_path: input.to_string_lossy().to_string(),
                                output_path: scratch.to_string_lossy().to_string(),
                                edits_json: edits_json.clone(),
                                font_path: font_path
                                    .as_ref()
                                    .map(|p| p.to_string_lossy().to_string()),
                                strict_fidelity: false,
                            },
                            tx,
                        ));

                        apply_result = rx.await;
                        // Cache only an exact, hash-verified Python output.
                        if matches!(
                            &apply_result,
                            Ok(PythonJobResult::ApplyReport(report)) if report.success
                        ) {
                            if let Some(parent) = cached_output.parent() {
                                if let Err(error) = std::fs::create_dir_all(parent) {
                                    tracing::warn!(
                                        %error,
                                        "[workflow] exact output succeeded but cache directory creation failed"
                                    );
                                }
                            }
                            if let Err(error) = std::fs::copy(&scratch, &cached_output) {
                                tracing::warn!(
                                    %error,
                                    "[workflow] exact output succeeded but cache write failed"
                                );
                            }
                        }
                    }

                    // Missing glyphs are an explicit unsupported fidelity case.
                    // Automatic composite, donor-font, or AI-selected glyph
                    // construction is intentionally forbidden because it changes
                    // the typeface without a separately reviewed substitution
                    // workflow. Preserve the original apply error below.
                    if let Ok(PythonJobResult::Error(ref message)) = apply_result {
                        if message.contains("FONT_COVERAGE_INSUFFICIENT")
                            || message.contains("FONT_EMBEDDING_UNAVAILABLE")
                        {
                            tracing::warn!(
                                "[workflow] exact font fidelity unavailable; output will not be published"
                            );
                        }
                    }

                    match apply_result {
                        Ok(PythonJobResult::ApplyReport(report)) if report.success => {
                            if let Err(error) =
                                crate::app::audit::snapshot_link_or_copy(&scratch, &output)
                            {
                                all_ok = false;
                                last_failure =
                                    Some(crate::engine::workflow::WorkflowFailure::Other(format!(
                                        "exact output publication failed: {error}"
                                    )));
                            }
                        }
                        Ok(PythonJobResult::ApplyReport(report)) => {
                            all_ok = false;
                            last_failure =
                                Some(crate::engine::workflow::WorkflowFailure::Other(format!(
                                    "exact apply failed: placed {}/{}; {}",
                                    report.placed,
                                    report.requested,
                                    report.warnings.join("; ")
                                )));
                        }
                        Ok(PythonJobResult::Success) if scratch.exists() => {
                            if let Err(error) =
                                crate::app::audit::snapshot_link_or_copy(&scratch, &output)
                            {
                                all_ok = false;
                                last_failure =
                                    Some(crate::engine::workflow::WorkflowFailure::Other(format!(
                                        "verified aggregate output publication failed: {error}"
                                    )));
                            }
                        }
                        Ok(PythonJobResult::Success) => {
                            all_ok = false;
                            last_failure = Some(crate::engine::workflow::WorkflowFailure::Other(
                                "verified aggregate apply produced no output artifact".into(),
                            ));
                        }
                        Ok(PythonJobResult::Error(msg)) => {
                            all_ok = false;
                            if msg.contains("FONT_COVERAGE_INSUFFICIENT") {
                                let missing = serde_json::from_str::<serde_json::Value>(&msg)
                                    .ok()
                                    .and_then(|v| v.get("missing_chars").cloned())
                                    .and_then(|m| serde_json::from_value::<Vec<String>>(m).ok())
                                    .unwrap_or_default();
                                last_failure = Some(
                                    crate::engine::workflow::WorkflowFailure::FontCoverageFailed {
                                        missing_chars: missing,
                                    },
                                );
                            } else {
                                last_failure =
                                    Some(crate::engine::workflow::WorkflowFailure::Other(msg));
                            }
                        }
                        other => {
                            all_ok = false;
                            last_failure = Some(crate::engine::workflow::WorkflowFailure::Other(
                                format!("untyped or unexpected apply_many_edits result rejected: {other:?}"),
                            ));
                        }
                    }

                    if !all_ok {
                        let f = last_failure.unwrap_or(
                            crate::engine::workflow::WorkflowFailure::Other(
                                "apply step failed".into(),
                            ),
                        );
                        let _ = res_tx.send(JobResult::WorkflowFailed(f));
                        return;
                    }

                    // Stage 5: visual validation against the original.
                    visual_attempts += 1;
                    let _ = res_tx.send(JobResult::Progress {
                        label: format!("Visual & Math Verification (Attempt {attempt})"),
                        fraction: 0.3 + (attempt as f32 * 0.1).min(0.6),
                    });
                    let _ = res_tx.send(JobResult::WorkflowStageChanged {
                        stage: crate::engine::workflow::WorkflowStage::Validating(
                            crate::engine::workflow::VisualAttempt {
                                attempt,
                                max_attempts: 1,
                                diff_score: 0.0,
                                threshold: 0.02,
                                only_intended: false,
                                message: "rendering pages".into(),
                            },
                        ),
                    });

                    let observed_statement = match tokio::task::spawn_blocking({
                        let engine = engine_for_tokio.clone();
                        let output = output.clone();
                        move || {
                            crate::engine::offline_parser::parse_statement_offline(&output, engine)
                        }
                    })
                    .await
                    {
                        Ok(Ok(statement)) => statement,
                        Ok(Err(error)) => {
                            let _ = res_tx.send(JobResult::WorkflowFailed(
                                crate::engine::workflow::WorkflowFailure::FidelityCheckFailed(
                                    format!("mandatory local final-output reparse failed: {error}"),
                                ),
                            ));
                            return;
                        }
                        Err(error) => {
                            let _ = res_tx.send(JobResult::WorkflowFailed(
                                crate::engine::workflow::WorkflowFailure::FidelityCheckFailed(
                                    format!(
                                        "mandatory local final-output reparse task failed: {error}"
                                    ),
                                ),
                            ));
                            return;
                        }
                    };
                    verified_reparsed_count = observed_statement.transactions.len();
                    verified_final_imbalance =
                        (observed_statement.closing_balance - expected_closing).round_dp(2);

                    let mut provider_gates = Vec::new();
                    let (docai_status, docai_message) =
                        match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                            Ok(client) => match crate::engine::pro_edit::perform_pro_edit(
                                "DocumentAI",
                                async {
                                    client
                                        .parse_entire_statement(&output, None::<&str>)
                                        .await
                                        .map_err(anyhow::Error::from)
                                },
                                wdog.clone(),
                            )
                            .await
                            {
                                Ok(statement) => {
                                    let mut issues =
                                        crate::engine::workflow::deterministic_parse_issues(
                                            statement.total_pages,
                                            &statement.transactions,
                                            statement.opening_balance,
                                            statement.closing_balance,
                                        );
                                    if statement.transactions.len()
                                        != post_edit_transactions.len()
                                    {
                                        issues.push(format!(
                                            "provider row count {} differs from expected {}",
                                            statement.transactions.len(),
                                            post_edit_transactions.len()
                                        ));
                                    }
                                    if statement.closing_balance.round_dp(2)
                                        != expected_closing
                                    {
                                        issues.push(format!(
                                            "provider closing balance {} differs from expected {}",
                                            statement.closing_balance.round_dp(2),
                                            expected_closing
                                        ));
                                    }
                                    if issues.is_empty() {
                                        (
                                            crate::engine::verification::VerificationGateStatus::Passed,
                                            format!(
                                                "optional Document AI returned {} structurally and financially consistent row(s)",
                                                statement.transactions.len()
                                            ),
                                        )
                                    } else {
                                        (
                                            crate::engine::verification::VerificationGateStatus::Failed,
                                            format!(
                                                "optional Document AI disagreed with deterministic evidence: {}",
                                                issues.join("; ")
                                            ),
                                        )
                                    }
                                }
                                Err(error) => (
                                    crate::engine::verification::VerificationGateStatus::Unavailable,
                                    format!("optional Document AI unavailable: {error}"),
                                ),
                            },
                            Err(error) => (
                                crate::engine::verification::VerificationGateStatus::Unavailable,
                                format!("optional Document AI not configured: {error}"),
                            ),
                        };
                    provider_gates.push(crate::engine::verification::VerificationGate::optional(
                        "provider.document_ai",
                        docai_status,
                        docai_message,
                    ));

                    let math_inputs = crate::engine::verification::MathInputs {
                        transactions: observed_statement.transactions.clone(),
                        expected_transactions: Some(post_edit_transactions.clone()),
                        opening_balance,
                        expected_final_balance: Some(expected_closing),
                        required: true,
                    };
                    let out_dir = std::path::PathBuf::from("audit/verify").join(format!(
                        "workflow-{}",
                        chrono::Utc::now().format("%Y%m%d%H%M%S")
                    ));
                    let report =
                        match crate::engine::verification::verify_edit_with_intents_and_gates(
                            &input,
                            &output,
                            &out_dir,
                            &intended_edits,
                            &provider_gates,
                            math_inputs,
                            cfg.auto_match_dpi,
                            cfg.vision_api_key.clone(),
                        )
                        .await
                        {
                            Ok(report) => report,
                            Err(error) => {
                                let _ = res_tx.send(JobResult::WorkflowFailed(
                                    crate::engine::workflow::WorkflowFailure::Other(format!(
                                        "independent verification failed: {error}"
                                    )),
                                ));
                                return;
                            }
                        };

                    last_score = report.visual_diff_score;
                    last_intended = report.mandatory_local_pass();
                    let attempt_state = crate::engine::workflow::VisualAttempt {
                        attempt,
                        max_attempts: 1,
                        diff_score: report.visual_diff_score,
                        threshold: report
                            .gates
                            .iter()
                            .find(|gate| gate.id == "visual.outside_intended_regions")
                            .map(|_| 0.02)
                            .unwrap_or(0.02),
                        only_intended: report.mandatory_local_pass(),
                        message: report.message.clone(),
                    };
                    let _ = res_tx.send(JobResult::WorkflowVisualAttempt(attempt_state));

                    if !report.mandatory_local_pass() {
                        let (unavailable_gates, failed_gates): (Vec<_>, Vec<_>) = report
                            .gates
                            .iter()
                            .filter(|gate| {
                                gate.mandatory
                                    && gate.status
                                        != crate::engine::verification::VerificationGateStatus::Passed
                            })
                            .partition(|gate| {
                                gate.status
                                    == crate::engine::verification::VerificationGateStatus::Unavailable
                            });

                        let mut bypass_reasons = Vec::new();
                        if !failed_gates.is_empty() {
                            let failed_desc = failed_gates
                                .iter()
                                .map(|g| format!("{}: {}", g.id, g.message))
                                .collect::<Vec<_>>()
                                .join("; ");
                            bypass_reasons.push(format!("Failed gates: [{failed_desc}]"));
                        }
                        if !unavailable_gates.is_empty() {
                            let unavail_desc = unavailable_gates
                                .iter()
                                .map(|g| format!("{}: {}", g.id, g.message))
                                .collect::<Vec<_>>()
                                .join("; ");
                            bypass_reasons.push(format!("Unavailable gates: [{unavail_desc}]"));
                        }
                        let failure_summary = bypass_reasons.join(" | ");

                        if ignore_visual_fidelity {
                            tracing::warn!(
                                "[audit] visual fidelity check bypassed via ignore_visual_fidelity: {failure_summary}"
                            );
                        } else {
                            let _ = res_tx.send(JobResult::WorkflowFailed(
                                crate::engine::workflow::WorkflowFailure::FidelityCheckFailed(
                                    format!(
                                        "mandatory verification gates failed: {failure_summary}"
                                    ),
                                ),
                            ));
                            return;
                        }
                    }
                }

                let _ = res_tx.send(JobResult::WorkflowStageChanged {
                    stage: crate::engine::workflow::WorkflowStage::FinalChecking,
                });
                let _ = res_tx.send(JobResult::Progress {
                    label: "Finalizing independently verified output...".into(),
                    fraction: 0.98,
                });
                let final_imbalance = verified_final_imbalance;
                let re_parsed_count = verified_reparsed_count;
                let math_valid = last_intended;

                let staged_bytes = match std::fs::read(&output) {
                    Ok(bytes) if !bytes.is_empty() => bytes,
                    Ok(_) => {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(
                                "verified workflow output is empty".into(),
                            ),
                        ));
                        return;
                    }
                    Err(error) => {
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::Other(format!(
                                "verified workflow output is unavailable: {error}"
                            )),
                        ));
                        return;
                    }
                };
                let staged_hash = crate::engine::workflow::sha256_hex_of(&staged_bytes);
                let mut publication = crate::app::commit::FileCommitBarrier::new();
                if let Err(error) = publication.publish(&output, &requested_output) {
                    let _ = res_tx.send(JobResult::WorkflowFailed(
                        crate::engine::workflow::WorkflowFailure::Other(format!(
                            "verified workflow output publication failed: {error}"
                        )),
                    ));
                    return;
                }
                let published_hash = std::fs::read(&requested_output)
                    .map(|bytes| crate::engine::workflow::sha256_hex_of(&bytes));
                if !matches!(published_hash, Ok(ref hash) if *hash == staged_hash) {
                    let _ = res_tx.send(JobResult::WorkflowFailed(
                        crate::engine::workflow::WorkflowFailure::Other(
                            "published workflow output did not match the verified stage; prior output restored"
                                .into(),
                        ),
                    ));
                    return;
                }
                publication.commit();

                let bypass_note = if !last_intended && ignore_visual_fidelity {
                    " (VISUAL FIDELITY BYPASSED - OUTPUT UNVERIFIED)"
                } else {
                    ""
                };
                let outcome = crate::engine::workflow::WorkflowOutcome {
                    final_pdf: requested_output.clone(),
                    transactions_re_parsed: re_parsed_count,
                    final_imbalance,
                    math_valid,
                    visual_attempts,
                    completion_summary: format!(
                        "Bank statement confirmed{bypass_note}. Visual diff {last_score:.4}, intended-only={last_intended}, math valid={math_valid}."
                    ),
                };
                let _ = res_tx.send(JobResult::WorkflowStageChanged {
                    stage: crate::engine::workflow::WorkflowStage::Complete(outcome.clone()),
                });
                let _ = res_tx.send(JobResult::Progress {
                    label: "Done".into(),
                    fraction: 1.0,
                });
                let _ = res_tx.send(JobResult::WorkflowComplete(outcome));

                // Stage 4 / Item #13: refine the matched bank template
                // from the actual edited bboxes. Background task - we
                // don't block completion on it, just fire and log.
                let edits_for_learn = edits.clone();
                let input_for_learn = input.clone();
                let eng_for_learn = eng.clone();
                tokio::task::spawn_blocking(move || {
                    use crate::extractors::GeometryProvider;
                    let templates_dir = crate::app::paths::resolve_asset_path("bank_templates");
                    let provider = crate::extractors::BankTemplateProvider::new(
                        templates_dir.as_path(),
                        eng_for_learn,
                    );

                    // Find which template (if any) matched any geometry on the input.
                    let geos = match provider.extract_line_geometry(&input_for_learn) {
                        Ok(g) => g,
                        Err(e) => {
                            tracing::debug!("[templates] learn skipped (extract failed): {}", e);
                            return;
                        }
                    };
                    let mut matched_id: Option<String> = None;
                    for g in &geos {
                        if let crate::extractors::GeometrySource::BankTemplate { template_id } =
                            &g.source
                        {
                            matched_id = Some(template_id.clone());
                            break;
                        }
                    }
                    let Some(template_id) = matched_id else {
                        tracing::debug!("[templates] no template matched, skipping refine");
                        return;
                    };
                    let template = match provider.templates.iter().find(|t| t.id == template_id) {
                        Some(t) => t.clone(),
                        None => return,
                    };

                    // Build observations from the user's edits.
                    let observations: Vec<(String, [f32; 4])> = edits_for_learn
                        .iter()
                        .map(|e| {
                            let field_name = match e.field {
                                crate::engine::workflow::EditField::Date => "date",
                                crate::engine::workflow::EditField::Description => "description",
                                crate::engine::workflow::EditField::Debit => "debit",
                                crate::engine::workflow::EditField::Credit => "credit",
                                crate::engine::workflow::EditField::RunningBalance => "balance",
                            };
                            (field_name.to_string(), e.bbox)
                        })
                        .collect();

                    if observations.is_empty() {
                        return;
                    }

                    match crate::extractors::learn_template(
                        templates_dir.as_path(),
                        &template,
                        &observations,
                    ) {
                        Ok(p) => tracing::info!("[templates] refined template -> {}", p.display()),
                        Err(e) => tracing::warn!("[templates] refine failed: {}", e),
                    }
                });
            });
        }

        _ => unreachable!("unhandled job in this domain handler"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::verification::{
        VerificationGate, VerificationGateStatus, VerificationReport,
    };

    #[test]
    fn test_bypass_partitioning_distinguishes_failed_from_unavailable() {
        let report = VerificationReport {
            math_valid: true,
            visual_diff_score: 0.05,
            only_intended_changes: false,
            report_files: Vec::new(),
            message: "visual diff failed".into(),
            max_tile_score: 0.05,
            max_edit_region_score: 0.0,
            min_ssim: 0.0,
            gates: vec![
                VerificationGate::mandatory(
                    "visual.outside_intended_regions",
                    VerificationGateStatus::Failed,
                    "tile diff 0.05 exceeds threshold",
                ),
                VerificationGate::mandatory(
                    "visual.perceptual_structure",
                    VerificationGateStatus::Unavailable,
                    "dimension mismatch",
                ),
            ],
        };
        assert!(!report.mandatory_local_pass());

        let (unavailable_gates, failed_gates): (Vec<_>, Vec<_>) = report
            .gates
            .iter()
            .filter(|gate| gate.mandatory && gate.status != VerificationGateStatus::Passed)
            .partition(|gate| gate.status == VerificationGateStatus::Unavailable);

        let mut bypass_reasons = Vec::new();
        if !failed_gates.is_empty() {
            let failed_desc = failed_gates
                .iter()
                .map(|g| format!("{}: {}", g.id, g.message))
                .collect::<Vec<_>>()
                .join("; ");
            bypass_reasons.push(format!("Failed gates: [{failed_desc}]"));
        }
        if !unavailable_gates.is_empty() {
            let unavail_desc = unavailable_gates
                .iter()
                .map(|g| format!("{}: {}", g.id, g.message))
                .collect::<Vec<_>>()
                .join("; ");
            bypass_reasons.push(format!("Unavailable gates: [{unavail_desc}]"));
        }
        let failure_summary = bypass_reasons.join(" | ");

        assert!(failure_summary.contains(
            "Failed gates: [visual.outside_intended_regions: tile diff 0.05 exceeds threshold]"
        ));
        assert!(failure_summary
            .contains("Unavailable gates: [visual.perceptual_structure: dimension mismatch]"));
    }
}
