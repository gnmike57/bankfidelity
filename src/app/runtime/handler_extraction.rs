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
        Job::ExplainImbalance {
            transactions_json,
            opening_balance,
            closing_balance,
            imbalance,
        } => {
            let client = crate::ai::local_llm::LocalLlmClient::new();
            let model = client.model.clone();
            let result_tx = result_tx_clone.clone();
            tokio::spawn(async move {
                let _ = result_tx.send(JobResult::Progress {
                    label: format!("Asking local model ({model}) to explain the math error..."),
                    fraction: 0.1,
                });
                match client
                    .explain_imbalance(
                        &transactions_json,
                        opening_balance,
                        closing_balance,
                        imbalance,
                    )
                    .await
                {
                    Ok(explanation) => {
                        let _ = result_tx.send(JobResult::ImbalanceExplained { explanation });
                    }
                    Err(e) => {
                        let _ = result_tx.send(JobResult::Error {
                            job_label: "explain_imbalance".into(),
                            message: format!("Local LLM Error: {e}"),
                        });
                    }
                }
            });
        }

        Job::NaturalLanguageEdit {
            prompt,
            transactions,
        } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();

            tokio::spawn(async move {
                let _ = res_tx.send(JobResult::Progress {
                    label: "Asking AI to apply edits...".into(),
                    fraction: 0.2,
                });

                let ai_backend =
                    match crate::ai::backend::AiBackend::from_app_config_async(&cfg).await {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "NaturalLanguageEdit".into(),
                                message: format!("AI provider unavailable: {e}"),
                            });
                            return;
                        }
                    };

                match ai_backend
                    .apply_natural_language_edit(&prompt, &transactions)
                    .await
                {
                    Ok(updated) => {
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Edits applied successfully!".into(),
                            fraction: 1.0,
                        });
                        let _ = res_tx.send(JobResult::NaturalLanguageEditReady(updated));
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "NaturalLanguageEdit".into(),
                            message: format!("Failed to apply edits: {e}"),
                        });
                    }
                }
            });
        }

        Job::CategorizeTransactions { mut transactions } => {
            let res_tx = result_tx_clone.clone();
            tokio::spawn(async move {
                crate::engine::categorization::categorize_transactions(&mut transactions);
                let _ = res_tx.send(JobResult::CategorizationReady(transactions));
            });
        }

        Job::ExtractTransactions { path, parser_mode } => {
            let res_tx = result_tx_clone.clone();
            let eng = engine_for_tokio.clone();
            let cfg = config_for_tokio.clone();
            let semaphore = api_semaphore.clone();
            let cache_for_job = parse_cache.clone();

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

                let source_hash = match tokio::fs::read(&path).await {
                    Ok(bytes) => crate::engine::workflow::sha256_hex_of(&bytes),
                    Err(_) => path.to_string_lossy().to_string(),
                };
                let cache_key = format!("{parser_mode:?}:{source_hash}");

                {
                    let mut cache = cache_for_job.lock().await;
                    if let Some(mut cached_stmt) = cache.get(&cache_key).cloned() {
                        cached_stmt.ensure_canonical_metadata();
                        let issues = crate::engine::workflow::deterministic_parse_issues(
                            cached_stmt.total_pages,
                            &cached_stmt.transactions,
                            cached_stmt.opening_balance,
                            cached_stmt.closing_balance,
                        );
                        if issues.is_empty() {
                            tracing::info!(
                                "[runtime] validated extraction cache hit: {}",
                                cache_key
                            );
                            let _ = res_tx
                                .send(JobResult::TransactionsExtracted(cached_stmt.transactions));
                            return;
                        }
                        tracing::warn!(
                            "[runtime] ignoring invalid extraction cache entry {}: {}",
                            cache_key,
                            issues.join("; ")
                        );
                    }
                }

                let _ = res_tx.send(JobResult::Progress {
                    label: "Extracting transactions".to_string(),
                    fraction: 0.1,
                });

                let provider_order = extraction_provider_order(parser_mode);
                let mut failures = Vec::new();
                let mut accepted_statement = None;

                for (attempt_index, provider) in provider_order.into_iter().enumerate() {
                    let _ = res_tx.send(JobResult::Progress {
                        label: format!("Extracting with {}", provider.label()),
                        fraction: 0.15 + attempt_index as f32 * 0.15,
                    });

                    let attempt: Result<crate::ai::document_ai::BankStatement, String> =
                        match provider {
                            crate::app::config::DocumentParserMode::Reducto => {
                                if let Ok(client) = crate::ai::reducto::ReductoClient::from_app_config(&cfg) {
                                    // Owned handles so the future is 'static: the
                                    // blocking helper may run it on a scratch thread.
                                    let client = std::sync::Arc::new(client);
                                    let p = path.clone();
                                    block_on_from_blocking_context(async move {
                                        client.parse_statement(&p).await
                                    })
                                    .map_err(|e| e.to_string())
                                } else {
                                    Err("Reducto client init failed".into())
                                }
                            }
                            crate::app::config::DocumentParserMode::LlamaParse => {
                                match crate::ai::llamaparse::LlamaParseClient::from_app_config(&cfg) {
                                    Ok(client) => crate::engine::pro_edit::perform_pro_edit(
                                        "LlamaParse",
                                        async {
                                            client
                                                .parse_statement(&path)
                                                .await
                                                .map_err(anyhow::Error::from)
                                        },
                                        wdog.clone(),
                                    )
                                    .await
                                    .map_err(|error| error.to_string()),
                                    Err(error) => Err(error.to_string()),
                                }
                            }
                            crate::app::config::DocumentParserMode::DocumentAi => {
                                match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg)
                                {
                                    Ok(client) => {
                                        let client = Arc::new(client);
                                        crate::engine::pro_edit::perform_pro_edit(
                                            "DocumentAI",
                                            async {
                                                client
                                                    .parse_entire_statement(&path, None::<&str>)
                                                    .await
                                                    .map_err(anyhow::Error::from)
                                            },
                                            wdog.clone(),
                                        )
                                        .await
                                        .map_err(|error| error.to_string())
                                    }
                                    Err(error) => Err(error.to_string()),
                                }
                            }
                            crate::app::config::DocumentParserMode::OfflineHeuristic => {
                                let engine = eng.clone();
                                let input = path.clone();
                                match tokio::task::spawn_blocking(move || {
                                    crate::engine::offline_parser::parse_statement_offline(
                                        &input, engine,
                                    )
                                })
                                .await
                                {
                                    Ok(result) => result,
                                    Err(error) => {
                                        Err(format!("offline parser task failed: {error}"))
                                    }
                                }
                            }
                            crate::app::config::DocumentParserMode::LocalOcrs => Err(
                                "Local OCR PDF parsing is not supported in v1; select Offline Heuristic"
                                    .to_string(),
                            ),
                        };

                    let mut statement = match attempt {
                        Ok(statement) => statement,
                        Err(error) => {
                            failures.push(format!("{}: {error}", provider.label()));
                            continue;
                        }
                    };
                    statement.ensure_canonical_metadata();

                    let template_provider = Arc::new(crate::extractors::BankTemplateProvider::new(
                        crate::app::paths::resolve_asset_path("bank_templates").as_path(),
                        eng.clone(),
                    ));
                    let merger = crate::extractors::HybridMerger::new(vec![
                        template_provider as Arc<dyn crate::extractors::GeometryProvider>,
                    ]);
                    let input = path.clone();
                    let transactions = std::mem::take(&mut statement.transactions);
                    let report = match tokio::task::spawn_blocking(move || {
                        let mut geometries = Vec::new();
                        for geometry_provider in &merger.providers {
                            if let Ok(geometry) = geometry_provider.extract_line_geometry(&input) {
                                geometries.extend(geometry);
                            }
                        }
                        merger.merge(transactions, geometries)
                    })
                    .await
                    {
                        Ok(report) => report,
                        Err(error) => {
                            failures.push(format!(
                                "{} geometry merge failed: {error}",
                                provider.label()
                            ));
                            continue;
                        }
                    };
                    statement.transactions = report.transactions;
                    statement.ensure_canonical_metadata();

                    let issues = crate::engine::workflow::deterministic_parse_issues(
                        statement.total_pages,
                        &statement.transactions,
                        statement.opening_balance,
                        statement.closing_balance,
                    );
                    if issues.is_empty() {
                        accepted_statement = Some(statement);
                        break;
                    }
                    failures.push(format!(
                        "{} output rejected: {}",
                        provider.label(),
                        issues.join("; ")
                    ));
                }

                let statement = match accepted_statement {
                    Some(statement) => statement,
                    None => {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "extract_transactions".into(),
                            message: format!(
                                "Extraction incomplete: no transaction rows passed deterministic validation. {}",
                                failures.join(" | ")
                            ),
                        });
                        return;
                    }
                };

                {
                    let mut cache = cache_for_job.lock().await;
                    cache.put(cache_key, statement.clone());
                }
                let _ = res_tx.send(JobResult::TransactionsExtracted(statement.transactions));
            });
        }

        Job::BalanceStatement { path } => {
            let res_tx = result_tx_clone.clone();
            let eng = engine_for_tokio.clone();
            let cfg = config_for_tokio.clone();
            let semaphore = api_semaphore.clone();
            let cache_for_job = parse_cache.clone();

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

                let cache_key = match tokio::fs::read(&path).await {
                    Ok(bytes) => crate::engine::workflow::sha256_hex_of(&bytes),
                    Err(_) => path.to_string_lossy().to_string(),
                };

                let _ = res_tx.send(JobResult::Progress {
                    label: "Smart Balance Analysis".to_string(),
                    fraction: 0.1,
                });

                let doc_ai = crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg)
                    .ok()
                    .map(Arc::new);
                let gemini = crate::ai::backend::AiBackend::from_app_config(&cfg)
                    .ok()
                    .map(Arc::new);

                // If both AI services are available, use the full smart engine
                if let (Some(doc_ai), Some(gemini)) = (doc_ai, gemini) {
                    let template_provider = Arc::new(crate::extractors::BankTemplateProvider::new(
                        crate::app::paths::resolve_asset_path("bank_templates").as_path(),
                        eng.clone(),
                    ));

                    let merger = Arc::new(crate::extractors::HybridMerger::new(vec![
                        template_provider as Arc<dyn crate::extractors::GeometryProvider>,
                    ]));

                    let mut smart_engine = crate::engine::statement::SmartDocumentEngine::new(
                        eng.clone(),
                        doc_ai,
                        gemini,
                        merger,
                    );

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Loading Document".to_string(),
                        fraction: 0.3,
                    });

                    let (dummy_tx, _) = std::sync::mpsc::channel();
                    if let Err(e) = smart_engine.load_full_document(&dummy_tx, &path).await {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "balance_statement".into(),
                            message: format!("Failed to load document: {e}"),
                        });
                        return;
                    }

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Analyzing layout and semantic meaning".to_string(),
                        fraction: 0.6,
                    });

                    match smart_engine.balance_entire_statement(&path).await {
                        Ok(changes) => {
                            let imbalance = smart_engine.calculate_global_imbalance();
                            let _ = res_tx.send(JobResult::BalanceProposed { imbalance, changes });
                            let _ = res_tx.send(JobResult::Progress {
                                label: "Done".to_string(),
                                fraction: 1.0,
                            });
                        }
                        Err(crate::engine::statement::EngineError::LowConfidence(c)) => {
                            let _ = res_tx.send(JobResult::Error { job_label: "balance_statement".into(), message: format!("Gemini confidence {c:.2} below 0.7 threshold; not enough certainty to propose adjustments.") });
                        }
                        Err(e) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "balance_statement".into(),
                                message: e.to_string(),
                            });
                        }
                    }
                } else {
                    // -- Offline fallback: local balance analysis ---------€
                    tracing::info!(
                        "[balance] AI services not configured; using offline balance analysis"
                    );
                    let _ = res_tx.send(JobResult::Progress {
                        label: "Using offline balance analysis (no AI)...".to_string(),
                        fraction: 0.3,
                    });

                    let eng_clone = eng.clone();
                    let path_clone = path.clone();
                    let stmt = if let Some(cached_stmt) = {
                        let mut cache = cache_for_job.lock().await;
                        cache.get(&cache_key).cloned()
                    } {
                        tracing::info!(
                            "[runtime] LRU cache HIT for BalanceStatement offline path: {}",
                            cache_key
                        );
                        cached_stmt
                    } else {
                        let stmt_res = match tokio::task::spawn_blocking(move || {
                            crate::engine::offline_parser::parse_statement_offline(
                                &path_clone,
                                eng_clone,
                            )
                        })
                        .await
                        {
                            Ok(Ok(s)) => s,
                            Ok(Err(e)) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "balance_statement".into(),
                                    message: format!("Offline balance analysis failed: {e}"),
                                });
                                return;
                            }
                            Err(e) => {
                                let _ = res_tx.send(JobResult::Error {
                                    job_label: "balance_statement".into(),
                                    message: format!("Offline balance panicked: {e}"),
                                });
                                return;
                            }
                        };

                        {
                            let mut cache = cache_for_job.lock().await;
                            cache.put(cache_key.clone(), stmt_res.clone());
                        }
                        stmt_res
                    };

                    if stmt.transactions.is_empty() {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "balance_statement".into(),
                            message: "Balance analysis incomplete: no transaction rows were found. The statement cannot be declared balanced."
                                .into(),
                        });
                        return;
                    }

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Computing balance chain locally...".to_string(),
                        fraction: 0.6,
                    });

                    // Compute running balance chain from offline-parsed transactions
                    let mut changes = Vec::new();
                    let mut running = stmt.opening_balance;
                    for tx in &stmt.transactions {
                        let net = tx.debit.unwrap_or(rust_decimal::Decimal::ZERO)
                            - tx.credit.unwrap_or(rust_decimal::Decimal::ZERO);
                        running += net;
                        if let Some(printed_bal) = tx.running_balance {
                            if (running - printed_bal).abs() > rust_decimal_macros::dec!(0.01) {
                                changes.push(crate::engine::model::ProposedChange {
                                                page: tx.page,
                                                old_text: format!("{printed_bal}"),
                                                new_text: format!("{running}"),
                                                reason: format!("Computed balance {running} differs from printed {printed_bal}"),
                                                confidence: 0.6,
                                                affects_subsequent_balances: true,
                                                bbox: tx
                                                    .field_bboxes
                                                    .running_balance
                                                    .or(tx.bbox),
                                            });
                            }
                        }
                    }

                    let imbalance = (running - stmt.closing_balance).abs();
                    let _ = res_tx.send(JobResult::BalanceProposed { imbalance, changes });
                    let _ = res_tx.send(JobResult::Progress {
                        label: "Done (offline mode)".to_string(),
                        fraction: 1.0,
                    });
                }
            });
        }

        Job::BalanceAndApplyAll {
            input,
            output: _,
            auto_apply,
        } => {
            let res_tx = TerminalTracker::new(result_tx_clone.clone(), "BalanceAndApplyAll");
            let eng = engine_for_tokio.clone();
            let cfg = config_for_tokio.clone();
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
                let _ = res_tx.send(JobResult::Progress {
                    label: "Adjusting entire statement...".to_string(),
                    fraction: 0.1,
                });

                let doc_ai = crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg)
                    .ok()
                    .map(Arc::new);
                let gemini = crate::ai::backend::AiBackend::from_app_config_async(&cfg)
                    .await
                    .ok()
                    .map(Arc::new);

                if let (Some(doc_ai), Some(gemini)) = (doc_ai, gemini) {
                    // -- Online: full smart engine ----------------------
                    let template_provider = Arc::new(crate::extractors::BankTemplateProvider::new(
                        crate::app::paths::resolve_asset_path("bank_templates").as_path(),
                        eng.clone(),
                    ));
                    let merger = Arc::new(crate::extractors::HybridMerger::new(vec![
                        template_provider as Arc<dyn crate::extractors::GeometryProvider>,
                    ]));

                    let mut smart_engine = crate::engine::statement::SmartDocumentEngine::new(
                        eng.clone(),
                        doc_ai,
                        gemini,
                        merger,
                    );

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Loading document".to_string(),
                        fraction: 0.3,
                    });
                    let (dummy_tx, _) = std::sync::mpsc::channel();
                    if let Err(e) = smart_engine.load_full_document(&dummy_tx, &input).await {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "balance_and_apply_all".into(),
                            message: format!("Failed to load document: {e}"),
                        });
                        return;
                    }

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Computing balanced adjustments".to_string(),
                        fraction: 0.6,
                    });
                    match smart_engine.balance_entire_statement(&input).await {
                        Ok(changes) => {
                            let imbalance = smart_engine.calculate_global_imbalance();
                            let _ = res_tx.send(JobResult::BalanceProposed {
                                imbalance,
                                changes: changes.clone(),
                            });
                            if auto_apply && !changes.is_empty() {
                                let _ = res_tx.send(JobResult::WorkflowStageChanged { stage:
                                                crate::engine::workflow::WorkflowStage::ImbalanceCorrectionWarning {
                                                    imbalance,
                                                    proposed_changes: changes.clone(),
                                                }
                                            });
                            } else if changes.is_empty() {
                                let _ = res_tx.send(JobResult::Progress {
                                    label: "Already balanced - nothing to apply".to_string(),
                                    fraction: 1.0,
                                });
                            }
                            let (disposition, message) = if changes.is_empty() {
                                (
                                    OperationDisposition::NoOp,
                                    "Statement is already balanced; no changes were required",
                                )
                            } else if auto_apply {
                                (
                                    OperationDisposition::Partial,
                                    "Changes were proposed and await explicit confirmation; no output was published",
                                )
                            } else {
                                (
                                    OperationDisposition::Succeeded,
                                    "Balance analysis completed and proposals are ready for review",
                                )
                            };
                            let _ = res_tx.send(JobResult::completed(
                                "balance_and_apply_all",
                                disposition,
                                None,
                                message,
                            ));
                        }
                        Err(crate::engine::statement::EngineError::LowConfidence(c)) => {
                            let _ = res_tx.send(JobResult::Error { job_label: "balance_and_apply_all".into(), message: format!("Gemini confidence {c:.2} below 0.7 threshold; not enough certainty to auto-apply adjustments.") });
                        }
                        Err(e) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "balance_and_apply_all".into(),
                                message: e.to_string(),
                            });
                        }
                    }
                } else {
                    // -- Offline fallback: local balance + optional auto-apply --
                    tracing::info!(
                        "[balance_and_apply_all] AI not configured; using offline balance"
                    );
                    let _ = res_tx.send(JobResult::Progress {
                        label: "Using offline balance analysis (no AI)...".to_string(),
                        fraction: 0.3,
                    });

                    let eng_clone = eng.clone();
                    let path_clone = input.clone();
                    let stmt = match tokio::task::spawn_blocking(move || {
                        crate::engine::offline_parser::parse_statement_offline(
                            &path_clone,
                            eng_clone,
                        )
                    })
                    .await
                    {
                        Ok(Ok(s)) => s,
                        Ok(Err(e)) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "balance_and_apply_all".into(),
                                message: format!("Offline balance analysis failed: {e}"),
                            });
                            return;
                        }
                        Err(e) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "balance_and_apply_all".into(),
                                message: format!("Offline balance panicked: {e}"),
                            });
                            return;
                        }
                    };

                    if stmt.transactions.is_empty() {
                        let _ = res_tx.send(JobResult::Error {
                            job_label: "balance_and_apply_all".into(),
                            message: "Balance analysis incomplete: no transaction rows were found. No changes were proposed or applied."
                                .into(),
                        });
                        return;
                    }

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Computing balance chain locally...".to_string(),
                        fraction: 0.6,
                    });

                    let mut changes = Vec::new();
                    let mut running = stmt.opening_balance;
                    for tx in &stmt.transactions {
                        let net = tx.debit.unwrap_or(rust_decimal::Decimal::ZERO)
                            - tx.credit.unwrap_or(rust_decimal::Decimal::ZERO);
                        running += net;
                        if let Some(printed_bal) = tx.running_balance {
                            if (running - printed_bal).abs() > rust_decimal_macros::dec!(0.01) {
                                changes.push(crate::engine::model::ProposedChange {
                                                page: tx.page,
                                                old_text: format!("{printed_bal}"),
                                                new_text: format!("{running}"),
                                                reason: format!("Computed balance {running} differs from printed {printed_bal}"),
                                                confidence: 0.6,
                                                affects_subsequent_balances: true,
                                                bbox: tx
                                                    .field_bboxes
                                                    .running_balance
                                                    .or(tx.bbox),
                                            });
                            }
                        }
                    }

                    let imbalance = (running - stmt.closing_balance).abs();
                    let _ = res_tx.send(JobResult::BalanceProposed {
                        imbalance,
                        changes: changes.clone(),
                    });

                    if auto_apply && !changes.is_empty() {
                        let _ = res_tx.send(JobResult::WorkflowStageChanged {
                            stage:
                                crate::engine::workflow::WorkflowStage::ImbalanceCorrectionWarning {
                                    imbalance,
                                    proposed_changes: changes.clone(),
                                },
                        });
                    } else if changes.is_empty() {
                        let _ = res_tx.send(JobResult::Progress {
                            label: "Already balanced - nothing to apply (offline)".to_string(),
                            fraction: 1.0,
                        });
                    }
                    let (disposition, message) = if changes.is_empty() {
                        (
                            OperationDisposition::NoOp,
                            "Statement is already balanced; no changes were required",
                        )
                    } else if auto_apply {
                        (
                            OperationDisposition::Partial,
                            "Changes were proposed and await explicit confirmation; no output was published",
                        )
                    } else {
                        (
                            OperationDisposition::Succeeded,
                            "Offline balance analysis completed and proposals are ready for review",
                        )
                    };
                    let _ = res_tx.send(JobResult::completed(
                        "balance_and_apply_all",
                        disposition,
                        None,
                        message,
                    ));
                }
            });
        }

        _ => unreachable!("unhandled job in this domain handler"),
    }
}
