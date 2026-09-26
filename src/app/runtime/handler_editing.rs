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
        Job::AiFixVisualFidelity { input: _, page: _ } => {
            let _ = result_tx_clone.send(JobResult::Error {
                job_label: "ai_fix_visual_fidelity".to_string(),
                message: "AI visual layout repair is not available in v1; no document was changed. Use the deterministic edit and verification workflow.".to_string(),
            });
        }

        Job::AdjustDatePeriods {
            input,
            output,
            mode,
        } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            let py_tx = python_tx_clone.clone();
            let eng = engine_for_tokio.clone();
            tokio::spawn(async move {
                let _ = res_tx.send(JobResult::Progress {
                    label: "Parsing statement for date adjustment...".to_string(),
                    fraction: 0.1,
                });

                // Parse the statement — try Document AI, fall back to offline parser.
                let stmt = match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                    Ok(c) => {
                        let doc_ai: std::sync::Arc<crate::ai::document_ai::DocumentAiClient> =
                            std::sync::Arc::new(c);
                        match doc_ai.parse_entire_statement(&input, None::<&str>).await {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::warn!("[adjust_dates] Document AI parse failed, falling back to offline parser: {e}");
                                let _ = res_tx.send(JobResult::Progress {
                                    label: "Document AI failed, using offline parser..."
                                        .to_string(),
                                    fraction: 0.2,
                                });
                                let eng_clone = eng.clone();
                                let input_clone = input.clone();
                                match tokio::task::spawn_blocking(move || {
                                    crate::engine::offline_parser::parse_statement_offline(
                                        &input_clone,
                                        eng_clone,
                                    )
                                })
                                .await
                                {
                                    Ok(Ok(s)) => s,
                                    Ok(Err(e2)) => {
                                        let _ = res_tx.send(JobResult::Error {
                                            job_label: "adjust_dates".into(),
                                            message: format!("Offline parser also failed: {e2}"),
                                        });
                                        return;
                                    }
                                    Err(e2) => {
                                        let _ = res_tx.send(JobResult::Error {
                                            job_label: "adjust_dates".into(),
                                            message: format!("Offline parser panicked: {e2}"),
                                        });
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        tracing::info!(
                            "[adjust_dates] Document AI not configured, using offline parser"
                        );
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Using offline parser (no Document AI)...".to_string(),
                            fraction: 0.2,
                        });
                        let eng_clone = eng.clone();
                        let input_clone = input.clone();
                        match tokio::task::spawn_blocking(move || {
                            crate::engine::offline_parser::parse_statement_offline(
                                &input_clone,
                                eng_clone,
                            )
                        })
                        .await
                        {
                            Ok(Ok(s)) => s,
                            Ok(Err(e)) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "adjust_dates".into(),
                                    message: format!("Offline extraction failed: {e}"),
                                });
                                return;
                            }
                            Err(e) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "adjust_dates".into(),
                                    message: format!("Offline extraction panicked: {e}"),
                                });
                                return;
                            }
                        }
                    }
                };

                let _ = res_tx.send(JobResult::Progress {
                    label: "Adjusting dates...".to_string(),
                    fraction: 0.4,
                });

                let mut transactions = stmt.transactions;
                let records = match mode {
                    crate::engine::date_adjust::DateAdjustMode::ShiftDays(days) => {
                        crate::engine::date_adjust::shift_dates(&mut transactions, days)
                    }
                    crate::engine::date_adjust::DateAdjustMode::RemapPeriod {
                        from_start,
                        to_start,
                    } => crate::engine::date_adjust::remap_date_period(
                        &mut transactions,
                        from_start,
                        to_start,
                    ),
                };

                let total = records.len();
                if total == 0 {
                    let _ = res_tx.send(JobResult::completed(
                        "adjust_dates",
                        OperationDisposition::NoOp,
                        None,
                        "No transaction dates matched the requested adjustment; the output was left untouched",
                    ));
                    return;
                }

                let output_parent = output.parent().unwrap_or_else(|| std::path::Path::new("."));
                let staged_output = match crate::app::commit::staging_path(
                    output_parent,
                    ".date-adjust-",
                    ".pdf",
                ) {
                    Ok(path) => path,
                    Err(error) => {
                        let _ = res_tx.send(JobResult::completed(
                            "adjust_dates",
                            OperationDisposition::Failed,
                            None,
                            format!("Could not create an isolated output stage: {error}"),
                        ));
                        return;
                    }
                };
                if let Err(error) = std::fs::copy(&input, &staged_output) {
                    let _ = res_tx.send(JobResult::completed(
                        "adjust_dates",
                        OperationDisposition::Failed,
                        None,
                        format!("Could not stage the source PDF: {error}"),
                    ));
                    return;
                }

                let mut applied = 0usize;
                let mut failures = Vec::new();
                for (index, record) in records.iter().enumerate() {
                    let transaction = transactions.iter().find(|transaction| {
                        transaction.page == record.page
                            && transaction.line_on_page == record.line_on_page
                    });
                    let Some(date_bbox) = transaction.and_then(|tx| tx.field_bboxes.date) else {
                        failures.push(format!(
                            "page {} line {} has no date geometry",
                            record.page, record.line_on_page
                        ));
                        continue;
                    };

                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                    if let Err(error) = py_tx.send((
                        PythonJob::ReplaceTextInRect {
                            pdf_path: staged_output.to_string_lossy().to_string(),
                            output_path: staged_output.to_string_lossy().to_string(),
                            page_num: record.page,
                            rect: date_bbox,
                            old_text: transaction
                                .map(|transaction| transaction.date.clone())
                                .unwrap_or_default(),
                            new_text: record.new_date.clone(),
                            font_path: None,
                        },
                        reply_tx,
                    )) {
                        failures.push(format!(
                            "page {} line {} could not reach the Python worker: {error}",
                            record.page, record.line_on_page
                        ));
                        continue;
                    }

                    match reply_rx.await {
                        Ok(PythonJobResult::Success) => applied += 1,
                        Ok(PythonJobResult::ReplacedWithReviewWarning { reason }) => {
                            failures.push(format!(
                                "page {} line {} requires review: {reason}",
                                record.page, record.line_on_page
                            ));
                        }
                        Ok(PythonJobResult::Error(error)) => failures.push(format!(
                            "page {} line {} failed: {error}",
                            record.page, record.line_on_page
                        )),
                        Ok(other) => failures.push(format!(
                            "page {} line {} returned an invalid Python result: {other:?}",
                            record.page, record.line_on_page
                        )),
                        Err(error) => failures.push(format!(
                            "page {} line {} lost its Python reply: {error}",
                            record.page, record.line_on_page
                        )),
                    }

                    let fraction = 0.4 + (0.5 * (index + 1) as f32 / total as f32);
                    let _ = res_tx.send(JobResult::Progress {
                        label: format!("Updating date {}/{}", index + 1, total),
                        fraction,
                    });
                }

                if applied != total || !failures.is_empty() {
                    let _ = res_tx.send(JobResult::completed(
                        "adjust_dates",
                        OperationDisposition::Failed,
                        None,
                        format!(
                            "Date adjustment was not published: applied {applied}/{total}; {}",
                            failures.join("; ")
                        ),
                    ));
                    return;
                }

                let mut barrier = crate::app::commit::FileCommitBarrier::new();
                if let Err(error) = barrier.publish(&staged_output, &output) {
                    let _ = res_tx.send(JobResult::completed(
                        "adjust_dates",
                        OperationDisposition::Failed,
                        None,
                        format!("Could not publish the verified date-adjusted PDF: {error}"),
                    ));
                    return;
                }
                barrier.commit();

                let _ = res_tx.send(JobResult::DatesAdjusted {
                    records,
                    output_path: output.clone(),
                });
                let _ = res_tx.send(JobResult::completed(
                    "adjust_dates",
                    OperationDisposition::Succeeded,
                    Some(output),
                    format!("Applied all {total} date changes"),
                ));
            });
        }

        Job::ApplyChange {
            input,
            output,
            page,
            bbox,
            new_text,
            old_text,
            description,
            deep_font_replication,
        } => {
            let _ = result_tx_clone.send(JobResult::Progress {
                label: "Applying change".to_string(),
                fraction: 0.1,
            });

            let eng = engine_for_tokio.clone();
            let audit_log_clone = audit_log.clone();
            let history_clone = history.clone();
            let res_tx = result_tx_clone.clone();
            let cfg_clone = config_for_tokio.clone();

            let map_opt = segment_map.clone();
            let mgr_opt = segment_manager
                .as_ref()
                .map(|m| m.temp_path().to_path_buf());

            tokio::task::spawn(async move {
                // Automatic font generation is not a fidelity-preserving edit.
                // Reject the compatibility flag before staging any artifact.
                let font_path: Option<PathBuf> = None;
                if deep_font_replication {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "apply_change".into(),
                        message: "Automatic glyph synthesis and donor-font substitution are disabled; choose covered text or a separately reviewed supplied font."
                            .into(),
                    });
                    return;
                }

                // Every mutation is staged first. The live output and segment
                // files remain untouched until the complete commit barrier passes.
                let input_for_blocking = input.clone();
                let new_text_for_blocking = new_text.clone();
                let old_text_for_blocking = old_text.clone();
                let output_parent = output
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let staged_output = match crate::app::commit::staging_path(
                    output_parent,
                    ".dcpp-output-",
                    ".pdf",
                ) {
                    Ok(path) => path,
                    Err(error) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "apply_change".into(),
                            message: format!("Failed to create staged output: {error}"),
                        });
                        return;
                    }
                };
                let staged_output_for_blocking = staged_output.to_path_buf();

                let outcome = tokio::task::spawn_blocking(move || {
                    if let (Some(map), Some(temp_dir)) = (map_opt, mgr_opt) {
                        map.validate_structure().map_err(|error| {
                            crate::pdf::EngineError::ApplyFailed(format!(
                                "Invalid segment map: {error}"
                            ))
                        })?;
                        let (seg_idx, local_page) = map.resolve(page).ok_or_else(|| {
                            crate::pdf::EngineError::ApplyFailed(format!(
                                "Global page {page} not found in segment map"
                            ))
                        })?;

                        let segment_path = map.segments[seg_idx].path.clone();
                        let staged_segment = crate::app::commit::staging_path(
                            &temp_dir,
                            &format!(".dcpp-segment-{seg_idx}-"),
                            ".pdf",
                        )
                        .map_err(|error| {
                            crate::pdf::EngineError::ApplyFailed(format!(
                                "Failed to create staged segment: {error}"
                            ))
                        })?;

                        eng.apply_change(
                            &segment_path,
                            staged_segment.as_ref(),
                            local_page,
                            bbox,
                            &new_text_for_blocking,
                            &old_text_for_blocking,
                            font_path.as_deref(),
                        )?;

                        let mut ordered_paths = map.ordered_merge_paths();
                        ordered_paths[seg_idx] = staged_segment.to_path_buf();
                        let merged_pages = crate::engine::pdf_split_merge::merge_pdfs(
                            &ordered_paths,
                            &staged_output_for_blocking,
                        )
                        .map_err(|error| {
                            crate::pdf::EngineError::ApplyFailed(format!(
                                "Failed to merge staged segments: {error}"
                            ))
                        })?;
                        if merged_pages != map.total_pages {
                            return Err(crate::pdf::EngineError::ApplyFailed(format!(
                                "Segment merge page-count mismatch: expected {}, got {merged_pages}",
                                map.total_pages
                            )));
                        }

                        Ok((
                            ReplaceOutcome {
                                success: true,
                                font_used: "Helvetica".into(),
                                overflow: false,
                                obj_id: None,
                            },
                            Some((staged_segment, segment_path)),
                        ))
                    } else {
                        eng.apply_change(
                            &input_for_blocking,
                            &staged_output_for_blocking,
                            page,
                            bbox,
                            &new_text_for_blocking,
                            &old_text_for_blocking,
                            font_path.as_deref(),
                        )
                        .map(|result| (result, None))
                    }
                })
                .await
                .unwrap_or_else(|e| {
                    Err(crate::pdf::EngineError::ApplyFailed(format!(
                        "blocking task panicked: {e}"
                    )))
                });

                match outcome {
                    Ok((o, staged_segment_update)) => {
                        let requires_visual_review = o.overflow;
                        let mut h = match history_clone.lock() {
                            Ok(g) => g,
                            Err(e) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_change".into(),
                                    message: format!("History lock poisoned: {e}"),
                                });
                                return;
                            }
                        };
                        let mut a = match audit_log_clone.lock() {
                            Ok(g) => g,
                            Err(e) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_change".into(),
                                    message: format!("Audit lock poisoned: {e}"),
                                });
                                return;
                            }
                        };

                        let mut final_record = h.create_record(
                            page,
                            old_text,
                            new_text.clone(),
                            bbox,
                            description,
                            None,
                        );
                        final_record.obj_id = o.obj_id;

                        let (snapshot_path, snapshot_evidence) = match a
                            .create_content_addressed_snapshot(
                                final_record.id,
                                staged_output.as_ref(),
                                Some(&input),
                            ) {
                            Ok(snapshot) => snapshot,
                            Err(error) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_change".into(),
                                    message: format!("Snapshot failed: {error}"),
                                });
                                return;
                            }
                        };
                        final_record.snapshot_path = Some(snapshot_path);
                        final_record.snapshot_evidence = Some(snapshot_evidence.clone());
                        if let Err(error) = a.verify_snapshot_record(&final_record) {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_change".into(),
                                message: format!("Snapshot verification failed: {error}"),
                            });
                            return;
                        }

                        let mut staged_history_state = h.clone();
                        staged_history_state.push_record(final_record.clone());
                        let autosave_path = PathBuf::from("audit").join("history.json");
                        let autosave_parent = autosave_path
                            .parent()
                            .filter(|parent| !parent.as_os_str().is_empty())
                            .unwrap_or_else(|| Path::new("."));
                        let staged_history = match crate::app::commit::staging_path(
                            autosave_parent,
                            ".dcpp-history-",
                            ".json",
                        ) {
                            Ok(path) => path,
                            Err(error) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_change".into(),
                                    message: format!("History staging failed: {error}"),
                                });
                                return;
                            }
                        };
                        if let Err(error) =
                            staged_history_state.save_to_file(staged_history.as_ref())
                        {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_change".into(),
                                message: format!("History staging failed: {error}"),
                            });
                            return;
                        }

                        let mut commit_barrier = crate::app::commit::FileCommitBarrier::new();
                        if let Some((staged_segment, segment_path)) = staged_segment_update.as_ref()
                        {
                            if let Err(error) =
                                commit_barrier.publish(staged_segment.as_ref(), segment_path)
                            {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_change".into(),
                                    message: format!("Segment commit failed: {error}"),
                                });
                                return;
                            }
                        }
                        if let Err(error) = commit_barrier.publish(staged_output.as_ref(), &output)
                        {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_change".into(),
                                message: format!("Output commit failed: {error}"),
                            });
                            return;
                        }
                        if let Err(error) =
                            a.verify_artifact_matches_snapshot(&output, &snapshot_evidence)
                        {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_change".into(),
                                message: format!("Published output verification failed: {error}"),
                            });
                            return;
                        }
                        if let Err(error) =
                            commit_barrier.publish(staged_history.as_ref(), &autosave_path)
                        {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_change".into(),
                                message: format!("History commit failed: {error}"),
                            });
                            return;
                        }
                        if let Err(error) = a.write(
                            &final_record,
                            &input,
                            &output,
                            "manual",
                            requires_visual_review,
                        ) {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_change".into(),
                                message: format!("Audit commit failed: {error}"),
                            });
                            return;
                        }

                        commit_barrier.commit();
                        *h = staged_history_state;

                        if let Some(url) = cfg_clone.webhook_url.clone() {
                            let old = final_record.old_text.clone();
                            let new = final_record.new_text.clone();
                            let desc = final_record.description.clone();
                            let page = final_record.page;
                            tokio::spawn(async move {
                                crate::app::notify::send_webhook(
                                    &url,
                                    crate::app::notify::WebhookPayload {
                                        event: "change_applied",
                                        page,
                                        old_text: &old,
                                        new_text: &new,
                                        description: &desc,
                                    },
                                )
                                .await;
                            });
                        }
                        let h_final = h.clone();
                        let _ = res_tx.send(JobResult::ChangeApplied {
                            record: final_record,
                            requires_visual_review,
                        });
                        let _ = res_tx.send(JobResult::HistoryUpdated { history: h_final });
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Done".to_string(),
                            fraction: 1.0,
                        });
                    }
                    Err(crate::pdf::EngineError::EncryptedOrRasterized(msg)) => {
                        let _ = res_tx.send(JobResult::NuclearFallbackRequired(msg));
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "apply_change".into(),
                            message: e.to_string(),
                        });
                    }
                }
            });
        }

        Job::Undo => {
            let history_clone = history.clone();
            let res_tx = result_tx_clone.clone();
            let _ = tokio::task::spawn_blocking(move || match history_clone.lock() {
                Ok(mut h) => {
                    h.undo();
                    let _ = res_tx.send(JobResult::HistoryUpdated { history: h.clone() });
                }
                Err(e) => {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "undo".into(),
                        message: format!("History lock poisoned: {e}"),
                    });
                }
            })
            .await;
        }

        Job::Redo => {
            let history_clone = history.clone();
            let res_tx = result_tx_clone.clone();
            let _ = tokio::task::spawn_blocking(move || match history_clone.lock() {
                Ok(mut h) => {
                    h.redo();
                    let _ = res_tx.send(JobResult::HistoryUpdated { history: h.clone() });
                }
                Err(e) => {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "redo".into(),
                        message: format!("History lock poisoned: {e}"),
                    });
                }
            })
            .await;
        }

        Job::ApplyProposedChanges {
            input,
            output,
            changes,
        } => {
            let res_tx = result_tx_clone.clone();
            let py_tx = python_tx_clone.clone();
            let semaphore = api_semaphore.clone();

            tokio::spawn(async move {
                let _permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "API Execution".into(),
                            message: format!("Semaphore closed: {e}"),
                        });
                        return;
                    }
                };
                // Determine page count: cascaded balance changes
                // routinely land MANY pages from the edited row -
                // often >3 pages away. A direct full-document apply
                // would trip the PyMuPDF Pro 3-page guard, so for
                // long statements we route through 3-Page-Mode:
                // split -> per-segment apply (<=3 pages each) ->
                // merge. Short docs use the simple direct path.
                let input_for_count = input.clone();
                let page_count = tokio::task::spawn_blocking(move || {
                    lopdf::Document::load(&input_for_count)
                        .map(|d| d.get_pages().len())
                        .unwrap_or(0)
                })
                .await
                .unwrap_or(0);

                // Drop changes with no resolved bbox up front (can't redact).
                let mut failures: Vec<String> = Vec::new();
                let usable: Vec<crate::engine::model::ProposedChange> = changes
                                .iter()
                                .filter(|c| {
                                    if c.bbox.is_none() {
                                        failures.push(format!(
                                            "Proposed change for page {} '{}' \u{2192} '{}' has no resolved bbox; skipped",
                                            c.page + 1, c.old_text, c.new_text
                                        ));
                                        false
                                    } else {
                                        true
                                    }
                                })
                                .cloned()
                                .collect();

                if !failures.is_empty() {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "apply_proposed_changes".into(),
                        message: format!(
                            "Exact batch apply rejected unresolved changes: {}",
                            failures.join("; ")
                        ),
                    });
                    return;
                }
                if usable.is_empty() {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "apply_proposed_changes".into(),
                        message: "Exact batch apply requires at least one resolved change".into(),
                    });
                    return;
                }

                if page_count > 3 {
                    // ---- 3-Page-Mode segmented batch apply ----
                    use crate::engine::pdf_split_merge::{merge_pdfs, split_pdf};
                    let _ = res_tx.send(JobResult::Progress {
                        label: "Splitting statement into <=3-page segments".into(),
                        fraction: 0.1,
                    });

                    // 1) Split (pure-Rust lopdf) on a blocking task.
                    let input_split = input.clone();
                    let split_res = tokio::task::spawn_blocking(move || {
                        let tmp = tempfile::Builder::new()
                            .prefix("apply-cascade-")
                            .tempdir()
                            .map_err(|e| format!("tempdir: {e}"))?;
                        let segments = split_pdf(&input_split, tmp.path(), 3)
                            .map_err(|e| format!("split failed: {e}"))?;
                        Ok::<_, String>((tmp, segments))
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("split task panicked: {e}")));

                    let (tmp, segments) = match split_res {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_proposed_changes".into(),
                                message: e,
                            });
                            return;
                        }
                    };

                    // 2) Group usable changes by segment (global -> local page).
                    use std::collections::BTreeMap;
                    let mut by_seg: BTreeMap<
                        usize,
                        Vec<(usize, crate::engine::model::ProposedChange)>,
                    > = BTreeMap::new();
                    for ch in &usable {
                        match segments.iter().position(|s| {
                            ch.page >= s.page_offset && ch.page < s.page_offset + s.page_count
                        }) {
                            Some(si) => {
                                let local = ch.page - segments[si].page_offset;
                                by_seg.entry(si).or_default().push((local, ch.clone()));
                            }
                            None => failures.push(format!(
                                "change on global page {} is out of range (doc has {} pages)",
                                ch.page + 1,
                                page_count
                            )),
                        }
                    }
                    if !failures.is_empty() {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "apply_proposed_changes".into(),
                            message: format!(
                                "Segment membership validation failed before mutation: {}",
                                failures.join("; ")
                            ),
                        });
                        return;
                    }

                    // 3) Per-segment apply via the Python actor (each <=3 pages, Pro-legal).
                    let mut seg_paths: Vec<std::path::PathBuf> =
                        segments.iter().map(|s| s.path.clone()).collect();
                    let mut applied = 0usize;
                    let total_segs = by_seg.len().max(1);
                    for (done, (si, edits)) in by_seg.into_iter().enumerate() {
                        let _ = res_tx.send(JobResult::Progress {
                            label: format!("Editing segment {} of {}", done + 1, total_segs),
                            fraction: 0.2 + 0.6 * (done as f32 / total_segs as f32),
                        });
                        let edits_json: Vec<serde_json::Value> = edits
                            .iter()
                            .filter_map(|(local, ch)| {
                                let b = ch.bbox?;
                                Some(serde_json::json!({
                                    "page": local,
                                    "rect": [b[0], b[1], b[2], b[3]],
                                    "old_text": ch.old_text,
                                    "new_text": ch.new_text,
                                }))
                            })
                            .collect();
                        let json_str =
                            serde_json::to_string(&edits_json).unwrap_or_else(|_| "[]".into());
                        let edited_out = tmp.path().join(format!("segment_{si:03}_edited.pdf"));
                        let expected = edits.len();

                        let (rtx, rrx) = oneshot::channel();
                        let _ = py_tx.send((
                            PythonJob::ApplyManyEdits {
                                pdf_path: seg_paths[si].to_string_lossy().to_string(),
                                output_path: edited_out.to_string_lossy().to_string(),
                                edits_json: json_str.clone(),
                                font_path: None,
                                strict_fidelity: false,
                            },
                            rtx,
                        ));
                        match rrx.await {
                            Ok(PythonJobResult::ApplyReport(report))
                                if report.success
                                    && report.requested == expected
                                    && report.matched == expected
                                    && report.placed == expected
                                    && report.failed == 0
                                    && report.review_flags.is_empty()
                                    && edited_out.is_file() =>
                            {
                                match crate::engine::segments::validate_segment_replacement(
                                    &segments[si].path,
                                    &edited_out,
                                    segments[si].page_count,
                                ) {
                                    Ok(()) => {
                                        seg_paths[si] = edited_out;
                                        applied += report.placed;
                                    }
                                    Err(validation_error) => {
                                        let _ = std::fs::remove_file(&edited_out);
                                        failures.push(format!(
                                            "segment {si}: Python output failed page membership validation: {validation_error}"
                                        ));
                                    }
                                }
                            }
                            Ok(PythonJobResult::ApplyReport(report)) => {
                                let _ = std::fs::remove_file(&edited_out);
                                failures.push(format!(
                                    "segment {si}: exact Python apply failed (requested {}, matched {}, placed {}, failed {}, expected {}): {}",
                                    report.requested,
                                    report.matched,
                                    report.placed,
                                    report.failed,
                                    expected,
                                    report.warnings.join("; ")
                                ));
                            }
                            Ok(PythonJobResult::Error(error)) => {
                                tracing::warn!(
                                    segment = si,
                                    python_error = %error,
                                    "Python actor errored; attempting exact-count native fallback"
                                );
                                let native_in = seg_paths[si].clone();
                                let native_path =
                                    tmp.path().join(format!("segment_{si:03}_native.pdf"));
                                let native_out = native_path.clone();
                                let native_json = json_str.clone();
                                let native_result = tokio::task::spawn_blocking(move || {
                                    let native_engine =
                                        crate::pdf::native_engine::OxidizePdfEngine::new();
                                    native_engine.apply_many_edits(
                                        &native_in,
                                        &native_out,
                                        &native_json,
                                        None,
                                    )
                                })
                                .await;
                                match native_result {
                                    Ok(Ok(count)) if count == expected && native_path.is_file() => {
                                        match crate::engine::segments::validate_segment_replacement(
                                            &segments[si].path,
                                            &native_path,
                                            segments[si].page_count,
                                        ) {
                                            Ok(()) => {
                                                seg_paths[si] = native_path;
                                                applied += count;
                                                tracing::info!(
                                                    segment = si,
                                                    edits_applied = count,
                                                    "Exact-count native fallback succeeded"
                                                );
                                            }
                                            Err(validation_error) => {
                                                let _ = std::fs::remove_file(&native_path);
                                                failures.push(format!(
                                                    "segment {si}: native output failed page membership validation: {validation_error}"
                                                ));
                                            }
                                        }
                                    }
                                    Ok(Ok(count)) => {
                                        let _ = std::fs::remove_file(&native_path);
                                        failures.push(format!(
                                            "segment {si}: Python failed ({error}); native applied {count}/{expected} edits"
                                        ));
                                    }
                                    Ok(Err(native_error)) => failures.push(format!(
                                        "segment {si}: Python failed ({error}); native failed ({native_error})"
                                    )),
                                    Err(panic_error) => failures.push(format!(
                                        "segment {si}: Python failed ({error}); native panicked ({panic_error})"
                                    )),
                                }
                            }
                            other => failures.push(format!(
                                "segment {si}: unexpected Python batch-edit result: {other:?}"
                            )),
                        }
                    }

                    if !failures.is_empty() || applied != usable.len() {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "apply_proposed_changes".into(),
                            message: format!(
                                "Exact batch apply aborted before merge: applied {applied}/{}; {}",
                                usable.len(),
                                failures.join("; ")
                            ),
                        });
                        return;
                    }

                    // 4) Merge into a same-directory stage, then publish through
                    // a rollback-capable barrier only after page membership passes.
                    let _ = res_tx.send(JobResult::Progress {
                        label: "Merging segments".into(),
                        fraction: 0.9,
                    });
                    let output_parent = output
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .unwrap_or_else(|| Path::new("."));
                    let staged_output = match crate::app::commit::staging_path(
                        output_parent,
                        ".dcpp-proposed-merge-",
                        ".pdf",
                    ) {
                        Ok(path) => path,
                        Err(error) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_proposed_changes".into(),
                                message: format!("Failed to stage segment merge: {error}"),
                            });
                            return;
                        }
                    };
                    let seg_paths_for_merge = seg_paths.clone();
                    let staged_for_merge = staged_output.to_path_buf();
                    let merge_res = tokio::task::spawn_blocking(move || {
                        merge_pdfs(&seg_paths_for_merge, &staged_for_merge)
                            .map_err(|error| format!("merge failed: {error}"))
                    })
                    .await
                    .unwrap_or_else(|error| Err(format!("merge task panicked: {error}")));

                    // Keep tmp alive until after merge reads the segment files.
                    drop(tmp);

                    match merge_res {
                        Ok(merged) if merged == page_count => {
                            let mut barrier = crate::app::commit::FileCommitBarrier::new();
                            if let Err(error) = barrier.publish(staged_output.as_ref(), &output) {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_proposed_changes".into(),
                                    message: format!("Merged output commit failed: {error}"),
                                });
                                return;
                            }
                            let published_pages = lopdf::Document::load(&output)
                                .map(|document| document.get_pages().len());
                            match published_pages {
                                Ok(count) if count == page_count => {}
                                Ok(count) => {
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "apply_proposed_changes".into(),
                                        message: format!(
                                            "Published page count {count} != original {page_count}; prior output restored"
                                        ),
                                    });
                                    return;
                                }
                                Err(error) => {
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "apply_proposed_changes".into(),
                                        message: format!(
                                            "Published merge could not be reopened: {error}; prior output restored"
                                        ),
                                    });
                                    return;
                                }
                            }
                            barrier.commit();
                            let _ = res_tx.send(JobResult::ProposedChangesApplied {
                                changes_applied: applied,
                                failures,
                            });
                            let _ = res_tx.send(JobResult::Progress {
                                label: "Done (3-page mode)".to_string(),
                                fraction: 1.0,
                            });
                        }
                        Ok(merged) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_proposed_changes".into(),
                                message: format!(
                                    "Merged page count {merged} != original {page_count}; output not published"
                                ),
                            });
                        }
                        Err(error) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_proposed_changes".into(),
                                message: error,
                            });
                        }
                    }
                    return;
                }

                // ---- Short document (<=3 pages): one ordered exact batch ----
                let _ = res_tx.send(JobResult::Progress {
                    label: format!(
                        "Applying {} changes as one document transaction",
                        usable.len()
                    ),
                    fraction: 0.25,
                });
                let edit_values: Vec<serde_json::Value> = usable
                    .iter()
                    .map(|change| {
                        #[allow(clippy::expect_used)]
                        let bbox = change.bbox.expect("usable changes filtered to have bboxes");
                        serde_json::json!({
                            "page": change.page,
                            "rect": [bbox[0], bbox[1], bbox[2], bbox[3]],
                            "old_text": change.old_text,
                            "new_text": change.new_text,
                        })
                    })
                    .collect();
                let edits_json = match serde_json::to_string(&edit_values) {
                    Ok(json) => json,
                    Err(error) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "apply_proposed_changes".into(),
                            message: format!("Exact batch serialization failed: {error}"),
                        });
                        return;
                    }
                };
                let scratch =
                    output.with_extension(format!("{}.apply-transaction.pdf", Uuid::new_v4()));
                let _ = std::fs::remove_file(&scratch);
                let (reply_tx, reply_rx) = oneshot::channel();
                if py_tx
                    .send((
                        PythonJob::ApplyManyEdits {
                            pdf_path: input.to_string_lossy().to_string(),
                            output_path: scratch.to_string_lossy().to_string(),
                            edits_json: edits_json.clone(),
                            font_path: None,
                            strict_fidelity: false,
                        },
                        reply_tx,
                    ))
                    .is_err()
                {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "apply_proposed_changes".into(),
                        message: "Python batch-edit actor is unavailable".into(),
                    });
                    return;
                }

                match reply_rx.await {
                    Ok(PythonJobResult::ApplyReport(report))
                        if report.success
                            && report.requested == usable.len()
                            && report.matched == usable.len()
                            && report.placed == usable.len()
                            && report.failed == 0
                            && report.review_flags.is_empty()
                            && scratch.is_file() =>
                    {
                        let mut barrier = crate::app::commit::FileCommitBarrier::new();
                        if let Err(error) = barrier.publish(&scratch, &output) {
                            let _ = std::fs::remove_file(&scratch);
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_proposed_changes".into(),
                                message: format!("Exact output commit failed: {error}"),
                            });
                            return;
                        }
                        let published_pages = lopdf::Document::load(&output)
                            .map(|document| document.get_pages().len());
                        if !matches!(published_pages, Ok(count) if count == page_count) {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "apply_proposed_changes".into(),
                                message: "Published exact output failed page-count validation; prior output restored"
                                    .into(),
                            });
                            return;
                        }
                        barrier.commit();
                        let _ = std::fs::remove_file(&scratch);
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Exact batch committed".to_string(),
                            fraction: 1.0,
                        });
                        let _ = res_tx.send(JobResult::ProposedChangesApplied {
                            changes_applied: report.placed,
                            failures: Vec::new(),
                        });
                    }
                    Ok(PythonJobResult::ApplyReport(report)) => {
                        let primary_error = format!(
                            "Exact batch failed: placed {}/{}; {}",
                            report.placed,
                            report.requested,
                            report.warnings.join("; ")
                        );
                        tracing::warn!(
                            %primary_error,
                            "Python exact batch incomplete; attempting exact-count native fallback"
                        );
                        let _ = std::fs::remove_file(&scratch);
                        let native_result = tokio::task::spawn_blocking({
                            let native_in = input.clone();
                            let native_out = scratch.clone();
                            let native_json = edits_json.clone();
                            move || {
                                let native_engine =
                                    crate::pdf::native_engine::OxidizePdfEngine::new();
                                native_engine.apply_many_edits(
                                    &native_in,
                                    &native_out,
                                    &native_json,
                                    None,
                                )
                            }
                        })
                        .await;
                        match native_result {
                            Ok(Ok(count)) if count == usable.len() && scratch.is_file() => {
                                let mut barrier = crate::app::commit::FileCommitBarrier::new();
                                if let Err(error) = barrier.publish(&scratch, &output) {
                                    let _ = std::fs::remove_file(&scratch);
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "apply_proposed_changes".into(),
                                        message: format!("Exact output commit failed: {error}"),
                                    });
                                    return;
                                }
                                let published_pages = lopdf::Document::load(&output)
                                    .map(|document| document.get_pages().len());
                                if !matches!(published_pages, Ok(c) if c == page_count) {
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "apply_proposed_changes".into(),
                                        message: "Published exact output failed page-count validation; prior output restored"
                                            .into(),
                                    });
                                    return;
                                }
                                barrier.commit();
                                let _ = std::fs::remove_file(&scratch);
                                let _ = res_tx.send(JobResult::Progress {
                                    label: "Exact batch committed (native fallback)".to_string(),
                                    fraction: 1.0,
                                });
                                let _ = res_tx.send(JobResult::ProposedChangesApplied {
                                    changes_applied: count,
                                    failures: Vec::new(),
                                });
                            }
                            _ => {
                                let _ = std::fs::remove_file(&scratch);
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_proposed_changes".into(),
                                    message: primary_error,
                                });
                            }
                        }
                    }
                    Ok(PythonJobResult::Error(error)) => {
                        tracing::warn!(
                            python_error = %error,
                            "Python actor errored; attempting exact-count native fallback"
                        );
                        let _ = std::fs::remove_file(&scratch);
                        let native_result = tokio::task::spawn_blocking({
                            let native_in = input.clone();
                            let native_out = scratch.clone();
                            let native_json = edits_json.clone();
                            move || {
                                let native_engine =
                                    crate::pdf::native_engine::OxidizePdfEngine::new();
                                native_engine.apply_many_edits(
                                    &native_in,
                                    &native_out,
                                    &native_json,
                                    None,
                                )
                            }
                        })
                        .await;

                        match native_result {
                            Ok(Ok(count)) if count == usable.len() && scratch.is_file() => {
                                let mut barrier = crate::app::commit::FileCommitBarrier::new();
                                if let Err(error) = barrier.publish(&scratch, &output) {
                                    let _ = std::fs::remove_file(&scratch);
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "apply_proposed_changes".into(),
                                        message: format!("Exact output commit failed: {error}"),
                                    });
                                    return;
                                }
                                let published_pages = lopdf::Document::load(&output)
                                    .map(|document| document.get_pages().len());
                                if !matches!(published_pages, Ok(c) if c == page_count) {
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "apply_proposed_changes".into(),
                                        message: "Published exact output failed page-count validation; prior output restored"
                                            .into(),
                                    });
                                    return;
                                }
                                barrier.commit();
                                let _ = std::fs::remove_file(&scratch);
                                let _ = res_tx.send(JobResult::Progress {
                                    label: "Exact batch committed (native fallback)".to_string(),
                                    fraction: 1.0,
                                });
                                let _ = res_tx.send(JobResult::ProposedChangesApplied {
                                    changes_applied: count,
                                    failures: Vec::new(),
                                });
                            }
                            _ => {
                                let _ = std::fs::remove_file(&scratch);
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "apply_proposed_changes".into(),
                                    message: error,
                                });
                            }
                        }
                    }
                    other => {
                        let _ = std::fs::remove_file(&scratch);
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "apply_proposed_changes".into(),
                            message: format!("Unexpected exact batch result: {other:?}"),
                        });
                    }
                }
            });
        }

        Job::GenerateVisualAlternatives {
            input,
            out_dir,
            page,
            edits,
            bbox,
        } => {
            let res_tx = result_tx_clone.clone();
            let py_tx = python_tx_clone.clone();
            let eng_clone = engine_for_tokio.clone();

            tokio::spawn(async move {
                // Produce only alternatives implemented by their named engines.
                let (rtx, rrx) = oneshot::channel();

                // A) PyMuPDF Pro (via Python Bridge)
                let py_out = out_dir.join(format!("page_{}_pymupdf.pdf", page));
                let edits_json: Vec<serde_json::Value> = edits
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "page": e.page,
                            "rect": [e.bbox[0], e.bbox[1], e.bbox[2], e.bbox[3]],
                            "old_text": e.old_text,
                            "new_text": e.new_text,
                        })
                    })
                    .collect();
                let json_str = serde_json::to_string(&edits_json).unwrap_or_else(|_| "[]".into());

                let _ = py_tx.send((
                    PythonJob::ApplyManyEdits {
                        pdf_path: input.to_string_lossy().to_string(),
                        output_path: py_out.to_string_lossy().to_string(),
                        edits_json: json_str.clone(),
                        font_path: None,
                        strict_fidelity: false,
                    },
                    rtx,
                ));
                let mut candidate_outputs: Vec<(&str, std::path::PathBuf)> = Vec::new();
                let mut candidate_failures = Vec::new();
                match rrx.await {
                    Ok(PythonJobResult::ApplyReport(report)) => {
                        let exact = report.validate_exact(edits.len());
                        let files = report.verify_files(&input, &py_out);
                        if exact.is_ok() && files.is_ok() && report.success {
                            candidate_outputs.push(("PyMuPDF Pro", py_out.clone()));
                        } else {
                            candidate_failures.push(format!(
                                "PyMuPDF Pro rejected: exact={exact:?}, files={files:?}, success={}",
                                report.success
                            ));
                        }
                    }
                    Ok(other) => candidate_failures
                        .push(format!("PyMuPDF Pro returned non-apply result: {other:?}")),
                    Err(error) => candidate_failures
                        .push(format!("PyMuPDF Pro response channel failed: {error}")),
                }

                // B) Native Rust
                let native_out = out_dir.join(format!("page_{}_native.pdf", page));
                let native_in = input.clone();
                let native_json = json_str.clone();
                let native_out_clone = native_out.clone();
                match tokio::task::spawn_blocking(move || {
                    let native_eng = crate::pdf::native_engine::OxidizePdfEngine::new();
                    native_eng.apply_many_edits(&native_in, &native_out_clone, &native_json, None)
                })
                .await
                {
                    Ok(Ok(applied)) if applied == edits.len() && native_out.is_file() => {
                        candidate_outputs.push(("Native Rust", native_out.clone()));
                    }
                    Ok(Ok(applied)) => candidate_failures.push(format!(
                        "Native Rust applied {applied} of {} edits",
                        edits.len()
                    )),
                    Ok(Err(error)) => {
                        candidate_failures.push(format!("Native Rust failed: {error}"))
                    }
                    Err(error) => {
                        candidate_failures.push(format!("Native Rust worker panicked: {error}"))
                    }
                }

                // Render each successful named output to PNG and crop to bbox + 50px padding.
                let mut images = Vec::new();
                let targets = candidate_outputs;

                for (label, out_path) in targets {
                    let render = tokio::task::spawn_blocking({
                        let eng = eng_clone.clone();
                        let path = out_path.clone();
                        move || eng.render_page(&path, page, 300.0)
                    })
                    .await
                    .ok()
                    .and_then(|r| r.ok());

                    if let Some(render_res) = render {
                        if let Ok(mut img) = image::load_from_memory(&render_res.png_bytes) {
                            // Simple crop logic based on bbox and DPI
                            // bbox is in pts (72 dpi). We rendered at 300 dpi.
                            let scale = 300.0 / 72.0;
                            let padding = 50.0;

                            let x = ((bbox[0] * scale) - padding).max(0.0) as u32;
                            let y = ((bbox[1] * scale) - padding).max(0.0) as u32;
                            let w = (((bbox[2] - bbox[0]) * scale) + 2.0 * padding).max(1.0) as u32;
                            let h = (((bbox[3] - bbox[1]) * scale) + 2.0 * padding).max(1.0) as u32;

                            let img_w = img.width();
                            let img_h = img.height();
                            let cropped = image::imageops::crop(
                                &mut img,
                                x,
                                y,
                                w.min(img_w.saturating_sub(x)),
                                h.min(img_h.saturating_sub(y)),
                            )
                            .to_image();
                            let mut buf = std::io::Cursor::new(Vec::new());
                            if cropped.write_to(&mut buf, image::ImageFormat::Png).is_ok() {
                                images.push((label.to_string(), buf.into_inner()));
                            }
                        }
                    }
                }

                if images.is_empty() {
                    let _ = res_tx.send(JobResult::Error {
                        job_label: "generate_visual_alternatives".into(),
                        message: format!(
                            "No named edit engine produced a verified visual alternative: {}",
                            candidate_failures.join("; ")
                        ),
                    });
                } else {
                    let _ = res_tx.send(JobResult::VisualAlternativesReady(images));
                }
            });
        }

        Job::TypstReconstruct {
            input: _,
            output: _,
        } => {
            // Typst rebuild is an export-style path that cannot preserve
            // edit-in-place visual fidelity. Keep the job for API stability
            // but fail closed with a clear reason (same gate as workflow finalize).
            let tx = result_tx_clone.clone();
            tokio::spawn(async move {
                let _ = tx.send(JobResult::Error {
                    job_label: "typst_reconstruct_disabled".into(),
                    message: "Automatic Typst reconstruction is disabled in this build: cannot preserve edit-in-place fidelity".into(),
                });
            });
        }

        _ => unreachable!("unhandled job in this domain handler"),
    }
}
