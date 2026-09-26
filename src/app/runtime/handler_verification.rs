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
        Job::Verify {
            original,
            edited,
            output_dir,
            intended_edits,
            use_pdfrest,
            pdfrest_key,
            auto_match_dpi,
        } => {
            let _ = result_tx_clone.send(JobResult::Progress {
                label: "Extracting optional financial evidence".to_string(),
                fraction: 0.1,
            });
            let mut provider_gates: Vec<crate::engine::verification::VerificationGate> = Vec::new();
            let mut financial_provider_status =
                crate::engine::verification::VerificationGateStatus::Unavailable;
            let mut financial_provider_message: String;

            #[derive(serde::Deserialize)]
            struct RawTxRow {
                page: usize,
                line_on_page: Option<usize>,
                date: Option<String>,
                raw_text: Option<String>,
                debit: Option<f64>,
                credit: Option<f64>,
                running_balance: Option<f64>,
                bbox: Option<[f32; 4]>,
            }

            fn parse_rows(json: &str, label: &str) -> Result<Vec<RawTxRow>, String> {
                serde_json::from_str(json)
                    .map_err(|error| format!("{label} financial extraction was malformed: {error}"))
            }

            let mut edited_rows: Vec<RawTxRow> = Vec::new();
            let (reply_tx, reply_rx) = oneshot::channel();
            match python_tx_clone.send((
                PythonJob::GetAllTransactions {
                    pdf_path: edited.to_string_lossy().to_string(),
                },
                reply_tx,
            )) {
                Ok(()) => match reply_rx.await {
                    Ok(PythonJobResult::Json(json)) => match parse_rows(&json, "edited PDF") {
                        Ok(rows) => {
                            financial_provider_status =
                                crate::engine::verification::VerificationGateStatus::Passed;
                            financial_provider_message = format!(
                                "optional Python extraction returned {} edited transaction row(s)",
                                rows.len()
                            );
                            edited_rows = rows;
                        }
                        Err(error) => {
                            financial_provider_message = error;
                        }
                    },
                    Ok(PythonJobResult::Error(error)) => {
                        financial_provider_message = format!(
                            "optional edited-PDF financial extraction unavailable: {error}"
                        );
                    }
                    Ok(_) => {
                        financial_provider_message =
                            "optional edited-PDF financial extraction returned an unexpected response"
                                .to_string();
                    }
                    Err(error) => {
                        financial_provider_message = format!(
                            "optional edited-PDF financial extraction channel failed: {error}"
                        );
                    }
                },
                Err(error) => {
                    financial_provider_message =
                        format!("optional Python financial extractor unavailable: {error}");
                }
            }

            let transactions: Vec<crate::engine::model::Transaction> = edited_rows
                .iter()
                .map(|row| crate::engine::model::Transaction {
                    page: row.page,
                    line_on_page: row.line_on_page.unwrap_or(0),
                    date: row.date.clone().unwrap_or_default(),
                    raw_text: row.raw_text.clone().unwrap_or_default(),
                    debit: row.debit.map(crate::engine::model::f64_to_dec),
                    credit: row.credit.map(crate::engine::model::f64_to_dec),
                    running_balance: row.running_balance.map(crate::engine::model::f64_to_dec),
                    bbox: row.bbox,
                    field_bboxes: Default::default(),
                    provenance: crate::engine::model::Provenance::Computed,
                    category: None,
                    canonical: Default::default(),
                })
                .collect();

            let mut expected_final_balance: Option<rust_decimal::Decimal> = None;
            let mut opening_balance = rust_decimal::Decimal::ZERO;
            if !transactions.is_empty() {
                let (reply_tx, reply_rx) = oneshot::channel();
                match python_tx_clone.send((
                    PythonJob::GetAllTransactions {
                        pdf_path: original.to_string_lossy().to_string(),
                    },
                    reply_tx,
                )) {
                    Ok(()) => match reply_rx.await {
                        Ok(PythonJobResult::Json(json)) => {
                            match parse_rows(&json, "original PDF") {
                                Ok(original_rows) if !original_rows.is_empty() => {
                                    if let Some(first) = original_rows.first() {
                                        let balance = first.running_balance.unwrap_or(0.0)
                                            - (first.debit.unwrap_or(0.0)
                                                - first.credit.unwrap_or(0.0));
                                        opening_balance = crate::engine::model::f64_to_dec(balance);
                                    }
                                    expected_final_balance = original_rows
                                        .last()
                                        .and_then(|row| row.running_balance)
                                        .map(crate::engine::model::f64_to_dec);
                                    financial_provider_message.push_str(&format!(
                                        "; original baseline returned {} row(s)",
                                        original_rows.len()
                                    ));
                                }
                                Ok(_) => {
                                    financial_provider_status =
                                        crate::engine::verification::VerificationGateStatus::Failed;
                                    financial_provider_message =
                                    "optional financial provider returned no original baseline rows"
                                        .to_string();
                                }
                                Err(error) => {
                                    financial_provider_status =
                                    crate::engine::verification::VerificationGateStatus::Unavailable;
                                    financial_provider_message = error;
                                }
                            }
                        }
                        Ok(PythonJobResult::Error(error)) => {
                            financial_provider_status =
                                crate::engine::verification::VerificationGateStatus::Unavailable;
                            financial_provider_message =
                                format!("original-PDF financial baseline unavailable: {error}");
                        }
                        Ok(_) => {
                            financial_provider_status =
                                crate::engine::verification::VerificationGateStatus::Unavailable;
                            financial_provider_message =
                                "original-PDF financial baseline returned an unexpected response"
                                    .to_string();
                        }
                        Err(error) => {
                            financial_provider_status =
                                crate::engine::verification::VerificationGateStatus::Unavailable;
                            financial_provider_message =
                                format!("original-PDF financial baseline channel failed: {error}");
                        }
                    },
                    Err(error) => {
                        financial_provider_status =
                            crate::engine::verification::VerificationGateStatus::Unavailable;
                        financial_provider_message =
                            format!("original-PDF financial baseline unavailable: {error}");
                    }
                }
            }
            provider_gates.push(crate::engine::verification::VerificationGate::optional(
                "provider.python_financial_extraction",
                financial_provider_status,
                financial_provider_message,
            ));

            // Optional pdfRest rendering is additive evidence only. It can never
            // weaken or replace the mandatory local Pdfium gates.
            let (mut pdfrest_status, mut pdfrest_message) = (
                crate::engine::verification::VerificationGateStatus::Unavailable,
                "optional pdfRest provider was not requested".to_string(),
            );
            if use_pdfrest {
                if let Some(ref key) = pdfrest_key {
                    let _ = result_tx_clone.send(JobResult::Progress {
                        label: "Rendering via optional pdfRest provider".to_string(),
                        fraction: 0.4,
                    });
                    let client = crate::ai::pdfrest::PdfRestClient::new(key.clone());
                    let pdfrest_dir = output_dir.join("pdfrest_renders");
                    match client
                        .render_pdf_to_images(&original, &pdfrest_dir.join("original"), 300)
                        .await
                    {
                        Ok(original_images) => match client
                            .render_pdf_to_images(&edited, &pdfrest_dir.join("edited"), 300)
                            .await
                        {
                            Ok(edited_images)
                                if !original_images.is_empty()
                                    && original_images.len() == edited_images.len() =>
                            {
                                pdfrest_status =
                                    crate::engine::verification::VerificationGateStatus::Passed;
                                pdfrest_message = format!(
                                    "optional pdfRest rendered {} matching page pair(s)",
                                    original_images.len()
                                );
                            }
                            Ok(edited_images) => {
                                pdfrest_status =
                                    crate::engine::verification::VerificationGateStatus::Failed;
                                pdfrest_message = format!(
                                    "optional pdfRest page-count disagreement: original={}, edited={}",
                                    original_images.len(),
                                    edited_images.len()
                                );
                            }
                            Err(error) => {
                                pdfrest_message =
                                    format!("optional pdfRest edited render unavailable: {error}");
                            }
                        },
                        Err(error) => {
                            pdfrest_message =
                                format!("optional pdfRest original render unavailable: {error}");
                        }
                    }
                } else {
                    pdfrest_message =
                        "optional pdfRest was requested without a configured key".to_string();
                }
            }
            provider_gates.push(crate::engine::verification::VerificationGate::optional(
                "provider.pdfrest",
                pdfrest_status,
                pdfrest_message,
            ));

            let _ = result_tx_clone.send(JobResult::Progress {
                label: "Rendering and comparing pages".to_string(),
                fraction: 0.5,
            });
            let math_inputs = crate::engine::verification::MathInputs {
                required: !transactions.is_empty() || expected_final_balance.is_some(),
                transactions,
                expected_transactions: None,
                opening_balance,
                expected_final_balance,
            };
            match crate::engine::verification::verify_edit_with_intents_and_gates(
                &original,
                &edited,
                &output_dir,
                &intended_edits,
                &provider_gates,
                math_inputs,
                auto_match_dpi,
                config_for_tokio.vision_api_key.clone(),
            )
            .await
            {
                Ok(report) => {
                    let _ = result_tx_clone.send(JobResult::VerificationReport(report));
                    let _ = result_tx_clone.send(JobResult::Progress {
                        label: "Done".to_string(),
                        fraction: 1.0,
                    });
                }
                Err(error) => {
                    let _ = result_tx_clone.send(JobResult::Error {
                        job_label: "verify".into(),
                        message: error.to_string(),
                    });
                }
            }
        }

        _ => unreachable!("unhandled job in this domain handler"),
    }
}
