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
        Job::ListDocAiVersions => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            tokio::spawn(async move {
                match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                    Ok(client) => match client.list_processor_versions().await {
                        Ok(versions) => {
                            let _ = res_tx.send(JobResult::DocAiVersionsListed(versions));
                        }
                        Err(e) => {
                            let _ = res_tx.send(JobResult::DocAiVersionError(format!(
                                "Failed to list versions: {e}"
                            )));
                        }
                    },
                    Err(e) => {
                        let _ = res_tx.send(JobResult::DocAiVersionError(format!(
                            "DocAI not configured: {e}"
                        )));
                    }
                }
            });
        }

        Job::DeployDocAiVersion { version_id } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            tokio::spawn(async move {
                match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                    Ok(client) => match client.deploy_processor_version(&version_id).await {
                        Ok(op) => {
                            let _ = res_tx.send(JobResult::DocAiVersionOperationStarted {
                                operation_name: op,
                                description: format!("Deploying version {version_id}"),
                            });
                        }
                        Err(e) => {
                            let _ = res_tx
                                .send(JobResult::DocAiVersionError(format!("Deploy failed: {e}")));
                        }
                    },
                    Err(e) => {
                        let _ = res_tx.send(JobResult::DocAiVersionError(format!("{e}")));
                    }
                }
            });
        }

        Job::UndeployDocAiVersion { version_id } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            tokio::spawn(async move {
                match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                    Ok(client) => match client.undeploy_processor_version(&version_id).await {
                        Ok(op) => {
                            let _ = res_tx.send(JobResult::DocAiVersionOperationStarted {
                                operation_name: op,
                                description: format!("Undeploying version {version_id}"),
                            });
                        }
                        Err(e) => {
                            let _ = res_tx.send(JobResult::DocAiVersionError(format!(
                                "Undeploy failed: {e}"
                            )));
                        }
                    },
                    Err(e) => {
                        let _ = res_tx.send(JobResult::DocAiVersionError(format!("{e}")));
                    }
                }
            });
        }

        Job::SetDefaultDocAiVersion { version_id } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            tokio::spawn(async move {
                match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                    Ok(client) => match client.set_default_processor_version(&version_id).await {
                        Ok(op) => {
                            let _ = res_tx.send(JobResult::DocAiVersionOperationStarted {
                                operation_name: op,
                                description: format!("Setting default to {version_id}"),
                            });
                        }
                        Err(e) => {
                            let _ = res_tx.send(JobResult::DocAiVersionError(format!(
                                "Set default failed: {e}"
                            )));
                        }
                    },
                    Err(e) => {
                        let _ = res_tx.send(JobResult::DocAiVersionError(format!("{e}")));
                    }
                }
            });
        }

        Job::TrainDocAiVersion {
            display_name,
            base_version,
        } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            tokio::spawn(async move {
                match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                    Ok(client) => {
                        match client
                            .train_processor_version(&display_name, base_version.as_deref())
                            .await
                        {
                            Ok(op) => {
                                let _ = res_tx.send(JobResult::DocAiVersionOperationStarted {
                                    operation_name: op,
                                    description: format!("Training: {display_name}"),
                                });
                            }
                            Err(e) => {
                                let _ = res_tx.send(JobResult::DocAiVersionError(format!(
                                    "Training failed: {e}"
                                )));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = res_tx.send(JobResult::DocAiVersionError(format!("{e}")));
                    }
                }
            });
        }

        _ => unreachable!("unhandled job in this domain handler"),
    }
}
