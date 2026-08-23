// Python operations run only through the supervised versioned worker process.
use crate::app::audit::AuditLog;
use crate::engine::history::{ChangeHistory, ChangeRecord};
use crate::engine::segments::{GlobalEdit, SegmentManager, SegmentMap};
use crate::pdf::engine::PdfEngine;
use crate::pdf::ReplaceOutcome;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

/// Opaque per-job handle. The runtime returns one when a job is enqueued;
/// callers can later `Job::Cancel` it.

use super::*;
use crate::app::runtime::client::*;
use crate::app::runtime::jobs::*;
use crate::app::runtime::tracking::*;
use crate::app::runtime::core::*;

#[derive(Debug, Clone)]
pub enum PythonJob {
    Ping,
    GetTextBlocks {
        pdf_path: String,
        page_num: usize,
    },
    ReplaceTextInRect {
        pdf_path: String,
        output_path: String,
        page_num: usize,
        rect: [f32; 4],
        old_text: String,
        new_text: String,
        font_path: Option<String>,
    },
    FindTextBlockAtClick {
        pdf_path: String,
        page_num: usize,
        x: f32,
        y: f32,
    },
    GetAllTransactions {
        pdf_path: String,
    },
    AnalyzeDocumentLayout {
        pdf_path: String,
    },
    CompleteFontWithAdaption {
        pdf_path: String,
        font_name: String,
    },
    DeepFontReplication {
        pdf_path: String,
        font_name: String,
        output_dir: String,
    },
    /// Stage 3 / Item #14: apply N edits in one open/save pass.
    /// `edits_json` is a JSON array of `{page, rect, old_text, new_text, fill_color?}`.
    ApplyManyEdits {
        pdf_path: String,
        output_path: String,
        edits_json: String,
        font_path: Option<String>,
    },
    /// Stage 3 / Item #16: split a PDF into chunks <= 30 pages so Document AI
    /// can parse documents above its single-request page cap.
    ChunkPdfForDocai {
        pdf_path: String,
        output_dir: String,
        max_pages_per_chunk: usize,
    },
    /// Stage 8.5: per-font usage + coverage analysis. Returns the JSON
    /// shape produced by `pymupdf_pro_integration.analyze_fonts`.
    AnalyzeFonts {
        pdf_path: String,
    },
    /// Stage 11: targeted font cascade. Runs composite synthesis ->
    /// subset extension -> Gemini Vision donor identification on the
    /// supplied `missing_chars`. Returns the JSON dict produced by
    /// `replicate_font_for_chars`.
    ReplicateFontForMissingChars {
        pdf_path: String,
        font_name: String,
        missing_chars_csv: String,
        output_dir: String,
    },
    /// Clone (duplicate) pages within a PDF to create capacity for more
    /// transactions. Each entry in `page_indices` is a source page to clone;
    /// clones are inserted immediately after the original. Does NOT require
    /// PyMuPDF Pro - page-level operations use the free tier.
    ClonePages {
        pdf_path: String,
        output_path: String,
        page_indices: Vec<usize>,
    },
    /// Remove pages from a PDF (excess capacity). Pages are deleted in
    /// descending order so indices don't shift. Does NOT require PyMuPDF Pro.
    RemovePages {
        pdf_path: String,
        output_path: String,
        page_indices: Vec<usize>,
    },
    RenderPageToPng {
        pdf_path: String,
        page_num: usize,
        dpi: f32,
    },
    GenerateVisualProof {
        pdf_path: String,
        output_path: String,
        edits_json: String,
    },
}

#[derive(Debug)]
pub enum PythonJobResult {
    Pong,
    Json(String),
    ApplyReport(crate::ai::apply_report::ApplyReport),
    ReplacedWithReviewWarning { reason: String },
    Success,
    Error(String),
}

impl PythonJob {
    pub(crate) fn to_worker_request(
        &self,
    ) -> Result<crate::ai::python_protocol::PythonRequestEnvelope, String> {
        use crate::ai::python_protocol::{PythonOperation, PythonRequestEnvelope};
        use serde_json::json;

        let (operation, input_path, payload) = match self {
            Self::Ping => (PythonOperation::Ping, None, json!({})),
            Self::GetTextBlocks { pdf_path, page_num } => (
                PythonOperation::GetTextBlocks,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path, "page_num": page_num}),
            ),
            Self::ReplaceTextInRect {
                pdf_path,
                output_path,
                page_num,
                rect,
                old_text,
                new_text,
                font_path,
            } => (
                PythonOperation::ReplaceTextInRect,
                Some(pdf_path.as_str()),
                json!({
                    "pdf_path": pdf_path,
                    "output_path": output_path,
                    "page_num": page_num,
                    "rect": rect,
                    "old_text": old_text,
                    "new_text": new_text,
                    "font_path": font_path,
                }),
            ),
            Self::FindTextBlockAtClick {
                pdf_path,
                page_num,
                x,
                y,
            } => (
                PythonOperation::FindTextBlockAtClick,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path, "page_num": page_num, "x": x, "y": y}),
            ),
            Self::GetAllTransactions { pdf_path } => (
                PythonOperation::GetAllTransactions,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path}),
            ),
            Self::AnalyzeDocumentLayout { pdf_path } => (
                PythonOperation::AnalyzeDocumentLayout,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path}),
            ),
            Self::CompleteFontWithAdaption {
                pdf_path,
                font_name,
            } => (
                PythonOperation::CompleteFontWithAdaption,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path, "font_name": font_name}),
            ),
            Self::DeepFontReplication {
                pdf_path,
                font_name,
                output_dir,
            } => (
                PythonOperation::DeepFontReplication,
                Some(pdf_path.as_str()),
                json!({
                    "pdf_path": pdf_path,
                    "font_name": font_name,
                    "output_dir": output_dir,
                }),
            ),
            Self::ApplyManyEdits {
                pdf_path,
                output_path,
                edits_json,
                font_path,
            } => {
                let edits: serde_json::Value = serde_json::from_str(edits_json)
                    .map_err(|error| format!("invalid edit payload: {error}"))?;
                (
                    PythonOperation::ApplyManyEdits,
                    Some(pdf_path.as_str()),
                    json!({
                        "pdf_path": pdf_path,
                        "output_path": output_path,
                        "edits": edits,
                        "font_path": font_path,
                    }),
                )
            }
            Self::ChunkPdfForDocai {
                pdf_path,
                output_dir,
                max_pages_per_chunk,
            } => (
                PythonOperation::ChunkPdfForDocai,
                Some(pdf_path.as_str()),
                json!({
                    "pdf_path": pdf_path,
                    "output_dir": output_dir,
                    "max_pages_per_chunk": max_pages_per_chunk,
                }),
            ),
            Self::AnalyzeFonts { pdf_path } => (
                PythonOperation::AnalyzeFonts,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path}),
            ),
            Self::ReplicateFontForMissingChars {
                pdf_path,
                font_name,
                missing_chars_csv,
                output_dir,
            } => (
                PythonOperation::ReplicateFontForMissingChars,
                Some(pdf_path.as_str()),
                json!({
                    "pdf_path": pdf_path,
                    "font_name": font_name,
                    "missing_chars": missing_chars_csv
                        .split(',')
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>(),
                    "output_dir": output_dir,
                }),
            ),
            Self::ClonePages {
                pdf_path,
                output_path,
                page_indices,
            } => (
                PythonOperation::ClonePages,
                Some(pdf_path.as_str()),
                json!({
                    "pdf_path": pdf_path,
                    "output_path": output_path,
                    "page_indices": page_indices,
                }),
            ),
            Self::RemovePages {
                pdf_path,
                output_path,
                page_indices,
            } => (
                PythonOperation::RemovePages,
                Some(pdf_path.as_str()),
                json!({
                    "pdf_path": pdf_path,
                    "output_path": output_path,
                    "page_indices": page_indices,
                }),
            ),
            Self::RenderPageToPng {
                pdf_path,
                page_num,
                dpi,
            } => (
                PythonOperation::RenderPageToPng,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path, "page_num": page_num, "dpi": dpi}),
            ),
            Self::GenerateVisualProof {
                pdf_path,
                output_path,
                edits_json,
            } => (
                PythonOperation::GenerateVisualProof,
                Some(pdf_path.as_str()),
                json!({"pdf_path": pdf_path, "output_path": output_path, "edits_json": edits_json}),
            ),
        };

        let submitted_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis() as u64;
        let input_sha256 = input_path.map(python_input_sha256).transpose()?;
        PythonRequestEnvelope::new(
            operation,
            Uuid::new_v4(),
            submitted_at_unix_ms,
            submitted_at_unix_ms + 120_000,
            input_sha256,
            payload,
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn worker_response_to_legacy(
        &self,
        response: crate::ai::python_protocol::PythonResponseEnvelope,
    ) -> PythonJobResult {
        use crate::ai::python_protocol::PythonDisposition;

        let result = response
            .payload
            .get("result")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Self::ApplyManyEdits { edits_json, .. } = self {
            let expected = serde_json::from_str::<Vec<serde_json::Value>>(edits_json)
                .map(|edits| edits.len())
                .unwrap_or_default();
            if let Ok(report) =
                crate::ai::apply_report::ApplyReport::from_json_exact(&result.to_string(), expected)
            {
                return PythonJobResult::ApplyReport(report);
            }
        }

        if response.disposition != PythonDisposition::Succeeded {
            let detail = response
                .failure
                .as_ref()
                .map(|failure| format!("{}: {}", failure.code, failure.message))
                .or_else(|| {
                    response
                        .warnings
                        .first()
                        .map(|warning| format!("{}: {}", warning.code, warning.message))
                })
                .unwrap_or_else(|| format!("{:?}", response.disposition));
            return PythonJobResult::Error(detail);
        }

        if matches!(self, Self::Ping) {
            return PythonJobResult::Pong;
        }
        match self {
            Self::ApplyManyEdits { edits_json, .. } => {
                let expected = serde_json::from_str::<Vec<serde_json::Value>>(edits_json)
                    .map(|edits| edits.len())
                    .unwrap_or_default();
                match crate::ai::apply_report::ApplyReport::from_json_exact(
                    &result.to_string(),
                    expected,
                ) {
                    Ok(report) => PythonJobResult::ApplyReport(report),
                    Err(error) => PythonJobResult::Error(error.to_string()),
                }
            }
            Self::ReplaceTextInRect { .. } if !response.warnings.is_empty() => {
                PythonJobResult::ReplacedWithReviewWarning {
                    reason: response
                        .warnings
                        .iter()
                        .map(|warning| warning.message.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                }
            }
            Self::ReplaceTextInRect { .. } => PythonJobResult::Success,
            _ => match serde_json::to_string(&result) {
                Ok(json) => PythonJobResult::Json(json),
                Err(error) => PythonJobResult::Error(error.to_string()),
            },
        }
    }
}

