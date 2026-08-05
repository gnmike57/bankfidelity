use super::engine::*;
use crate::app::runtime::{PythonJob, PythonJobResult};
use std::path::Path;
use tokio::sync::oneshot;

/// Receive on a tokio oneshot from a sync context.
///
/// `oneshot::Receiver::blocking_recv` panics when called from a thread that
/// is currently driving a tokio task (which is exactly what
/// `tokio::task::spawn_blocking` produces - the OS thread is owned by the
/// runtime even while it's executing user code). We work around it by:
///   * using `block_in_place` + `Handle::block_on` if a runtime exists, OR
///   * falling back to `blocking_recv` when no runtime is present (CLI sync
///     wait-loop, plain unit tests).
fn recv_blocking<T>(rx: oneshot::Receiver<T>) -> Result<T, oneshot::error::RecvError>
where
    T: Send + 'static,
{
    // PdfEngine methods are synchronous and executed inside `spawn_blocking` threads.
    // Calling `block_in_place` from a blocking thread panics in tokio, so we just
    // use `blocking_recv()` directly which correctly parks the current blocking thread.
    rx.blocking_recv()
}

#[derive(Debug)]
pub struct PyMuPdfEngine {
    job_tx: std::sync::mpsc::Sender<crate::app::runtime::Job>,
}

impl PyMuPdfEngine {
    pub fn new(job_tx: std::sync::mpsc::Sender<crate::app::runtime::Job>) -> Self {
        Self { job_tx }
    }
}

impl PdfEngine for PyMuPdfEngine {
    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            supports_redaction: true,
            supports_cjk: true,
            supports_embedded_fonts: true,
            estimated_fidelity: 0.95,
        }
    }

    fn render_page(&self, path: &Path, page: usize, dpi: f32) -> Result<RenderedPage, EngineError> {
        use base64::Engine as _;

        let (tx, rx) = oneshot::channel();
        self.job_tx
            .send(crate::app::runtime::Job::Python(
                PythonJob::RenderPageToPng {
                    pdf_path: path.to_string_lossy().to_string(),
                    page_num: page,
                    dpi,
                },
                tx,
            ))
            .map_err(|_| EngineError::RenderFailed("Worker thread disconnected".into()))?;

        let res = recv_blocking(rx).map_err(|e| EngineError::RenderFailed(e.to_string()))?;

        match res {
            PythonJobResult::Json(json) => {
                let parsed: serde_json::Value = serde_json::from_str(&json).map_err(|e| {
                    EngineError::RenderFailed(format!("Bad JSON from PyMuPDF render: {e}"))
                })?;
                let png_b64 = parsed["png_base64"].as_str().ok_or_else(|| {
                    EngineError::RenderFailed("Missing png_base64 in response".into())
                })?;
                let width_pts = parsed["width_pts"].as_f64().unwrap_or(612.0) as f32;
                let height_pts = parsed["height_pts"].as_f64().unwrap_or(792.0) as f32;
                let png_bytes = base64::engine::general_purpose::STANDARD
                    .decode(png_b64)
                    .map_err(|e| EngineError::RenderFailed(format!("base64 decode failed: {e}")))?;
                Ok(RenderedPage {
                    png_bytes,
                    width_pts,
                    height_pts,
                })
            }
            PythonJobResult::Error(e) => Err(EngineError::RenderFailed(e)),
            _ => Err(EngineError::RenderFailed(
                "Unexpected result from PyMuPDF render".into(),
            )),
        }
    }

    fn get_text_blocks(&self, path: &Path, page: usize) -> Result<Vec<TextBlock>, EngineError> {
        let (tx, rx) = oneshot::channel();
        self.job_tx
            .send(crate::app::runtime::Job::Python(
                PythonJob::GetTextBlocks {
                    pdf_path: path.to_string_lossy().to_string(),
                    page_num: page,
                },
                tx,
            ))
            .map_err(|_| EngineError::ExtractFailed("Worker thread disconnected".into()))?;

        // This blocks the calling thread (usually a spawn_blocking tokio task)
        let res = recv_blocking(rx).map_err(|e| EngineError::ExtractFailed(e.to_string()))?;

        match res {
            PythonJobResult::Json(json) => {
                let blocks = serde_json::from_str(&json)
                    .map_err(|e| EngineError::ExtractFailed(e.to_string()))?;
                Ok(blocks)
            }
            PythonJobResult::Error(e) => Err(EngineError::ExtractFailed(e)),
            _ => Err(EngineError::ExtractFailed("Unexpected result".into())),
        }
    }

    fn find_text_block_at_click(
        &self,
        path: &Path,
        page: usize,
        x: f32,
        y: f32,
    ) -> Result<Option<TextBlock>, EngineError> {
        let (tx, rx) = oneshot::channel();
        self.job_tx
            .send(crate::app::runtime::Job::Python(
                PythonJob::FindTextBlockAtClick {
                    pdf_path: path.to_string_lossy().to_string(),
                    page_num: page,
                    x,
                    y,
                },
                tx,
            ))
            .map_err(|_| EngineError::ExtractFailed("Worker thread disconnected".into()))?;

        let res = recv_blocking(rx).map_err(|e| EngineError::ExtractFailed(e.to_string()))?;

        match res {
            PythonJobResult::Json(json) => {
                let trimmed = json.trim();
                if trimmed == "null" || trimmed.is_empty() {
                    return Ok(None);
                }
                let block = serde_json::from_str(&json)
                    .map_err(|e| EngineError::ExtractFailed(e.to_string()))?;
                Ok(Some(block))
            }
            PythonJobResult::Error(e) => Err(EngineError::ExtractFailed(e)),
            _ => Ok(None),
        }
    }

    fn apply_change(
        &self,
        input: &Path,
        output: &Path,
        page: usize,
        bbox: [f32; 4],
        new_text: &str,
        old_text: &str,
        font_path: Option<&Path>,
    ) -> Result<ReplaceOutcome, EngineError> {
        let (tx, rx) = oneshot::channel();
        self.job_tx
            .send(crate::app::runtime::Job::Python(
                PythonJob::ReplaceTextInRect {
                    pdf_path: input.to_string_lossy().to_string(),
                    output_path: output.to_string_lossy().to_string(),
                    page_num: page,
                    rect: bbox,
                    old_text: old_text.to_string(),
                    new_text: new_text.to_string(),
                    font_path: font_path.map(|p| p.to_string_lossy().to_string()),
                },
                tx,
            ))
            .map_err(|_| EngineError::ApplyFailed("Worker thread disconnected".into()))?;

        let res = recv_blocking(rx).map_err(|e| EngineError::ApplyFailed(e.to_string()))?;

        match res {
            PythonJobResult::Success => Ok(ReplaceOutcome {
                success: true,
                font_used: "unknown".into(),
                overflow: false,
                obj_id: None,
            }),
            PythonJobResult::ReplacedWithReviewWarning { .. } => Ok(ReplaceOutcome {
                success: true,
                font_used: "unknown".into(),
                overflow: false,
                obj_id: None,
            }),
            PythonJobResult::Error(e) => Err(EngineError::ApplyFailed(e)),
            _ => Err(EngineError::ApplyFailed("Unexpected result".into())),
        }
    }

    fn analyze_layout(&self, path: &Path) -> Result<DocumentLayout, EngineError> {
        let (tx, rx) = oneshot::channel();
        self.job_tx
            .send(crate::app::runtime::Job::Python(
                PythonJob::AnalyzeDocumentLayout {
                    pdf_path: path.to_string_lossy().to_string(),
                },
                tx,
            ))
            .map_err(|_| EngineError::LayoutFailed("Worker thread disconnected".into()))?;

        let res = recv_blocking(rx).map_err(|e| EngineError::LayoutFailed(e.to_string()))?;

        match res {
            PythonJobResult::Json(json) => {
                let pages: Vec<crate::engine::layout::PageLayout> = serde_json::from_str(&json)
                    .map_err(|e| EngineError::LayoutFailed(e.to_string()))?;
                Ok(DocumentLayout {
                    total_pages: pages.len(),
                    has_consistent_headers: pages.iter().all(|p| p.has_header),
                    has_consistent_footers: pages.iter().all(|p| p.has_footer),
                    pages,
                    overall_style: "Professional Bank Statement".to_string(),
                    layout_confidence: 0.85,
                })
            }
            PythonJobResult::Error(e) => Err(EngineError::LayoutFailed(e)),
            _ => Err(EngineError::LayoutFailed("Unexpected result".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::runtime::{Job, PythonJobResult};
    use std::sync::mpsc;

    #[test]
    fn find_text_block_at_click_returns_none_on_null_json() {
        let (job_tx, job_rx) = mpsc::channel();
        let engine = PyMuPdfEngine::new(job_tx);

        std::thread::spawn(move || {
            if let Ok(Job::Python(_, reply_tx)) = job_rx.recv() {
                let _ = reply_tx.send(PythonJobResult::Json("null".into()));
            }
        });

        let res = engine
            .find_text_block_at_click(Path::new("test.pdf"), 0, 0.0, 0.0)
            .unwrap();
        assert!(res.is_none());
    }
}
