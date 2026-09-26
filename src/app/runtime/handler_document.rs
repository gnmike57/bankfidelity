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
        Job::LoadDocument {
            path,
            three_page_mode,
        } => {
            let _ = result_tx_clone.send(JobResult::Progress {
                label: "Analyzing layout".to_string(),
                fraction: 0.1,
            });

            // Cleanup previous segments if any
            if let Some(mgr) = segment_manager.take() {
                mgr.cleanup();
            }
            *segment_map = None;

            if three_page_mode {
                match SegmentManager::new() {
                    Ok(mgr) => match mgr.prepare(&path, 3) {
                        Ok(map) => {
                            *segment_map = Some(map.clone());
                            let total_pages = map.total_pages;
                            *segment_manager = Some(mgr);
                            let _ = result_tx_clone.send(JobResult::DocumentLoaded {
                                layout_json: "[]".into(),
                                total_pages,
                            });
                            let _ = result_tx_clone.send(JobResult::Progress {
                                label: "Done (3-page mode)".into(),
                                fraction: 1.0,
                            });
                        }
                        Err(e) => {
                            let _ = result_tx_clone.send(JobResult::Error {
                                job_label: "load_document_split".into(),
                                message: e.to_string(),
                            });
                        }
                    },
                    Err(e) => {
                        let _ = result_tx_clone.send(JobResult::Error {
                            job_label: "load_document_tempdir".into(),
                            message: e.to_string(),
                        });
                    }
                }
            } else {
                let eng = engine_for_tokio.clone();
                let res_tx = result_tx_clone.clone();
                let path_for_blocking = path.clone();
                tokio::task::spawn_blocking(move || match eng.analyze_layout(&path_for_blocking) {
                    Ok(layout) => {
                        let json = serde_json::to_string(&layout.pages).unwrap_or_default();
                        let _ = res_tx.send(JobResult::DocumentLoaded {
                            layout_json: json,
                            total_pages: layout.total_pages,
                        });
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Done".to_string(),
                            fraction: 1.0,
                        });
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "load_document".into(),
                            message: e.to_string(),
                        });
                    }
                });
            }

            // Stage 8.5: kick off the font analysis in parallel.
            let res_tx_fonts = result_tx_clone.clone();
            let py_tx_for_fonts = python_tx_clone.clone();
            let path_for_fonts = path.clone();
            tokio::spawn(async move {
                // Compute the hash on a blocking task so we
                // don't stall the tokio runtime.
                let path_for_hash = path_for_fonts.clone();
                let hash_opt: Option<String> =
                    tokio::task::spawn_blocking(move || -> Option<String> {
                        let bytes = std::fs::read(&path_for_hash).ok()?;
                        Some(crate::engine::workflow::sha256_hex_of(&bytes))
                    })
                    .await
                    .ok()
                    .flatten();

                if let Some(ref hash) = hash_opt {
                    let cache_path = std::path::PathBuf::from("audit")
                        .join("font_analysis_cache")
                        .join(format!("{hash}.json"));
                    if let Ok(raw) = std::fs::read_to_string(&cache_path) {
                        if let Ok(analysis) =
                            crate::engine::font_analysis::FontAnalysis::from_json(&raw)
                        {
                            tracing::info!("[font-analysis] cache hit for {}", hash);
                            let _ = res_tx_fonts.send(JobResult::FontAnalysisReady(analysis));
                            return;
                        }
                    }
                }

                let (reply_tx, reply_rx) = oneshot::channel();
                if py_tx_for_fonts
                    .send((
                        PythonJob::AnalyzeFonts {
                            pdf_path: path_for_fonts.to_string_lossy().to_string(),
                        },
                        reply_tx,
                    ))
                    .is_ok()
                {
                    if let Ok(PythonJobResult::Json(json)) = reply_rx.await {
                        match crate::engine::font_analysis::FontAnalysis::from_json(&json) {
                            Ok(analysis) => {
                                // Write the cache entry for next time.
                                if let Some(hash) = hash_opt.as_ref() {
                                    let cache_dir = std::path::PathBuf::from("audit")
                                        .join("font_analysis_cache");
                                    let _ = std::fs::create_dir_all(&cache_dir);
                                    let cache_path = cache_dir.join(format!("{hash}.json"));
                                    // Atomic file operation: write to .tmp and rename
                                    let tmp_path = cache_path.with_extension("tmp");
                                    if std::fs::write(&tmp_path, &json).is_ok() {
                                        let _ = std::fs::rename(tmp_path, &cache_path);
                                    }
                                }
                                let _ = res_tx_fonts.send(JobResult::FontAnalysisReady(analysis));
                            }
                            Err(e) => {
                                tracing::warn!("[font-analysis] decode failed: {e}");
                            }
                        }
                    }
                }
            });
        }

        Job::AnalyzeFonts { path } => {
            let res_tx = result_tx_clone.clone();
            let py_tx = python_tx_clone.clone();
            tokio::spawn(async move {
                let _ = res_tx.send(JobResult::Progress {
                    label: "Analyzing fonts".to_string(),
                    fraction: 0.1,
                });
                let (reply_tx, reply_rx) = oneshot::channel();
                if py_tx
                    .send((
                        PythonJob::AnalyzeFonts {
                            pdf_path: path.to_string_lossy().to_string(),
                        },
                        reply_tx,
                    ))
                    .is_ok()
                {
                    match reply_rx.await {
                        Ok(PythonJobResult::Json(json)) => {
                            match crate::engine::font_analysis::FontAnalysis::from_json(&json) {
                                Ok(analysis) => {
                                    let _ = res_tx.send(JobResult::FontAnalysisReady(analysis));
                                }
                                Err(e) => {
                                    let _ = res_tx.send(JobResult::Error {
                                        job_label: "analyze_fonts".into(),
                                        message: e,
                                    });
                                }
                            }
                        }
                        Ok(PythonJobResult::Error(msg)) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "analyze_fonts".into(),
                                message: msg,
                            });
                        }
                        _ => {}
                    }
                }
                let _ = res_tx.send(JobResult::Progress {
                    label: "Done".into(),
                    fraction: 1.0,
                });
            });
        }

        Job::RenderPage {
            path,
            page,
            dpi,
            tag,
        } => {
            let res_tx = result_tx_clone.clone();
            let eng = engine_for_tokio.clone();

            let (actual_path, actual_page) = if let Some(map) = &segment_map {
                map.resolve(page)
                    .map(|(idx, p)| (map.segments[idx].path.clone(), p))
                    .unwrap_or((path, page))
            } else {
                (path, page)
            };

            tokio::task::spawn_blocking(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    eng.render_page(&actual_path, actual_page, dpi)
                }));
                match result {
                    Ok(Ok(rendered)) => {
                        let _ = res_tx.send(JobResult::PageRendered {
                            png_bytes: rendered.png_bytes,
                            page,
                            dpi,
                            tag,
                            width_pts: rendered.width_pts,
                            height_pts: rendered.height_pts,
                        });
                    }
                    Ok(Err(e)) => {
                        tracing::error!("[render_page] engine error: {}", e);
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "render_page".into(),
                            message: e.to_string(),
                        });
                    }
                    Err(panic_info) => {
                        let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic_info.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "render_page panicked".to_string()
                        };
                        tracing::error!("[render_page] panic: {}", msg);
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "render_page".into(),
                            message: format!("Render panic: {msg}"),
                        });
                    }
                }
            });
        }

        Job::CompleteFont { .. } => {
            let _ = result_tx_clone.send(JobResult::Error {
                job_label: "complete_font".into(),
                message: "Automatic font completion is disabled because synthesized or donor glyphs are not fidelity-preserving."
                    .into(),
            });
        }

        Job::ExportChangeHistory { output } => {
            let history_clone = history.clone();
            let output_clone = output.clone();
            let res_tx = result_tx_clone.clone();
            tokio::task::spawn_blocking(move || {
                let h = history_clone.lock().map_err(|e| e.to_string())?;
                h.save_to_file(&output_clone).map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(format!("blocking task panicked: {e}")))
            .map(|_| {
                let _ = res_tx.send(JobResult::ChangeHistoryExported { path: output });
            })
            .unwrap_or_else(|e| {
                let _ = res_tx.send(JobResult::Error {
                    job_label: "export_history".into(),
                    message: e,
                });
            });
        }

        Job::McpRenderPage { input, page } => {
            let engine_clone = engine_for_tokio.clone();
            let tx = result_tx_clone.clone();
            tokio::task::spawn_blocking(move || {
                if !input.exists() {
                    let _ = tx.send(JobResult::Error {
                        job_label: "mcp_render_page".into(),
                        message: format!("Input file does not exist: {:?}", input),
                    });
                    return;
                }
                use base64::Engine as _;
                match engine_clone.render_page(&input, page, 150.0) {
                    Ok(rendered) => {
                        let base64_png =
                            base64::engine::general_purpose::STANDARD.encode(&rendered.png_bytes);
                        let _ = tx.send(JobResult::McpRenderComplete { base64_png });
                    }
                    Err(e) => {
                        let _ = tx.send(JobResult::Error {
                            job_label: "mcp_render_page".into(),
                            message: format!("Render failed: {}", e),
                        });
                    }
                }
            });
        }

        Job::LoadHistory { input } => {
            let history_clone = history.clone();
            let res_tx = result_tx_clone.clone();
            tokio::task::spawn_blocking(move || {
                match crate::engine::history::ChangeHistory::load_from_file(&input) {
                    Ok(loaded) => {
                        if let Ok(mut h) = history_clone.lock() {
                            *h = loaded.clone();
                            let _ = res_tx.send(JobResult::HistoryUpdated { history: loaded });
                        } else {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "load_history".into(),
                                message: "history mutex poisoned".into(),
                            });
                        }
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "load_history".into(),
                            message: e.to_string(),
                        });
                    }
                }
            })
            .await
            .unwrap_or(());
        }

        _ => unreachable!("unhandled job in this domain handler"),
    }
}
