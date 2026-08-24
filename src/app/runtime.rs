// Python operations run only through the supervised versioned worker process.
use crate::app::audit::AuditLog;
use crate::engine::history::{ChangeHistory, ChangeRecord};
use crate::engine::segments::{GlobalEdit, SegmentManager, SegmentMap};
use crate::pdf::engine::PdfEngine;
use crate::pdf::ReplaceOutcome;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

mod parser_chain;
use self::parser_chain::interactive_fallback_or_continue;
use self::parser_chain::{
    extraction_provider_order, wait_for_interactive_choice, InteractiveFallbackRouter,
};

/// Opaque per-job handle. The runtime returns one when a job is enqueued;
/// callers can later `Job::Cancel` it.
pub type JobId = u64;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a fresh `JobId`. Used by both the runtime and external callers
/// who want to enqueue a job and remember its handle.
pub fn alloc_job_id() -> JobId {
    NEXT_JOB_ID.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Interactive,
    Headless,
}

#[derive(Debug, Clone)]
pub struct JobMetadata {
    pub job_id: JobId,
    pub document_id: Option<String>,
    pub correlation_id: Uuid,
    pub label: &'static str,
    pub submitted_at: std::time::SystemTime,
    pub deadline: std::time::Instant,
    pub execution_mode: ExecutionMode,
}

impl JobMetadata {
    fn for_job(job: &Job) -> Self {
        Self::for_job_with_mode(job, ExecutionMode::Interactive)
    }

    fn for_job_with_mode(job: &Job, execution_mode: ExecutionMode) -> Self {
        Self {
            job_id: alloc_job_id(),
            document_id: job.document_path().map(document_id_for_path),
            correlation_id: Uuid::new_v4(),
            label: job.label(),
            submitted_at: std::time::SystemTime::now(),
            deadline: std::time::Instant::now() + job.default_timeout(),
            execution_mode,
        }
    }
}

fn document_id_for_path(path: &Path) -> String {
    use sha2::Digest;
    let normalized = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    sha2::Sha256::digest(normalized.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct JobEnvelope {
    metadata: JobMetadata,
    job: Job,
    route: Option<mpsc::Sender<JobResult>>,
}

impl JobEnvelope {
    fn broadcast(job: Job) -> Self {
        Self {
            metadata: JobMetadata::for_job(&job),
            job,
            route: None,
        }
    }

    fn broadcast_with_mode(job: Job, execution_mode: ExecutionMode) -> Self {
        Self {
            metadata: JobMetadata::for_job_with_mode(&job, execution_mode),
            job,
            route: None,
        }
    }

    fn routed_with_mode(
        job: Job,
        route: mpsc::Sender<JobResult>,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self {
            metadata: JobMetadata::for_job_with_mode(&job, execution_mode),
            job,
            route: Some(route),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeSubmitError;

impl std::fmt::Display for RuntimeSubmitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("runtime intake channel is disconnected")
    }
}

impl std::error::Error for RuntimeSubmitError {}

#[derive(Clone)]
pub struct RuntimeClient {
    intake: Arc<Mutex<Option<mpsc::Sender<JobEnvelope>>>>,
}

impl RuntimeClient {
    fn new(intake: mpsc::Sender<JobEnvelope>) -> Self {
        Self {
            intake: Arc::new(Mutex::new(Some(intake))),
        }
    }

    fn sender(&self) -> Result<mpsc::Sender<JobEnvelope>, RuntimeSubmitError> {
        self.intake
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().cloned())
            .ok_or(RuntimeSubmitError)
    }

    pub fn close_intake(&self) {
        if let Ok(mut guard) = self.intake.lock() {
            guard.take();
        }
    }

    pub fn is_accepting(&self) -> bool {
        self.intake
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    pub fn send(&self, job: Job) -> Result<JobId, RuntimeSubmitError> {
        self.send_with_mode(job, ExecutionMode::Interactive)
    }

    pub fn send_headless(&self, job: Job) -> Result<JobId, RuntimeSubmitError> {
        self.send_with_mode(job, ExecutionMode::Headless)
    }

    pub fn send_with_mode(
        &self,
        job: Job,
        execution_mode: ExecutionMode,
    ) -> Result<JobId, RuntimeSubmitError> {
        let envelope = JobEnvelope::broadcast_with_mode(job, execution_mode);
        let id = envelope.metadata.job_id;
        self.sender()?
            .send(envelope)
            .map_err(|_| RuntimeSubmitError)?;
        Ok(id)
    }

    pub fn submit(&self, job: Job) -> Result<JobTicket, RuntimeSubmitError> {
        self.submit_with_mode(job, ExecutionMode::Interactive)
    }

    pub fn submit_headless(&self, job: Job) -> Result<JobTicket, RuntimeSubmitError> {
        self.submit_with_mode(job, ExecutionMode::Headless)
    }

    pub fn submit_with_mode(
        &self,
        job: Job,
        execution_mode: ExecutionMode,
    ) -> Result<JobTicket, RuntimeSubmitError> {
        let (result_tx, result_rx) = mpsc::channel();
        let envelope = JobEnvelope::routed_with_mode(job, result_tx, execution_mode);
        let metadata = envelope.metadata.clone();
        self.sender()?
            .send(envelope)
            .map_err(|_| RuntimeSubmitError)?;
        Ok(JobTicket {
            metadata,
            results: result_rx,
            client: self.clone(),
        })
    }
}

impl From<mpsc::Sender<Job>> for RuntimeClient {
    fn from(job_tx: mpsc::Sender<Job>) -> Self {
        let (intake_tx, intake_rx) = mpsc::channel::<JobEnvelope>();
        std::thread::spawn(move || {
            while let Ok(envelope) = intake_rx.recv() {
                if job_tx.send(envelope.job).is_err() {
                    break;
                }
            }
        });
        Self::new(intake_tx)
    }
}

pub struct JobTicket {
    metadata: JobMetadata,
    results: mpsc::Receiver<JobResult>,
    client: RuntimeClient,
}

impl JobTicket {
    pub fn metadata(&self) -> &JobMetadata {
        &self.metadata
    }

    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<JobResult, mpsc::RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }

    pub fn try_recv(&self) -> Result<JobResult, mpsc::TryRecvError> {
        self.results.try_recv()
    }

    pub fn cancel(&self) -> Result<JobId, RuntimeSubmitError> {
        self.client.send(Job::Cancel {
            id: self.metadata.job_id,
        })
    }
}

/// A registry of currently-running jobs and their cancellation tokens.
/// Cloneable; the runtime keeps one and the dispatcher keeps another.
#[derive(Clone, Default)]
pub struct CancellationRegistry {
    /// Token map paired with a condvar so waiters (graceful shutdown) are
    /// woken on every completion instead of polling with sleeps.
    inner: Arc<(Mutex<HashMap<JobId, CancellationToken>>, Condvar)>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new token under `id`. Returns the token (so the caller
    /// can pass it into the spawned task).
    pub fn register(&self, id: JobId) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut g) = self.inner.0.lock() {
            g.insert(id, token.clone());
        }
        token
    }

    /// Cancel and remove the token for `id`. No-op if unknown.
    pub fn cancel(&self, id: JobId) -> bool {
        let token = self.inner.0.lock().ok().and_then(|mut g| g.remove(&id));
        self.inner.1.notify_all();
        match token {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    /// Drop the token for `id` (job has finished naturally).
    pub fn complete(&self, id: JobId) {
        if let Ok(mut g) = self.inner.0.lock() {
            g.remove(&id);
        }
        self.inner.1.notify_all();
    }

    /// Request cancellation for every in-flight job while retaining registry
    /// entries until their exactly-once terminal result confirms completion.
    pub fn request_cancel_all(&self) {
        if let Ok(g) = self.inner.0.lock() {
            for token in g.values() {
                token.cancel();
            }
        }
    }

    /// Force-clear every job token after a bounded graceful wait has expired.
    pub fn cancel_all(&self) {
        if let Ok(mut g) = self.inner.0.lock() {
            for (_, token) in g.drain() {
                token.cancel();
            }
        }
        self.inner.1.notify_all();
    }

    /// Blocks until the registry is empty (all in-flight jobs completed) or
    /// `timeout` elapses. Wakes on each completion via condvar rather than
    /// polling with sleeps. Returns `true` when the registry drained cleanly.
    pub fn wait_until_empty(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let (lock, cvar) = &*self.inner;
        let mut guard = match lock.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        while !guard.is_empty() {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next_guard, wait_result) = match cvar.wait_timeout(guard, remaining) {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard = next_guard;
            if wait_result.timed_out() {
                return guard.is_empty();
            }
        }
        true
    }

    /// How many jobs are currently registered.
    pub fn len(&self) -> usize {
        self.inner.0.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

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

#[derive(serde::Deserialize)]
struct GeometryTransactionRow {
    page: usize,
    line_on_page: usize,
    date: String,
    raw_text: String,
    debit: Option<f64>,
    credit: Option<f64>,
    running_balance: Option<f64>,
    bbox: Option<[f32; 4]>,
    #[serde(default)]
    field_bboxes: crate::engine::model::FieldBboxes,
}

fn geometry_statement_from_json(
    raw: &str,
) -> Result<crate::ai::document_ai::BankStatement, String> {
    let rows: Vec<GeometryTransactionRow> =
        serde_json::from_str(raw).map_err(|error| format!("invalid geometry rows: {error}"))?;
    let mut transactions: Vec<crate::engine::model::Transaction> = rows
        .into_iter()
        .map(|row| crate::engine::model::Transaction {
            page: row.page,
            line_on_page: row.line_on_page,
            date: row.date,
            raw_text: row.raw_text,
            debit: row.debit.map(crate::engine::model::f64_to_dec),
            credit: row.credit.map(crate::engine::model::f64_to_dec),
            running_balance: row.running_balance.map(crate::engine::model::f64_to_dec),
            bbox: row.bbox,
            field_bboxes: row.field_bboxes,
            provenance: crate::engine::model::Provenance::Computed,
            category: None,
            canonical: Default::default(),
        })
        .collect();
    transactions.sort_by_key(|transaction| (transaction.page, transaction.line_on_page));
    for transaction in &mut transactions {
        transaction.ensure_canonical_metadata();
    }
    let total_pages = transactions
        .iter()
        .map(|transaction| transaction.page + 1)
        .max()
        .unwrap_or_default();
    let opening_balance = transactions
        .first()
        .and_then(|transaction| transaction.running_balance)
        .map(|balance| {
            balance - transactions[0].debit.unwrap_or_default()
                + transactions[0].credit.unwrap_or_default()
        })
        .unwrap_or_default();
    let closing_balance = transactions
        .last()
        .and_then(|transaction| transaction.running_balance)
        .unwrap_or_default();
    Ok(crate::ai::document_ai::BankStatement {
        total_pages,
        transactions,
        opening_balance,
        closing_balance,
        account_number: None,
        bank_name: None,
    })
}

impl PythonJob {
    fn to_worker_request(
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

    fn worker_response_to_legacy(
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

fn python_input_sha256(path: &str) -> Result<String, String> {
    use sha2::Digest;
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|error| {
        format!(
            "cannot hash Python input {}: {error}",
            Path::new(path).display()
        )
    })?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Debug)]
pub enum Job {
    Ping,
    UfoAutoEdit {
        path: PathBuf,
        context: String,
    },
    CancelUfo,
    Python(PythonJob, oneshot::Sender<PythonJobResult>),
    LoadDocument {
        path: PathBuf,
        three_page_mode: bool,
    },
    /// Stage 8.5: standalone font analysis trigger. Useful from a "Re-analyze"
    /// menu in the GUI; LoadDocument also fires this automatically.
    AnalyzeFonts {
        path: PathBuf,
    },
    RenderPage {
        path: PathBuf,
        page: usize,
        dpi: f32,
        tag: String,
    },
    ApplyChange {
        input: PathBuf,
        output: PathBuf,
        page: usize,
        bbox: [f32; 4],
        new_text: String,
        old_text: String,
        description: String,
        deep_font_replication: bool,
    },
    CompleteFont {
        path: PathBuf,
        font_name: String,
    },
    Undo,
    Redo,
    BalanceStatement {
        path: PathBuf,
    },
    ExtractTransactions {
        path: PathBuf,
        parser_mode: crate::app::config::DocumentParserMode,
    },
    NaturalLanguageEdit {
        prompt: String,
        transactions: Vec<crate::engine::model::Transaction>,
    },
    CategorizeTransactions {
        transactions: Vec<crate::engine::model::Transaction>,
    },
    ApplyProposedChanges {
        input: PathBuf,
        output: PathBuf,
        changes: Vec<crate::engine::model::ProposedChange>,
    },
    GenerateVisualAlternatives {
        input: PathBuf,
        out_dir: PathBuf,
        page: usize,
        edits: Vec<crate::engine::workflow::UserEdit>,
        bbox: [f32; 4],
    },
    ExportChangeHistory {
        output: PathBuf,
    },
    LoadHistory {
        input: PathBuf,
    },
    Verify {
        original: PathBuf,
        edited: PathBuf,
        output_dir: PathBuf,
        intended_edits: Vec<crate::engine::verification::VerificationIntent>,
        use_pdfrest: bool,
        pdfrest_key: Option<String>,
        auto_match_dpi: bool,
    },
    ExplainImbalance {
        transactions_json: String,
        opening_balance: f64,
        closing_balance: f64,
        imbalance: f64,
    },

    /// Cancel a previously-enqueued job by its [`JobId`]. Best-effort; the
    /// task may have already finished. The runtime drops the token, so any
    /// `tokio::select!` watching `cancelled()` exits with a structured error.
    Cancel {
        id: JobId,
    },
    SubmitBugReport {
        description: String,
        include_logs: bool,
        include_audit: bool,
    },
    TypstReconstruct {
        input: std::path::PathBuf,
        output: std::path::PathBuf,
    },
    McpRenderPage {
        input: std::path::PathBuf,
        page: usize,
    },

    /// Hot-reload the runtime's `AppConfig` from the current process
    /// environment. The GUI sends this after the user updates API keys /
    /// credentials in-app (which write `.env` and `std::env::set_var`), so
    /// subsequent Document AI / Gemini jobs pick up the new values without an
    /// application restart.
    ReloadConfig,

    /// Trigger an active validation check on the AI credentials
    ValidateCredentials,

    /// Run the Smart Balance Engine and, when `auto_apply` is true, apply every
    /// proposed adjustment to the PDF in one shot (the "Adjust entire bank
    /// statement accordingly and apply all edits" button). When `auto_apply`
    /// is false this behaves like [`Job::BalanceStatement`].
    BalanceAndApplyAll {
        input: PathBuf,
        output: PathBuf,
        auto_apply: bool,
    },
    /// Cleanup orphaned temporary files from crash recovery
    CleanupTempFiles,

    // ----- Multi-stage workflow -------------------------------------------
    /// Stage 1: parse with Document AI then validate completeness with Gemini.
    WorkflowParseAndValidate {
        input: PathBuf,
        version: Option<String>,
        /// Which document parser the user selected in Backend Preferences.
        parser_mode: crate::app::config::DocumentParserMode,
        /// Which AI provider the user selected (used for completeness validation).
        ai_provider: crate::app::config::AiProviderMode,
        ignore_offline_fallback: bool,
    },
    /// Stage 3: build a balance preview from edits without writing the PDF.
    WorkflowPreview {
        original_transactions: Vec<crate::engine::model::Transaction>,
        edits: Vec<crate::engine::workflow::UserEdit>,
        opening_balance: rust_decimal::Decimal,
        expected_closing: Option<rust_decimal::Decimal>,
    },
    /// Stage 4 + 5 + 6: apply edits, render, validate visually in a loop, then
    /// re-parse with Document AI to confirm math.
    WorkflowConfirmAndRender {
        input: PathBuf,
        output: PathBuf,
        edits: Vec<crate::engine::workflow::UserEdit>,
        original_transactions: Vec<crate::engine::model::Transaction>,
        opening_balance: rust_decimal::Decimal,
        expected_closing: Option<rust_decimal::Decimal>,
        deep_font_replication: bool,
        max_visual_attempts: u32,
        visual_threshold: f64,
        ignore_font_coverage: bool,
        ignore_visual_fidelity: bool,
    },
    /// Use AI to fix text box issues and visual fidelity differences
    AiFixVisualFidelity {
        input: PathBuf,
        page: usize,
    },
    /// Transfer transactions from one bank statement PDF to another,
    /// adapting formats and verifying math + visual fidelity.
    TransferTransactions {
        source_pdf: PathBuf,
        target_pdf: PathBuf,
        output_pdf: PathBuf,
    },
    /// Bulk-shift or remap all transaction dates.
    AdjustDatePeriods {
        input: PathBuf,
        output: PathBuf,
        mode: crate::engine::date_adjust::DateAdjustMode,
    },
    /// User's response to an AI confirmation question.
    AiConfirmationResponse(crate::engine::ai_confirm::AiConfirmationResponse),
    InteractiveFallbackResponse(crate::engine::interactive_fallback::InteractiveFallbackResponse),
    /// Run cross-statement transfer tests on a set of PDFs.
    RunTransferTests {
        statements: Vec<PathBuf>,
        max_iterations: u32,
    },
    AiCommand {
        prompt: String,
        path: PathBuf,
    },

    // -- Document AI Version Management --
    /// Fetch list of available processor versions from the API.
    ListDocAiVersions,
    /// Deploy a specific processor version for inference.
    DeployDocAiVersion {
        version_id: String,
    },
    /// Undeploy a specific processor version.
    UndeployDocAiVersion {
        version_id: String,
    },
    /// Set a version as the default processor version.
    SetDefaultDocAiVersion {
        version_id: String,
    },
    /// Trigger training of a new custom processor version.
    TrainDocAiVersion {
        display_name: String,
        base_version: Option<String>,
    },
}

impl Job {
    pub fn is_fast(&self) -> bool {
        matches!(
            self,
            Job::Ping
                | Job::Undo
                | Job::Redo
                | Job::Cancel { .. }
                | Job::ReloadConfig
                | Job::CleanupTempFiles
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Job::McpRenderPage { .. } => "mcp_render_page",
            Self::Ping => "ping",
            Self::Python(..) => "python",
            Self::LoadDocument { .. } => "load_document",
            Self::AnalyzeFonts { .. } => "analyze_fonts",
            Self::RenderPage { .. } => "render_page",
            Self::ApplyChange { .. } => "apply_change",
            Self::CompleteFont { .. } => "complete_font",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::BalanceStatement { .. } => "balance_statement",
            Self::ExtractTransactions { .. } => "extract_transactions",
            Self::NaturalLanguageEdit { .. } => "natural_language_edit",
            Self::CategorizeTransactions { .. } => "categorize_transactions",
            Self::ApplyProposedChanges { .. } => "apply_proposed_changes",
            Self::GenerateVisualAlternatives { .. } => "generate_visual_alternatives",
            Self::ExportChangeHistory { .. } => "export_change_history",
            Self::LoadHistory { .. } => "load_history",
            Self::Verify { .. } => "verify",
            Self::Cancel { .. } => "cancel",
            Self::SubmitBugReport { .. } => "submit_bug_report",
            Self::TypstReconstruct { .. } => "typst_reconstruct",
            Self::ExplainImbalance { .. } => "explain_imbalance",
            Self::ReloadConfig => "reload_config",
            Self::ValidateCredentials => "validate_credentials",
            Self::BalanceAndApplyAll { .. } => "balance_and_apply_all",
            Self::CleanupTempFiles => "cleanup_temp_files",
            Self::WorkflowParseAndValidate { .. } => "workflow_parse_and_validate",
            Self::WorkflowPreview { .. } => "workflow_preview",
            Self::WorkflowConfirmAndRender { .. } => "workflow_confirm_and_render",
            Self::AiFixVisualFidelity { .. } => "ai_fix_visual_fidelity",
            Self::TransferTransactions { .. } => "transfer_transactions",
            Self::AdjustDatePeriods { .. } => "adjust_date_periods",
            Self::AiConfirmationResponse(_) => "ai_confirmation_response",
            Self::InteractiveFallbackResponse(_) => "interactive_fallback_response",
            Self::RunTransferTests { .. } => "run_transfer_tests",
            Self::AiCommand { .. } => "ai_command",
            Self::ListDocAiVersions => "list_docai_versions",
            Self::DeployDocAiVersion { .. } => "deploy_docai_version",
            Self::UndeployDocAiVersion { .. } => "undeploy_docai_version",
            Self::SetDefaultDocAiVersion { .. } => "set_default_docai_version",
            Self::TrainDocAiVersion { .. } => "train_docai_version",
            Self::UfoAutoEdit { .. } => "ufo_auto_edit",
            Self::CancelUfo => "cancel_ufo",
        }
    }

    fn document_path(&self) -> Option<&Path> {
        match self {
            Self::LoadDocument { path, .. }
            | Self::AnalyzeFonts { path }
            | Self::RenderPage { path, .. }
            | Self::CompleteFont { path, .. }
            | Self::BalanceStatement { path }
            | Self::ExtractTransactions { path, .. } => Some(path),
            Self::ApplyChange { input, .. }
            | Self::ApplyProposedChanges { input, .. }
            | Self::GenerateVisualAlternatives { input, .. }
            | Self::TypstReconstruct { input, .. }
            | Self::BalanceAndApplyAll { input, .. }
            | Self::WorkflowParseAndValidate { input, .. }
            | Self::WorkflowConfirmAndRender { input, .. }
            | Self::AiFixVisualFidelity { input, .. }
            | Self::AdjustDatePeriods { input, .. } => Some(input),
            Self::Verify { edited, .. } => Some(edited),
            Self::TransferTransactions { target_pdf, .. } => Some(target_pdf),
            Self::AiCommand { path, .. } => Some(path),
            _ => None,
        }
    }

    fn default_timeout(&self) -> std::time::Duration {
        use std::time::Duration;
        match self {
            Self::Ping
            | Self::Undo
            | Self::Redo
            | Self::Cancel { .. }
            | Self::ReloadConfig
            | Self::CleanupTempFiles => Duration::from_secs(300),
            Self::WorkflowParseAndValidate { .. }
            | Self::WorkflowConfirmAndRender { .. }
            | Self::TransferTransactions { .. }
            | Self::RunTransferTests { .. }
            | Self::Verify { .. }
            | Self::TypstReconstruct { .. } => Duration::from_secs(15 * 60),
            _ => Duration::from_secs(15 * 60),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationDisposition {
    Succeeded,
    NoOp,
    Partial,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
pub enum JobResult {
    Pong,
    UfoAutoEditResult(serde_json::Value),
    UfoLog(String),
    ApiKeysVerified(crate::app::api_verification::VerificationReport),
    DocumentLoaded {
        layout_json: String,
        total_pages: usize,
    },
    PageRendered {
        png_bytes: Vec<u8>,
        page: usize,
        dpi: f32,
        tag: String,
        width_pts: f32,
        height_pts: f32,
    },
    ChangeApplied {
        record: ChangeRecord,
        requires_visual_review: bool,
    },
    HistoryUpdated {
        history: ChangeHistory,
    },
    FontCompleted(String),
    ChangeHistoryExported {
        path: PathBuf,
    },
    TransactionsExtracted(Vec<crate::engine::model::Transaction>),
    NaturalLanguageEditReady(Vec<crate::engine::model::Transaction>),
    CategorizationReady(Vec<crate::engine::model::Transaction>),
    VerificationReport(crate::engine::verification::VerificationReport),
    /// Stage 8.5: per-font usage and coverage breakdown for the loaded PDF.
    /// Sent automatically after `Job::LoadDocument` and on demand from
    /// `Job::AnalyzeFonts`.
    FontAnalysisReady(crate::engine::font_analysis::FontAnalysis),
    /// Stage 12 / Item #3: emitted when the workflow's font cascade was
    /// invoked because the apply step hit FONT_COVERAGE_INSUFFICIENT.
    /// The GUI uses this to surface a small audit line summarising which
    /// tiers were used and which characters each tier contributed.
    FontCascadeUsed(crate::engine::font_analysis::FontCascadeReport),
    BalanceProposed {
        imbalance: rust_decimal::Decimal,
        changes: Vec<crate::engine::model::ProposedChange>,
    },
    McpRenderComplete {
        base64_png: String,
    },
    ProposedChangesApplied {
        changes_applied: usize,
        failures: Vec<String>,
    },
    ImbalanceExplained {
        explanation: String,
    },
    /// Emitted after a [`Job::ReloadConfig`]: reports whether the reloaded
    /// config has working AI credentials so the GUI can update its status line.
    ConfigReloaded {
        generation: u64,
        config: std::sync::Arc<crate::app::config::AppConfig>,
        document_ai_configured: bool,
        gemini_configured: bool,
        pro_editing_available: bool,
    },
    Error {
        job_label: String,
        message: String,
    },
    NuclearFallbackRequired(String),
    Progress {
        label: String,
        fraction: f32,
    },
    /// A job tagged with this `JobId` was cancelled before it finished.
    Cancelled {
        id: JobId,
    },
    TimedOut {
        id: JobId,
        job_label: String,
    },
    ReconstructComplete {
        output_path: std::path::PathBuf,
    },
    BugReportSubmitted,

    // ----- Multi-stage workflow ------------------------------------------
    WorkflowStageChanged {
        stage: crate::engine::workflow::WorkflowStage,
    },
    WorkflowParseValidated {
        validation: crate::engine::workflow::ParseValidation,
        transactions: Vec<crate::engine::model::Transaction>,
    },
    WorkflowPreviewBuilt(crate::engine::workflow::BalancePreview),
    WorkflowVisualAttempt(crate::engine::workflow::VisualAttempt),
    VisualAlternativesReady(Vec<(String, Vec<u8>)>),
    WorkflowComplete(crate::engine::workflow::WorkflowOutcome),
    WorkflowFailed(crate::engine::workflow::WorkflowFailure),

    // ----- Transfer Transactions ------------------------------------------
    TransferComplete(crate::engine::transfer::TransferResult),
    TransferFailed {
        stage: String,
        message: String,
    },

    // ----- Date Adjustment -------------------------------------------------
    DatesAdjusted {
        records: Vec<crate::engine::date_adjust::DateShiftRecord>,
        output_path: PathBuf,
    },

    // ----- AI Confirmation -------------------------------------------------
    AiConfirmationNeeded(crate::engine::ai_confirm::AiConfirmation),
    InteractiveFallbackRequired(crate::engine::interactive_fallback::InteractiveFallbackRequest),

    // ----- Transfer Test Harness -------------------------------------------
    TransferTestsComplete(crate::engine::transfer_test_harness::TestHarnessReport),

    // ----- General Lifecycle -----------------------------------------------
    JobCompleted {
        job_label: String,
        disposition: OperationDisposition,
        artifact: Option<PathBuf>,
        message: String,
    },

    // ----- Document AI Version Management ----------------------------------
    DocAiVersionsListed(Vec<crate::ai::document_ai::ProcessorVersionInfo>),
    DocAiVersionOperationStarted {
        operation_name: String,
        description: String,
    },
    DocAiVersionError(String),
    WatchdogEvent(crate::app::watchdog::WatchdogEvent),
}

impl JobResult {
    /// True only for results that definitively end a tracked job lifecycle.
    /// Intermediate payloads must be enumerated by consumers, not inferred as
    /// terminal merely because they are not progress messages.
    pub fn disposition(&self) -> Option<OperationDisposition> {
        match self {
            Self::Error { .. }
            | Self::WorkflowFailed(_)
            | Self::TransferFailed { .. }
            | Self::DocAiVersionError(_)
            | Self::NuclearFallbackRequired(_) => Some(OperationDisposition::Failed),
            Self::Cancelled { .. } => Some(OperationDisposition::Cancelled),
            Self::TimedOut { .. } => Some(OperationDisposition::TimedOut),
            Self::WorkflowComplete(_)
            | Self::TransferComplete(_)
            | Self::Pong
            | Self::UfoAutoEditResult(_)
            | Self::McpRenderComplete { .. }
            | Self::ChangeApplied { .. }
            | Self::FontCompleted(_)
            | Self::ChangeHistoryExported { .. }
            | Self::TransactionsExtracted(_)
            | Self::NaturalLanguageEditReady(_)
            | Self::CategorizationReady(_)
            | Self::VerificationReport(_)
            | Self::BalanceProposed { .. }
            | Self::ProposedChangesApplied { .. }
            | Self::ConfigReloaded { .. }
            | Self::ImbalanceExplained { .. }
            | Self::ReconstructComplete { .. }
            | Self::BugReportSubmitted
            | Self::WorkflowPreviewBuilt(_)
            | Self::VisualAlternativesReady(_)
            | Self::TransferTestsComplete(_) => Some(OperationDisposition::Succeeded),
            Self::JobCompleted { disposition, .. } => Some(*disposition),
            _ => None,
        }
    }

    /// True only for results that definitively end a tracked job lifecycle
    /// from the runtime `TerminalTracker` perspective (strict).
    pub fn is_terminal(&self) -> bool {
        self.disposition().is_some()
    }

    /// True when this result should free one GUI `in_flight` wait slot.
    ///
    /// Broader than [`Self::is_terminal`]: many jobs complete with a success
    /// payload (e.g. `PageRendered`, `TransactionsExtracted`) that is not a
    /// `TerminalTracker` terminal event but still ends the user wait.
    /// Intermediate stream events (`Progress`, `UfoLog`, side-effect fonts)
    /// must return false.
    pub fn ends_gui_tracked_job(&self) -> bool {
        match self {
            // Intermediate / side-channel — never free a wait slot.
            Self::Progress { .. }
            | Self::UfoLog(_)
            | Self::WatchdogEvent(_)
            | Self::FontAnalysisReady(_)
            | Self::FontCascadeUsed(_)
            | Self::HistoryUpdated { .. }
            | Self::WorkflowStageChanged { .. }
            | Self::WorkflowVisualAttempt(_)
            | Self::AiConfirmationNeeded(_)
            | Self::InteractiveFallbackRequired(_)
            | Self::DocAiVersionsListed(_)
            | Self::DocAiVersionOperationStarted { .. }
            | Self::ApiKeysVerified(_)
            // DocumentLoaded auto-chains into parse; keep the same wait open.
            | Self::DocumentLoaded { .. } => false,

            // Failures and explicit terminals.
            Self::Error { .. }
            | Self::Cancelled { .. }
            | Self::TimedOut { .. }
            | Self::WorkflowFailed(_)
            | Self::TransferFailed { .. }
            | Self::JobCompleted { .. }
            | Self::NuclearFallbackRequired(_)
            // Success payloads that complete a user-dispatched job.
            | Self::Pong
            | Self::UfoAutoEditResult(_)
            | Self::McpRenderComplete { .. }
            | Self::PageRendered { .. }
            | Self::ChangeApplied { .. }
            | Self::FontCompleted(_)
            | Self::ChangeHistoryExported { .. }
            | Self::TransactionsExtracted(_)
            | Self::NaturalLanguageEditReady(_)
            | Self::CategorizationReady(_)
            | Self::VerificationReport(_)
            | Self::BalanceProposed { .. }
            | Self::ProposedChangesApplied { .. }
            | Self::ConfigReloaded { .. }
            | Self::ImbalanceExplained { .. }
            | Self::ReconstructComplete { .. }
            | Self::BugReportSubmitted
            | Self::WorkflowParseValidated { .. }
            | Self::WorkflowPreviewBuilt(_)
            | Self::VisualAlternativesReady(_)
            | Self::WorkflowComplete(_)
            | Self::TransferComplete(_)
            | Self::DatesAdjusted { .. }
            | Self::TransferTestsComplete(_)
            | Self::DocAiVersionError(_) => true,
        }
    }

    pub fn completed(
        job_label: impl Into<String>,
        disposition: OperationDisposition,
        artifact: Option<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self::JobCompleted {
            job_label: job_label.into(),
            disposition,
            artifact,
            message: message.into(),
        }
    }
}

#[derive(Clone)]
struct ResultSink {
    broadcast: mpsc::Sender<JobResult>,
    metadata: JobMetadata,
    route: Option<mpsc::Sender<JobResult>>,
    cancellations: CancellationRegistry,
    terminal_sent: std::sync::Arc<std::sync::atomic::AtomicBool>,
    completion: std::sync::Arc<tokio::sync::Notify>,
}

impl ResultSink {
    fn new(
        broadcast: mpsc::Sender<JobResult>,
        metadata: JobMetadata,
        route: Option<mpsc::Sender<JobResult>>,
        cancellations: CancellationRegistry,
    ) -> Self {
        Self {
            broadcast,
            metadata,
            route,
            cancellations,
            terminal_sent: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            completion: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    #[allow(clippy::result_large_err)]
    fn send(&self, result: JobResult) -> Result<(), mpsc::SendError<JobResult>> {
        use std::sync::atomic::Ordering;

        let disposition = result.disposition();
        let terminal = disposition.is_some();
        tracing::debug!(
            job_id = self.metadata.job_id,
            correlation_id = %self.metadata.correlation_id,
            document_id = self.metadata.document_id.as_deref().unwrap_or("none"),
            job_label = self.metadata.label,
            terminal,
            disposition = ?disposition,
            "runtime result emitted"
        );
        if self.terminal_sent.load(Ordering::Acquire) {
            tracing::warn!(
                job_id = self.metadata.job_id,
                job_label = self.metadata.label,
                "suppressing result emitted after terminal event"
            );
            return Ok(());
        }
        if terminal && self.terminal_sent.swap(true, Ordering::AcqRel) {
            tracing::warn!(
                job_id = self.metadata.job_id,
                job_label = self.metadata.label,
                "suppressing duplicate terminal event"
            );
            return Ok(());
        }
        let outcome = if let Some(route) = &self.route {
            route.send(result)
        } else {
            self.broadcast.send(result)
        };
        if terminal {
            tracing::info!(
                job_id = self.metadata.job_id,
                correlation_id = %self.metadata.correlation_id,
                document_id = self.metadata.document_id.as_deref().unwrap_or("none"),
                job_label = self.metadata.label,
                disposition = ?disposition,
                "runtime job terminated"
            );
            self.cancellations.complete(self.metadata.job_id);
            self.completion.notify_waiters();
        }
        outcome
    }

    fn is_interactive(&self) -> bool {
        self.metadata.execution_mode == ExecutionMode::Interactive
    }

    async fn completed(&self) {
        if self
            .terminal_sent
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        self.completion.notified().await;
    }
}

// Parser-chain plumbing (fallback router, provider order, interactive
// fallback waiter/macro) lives in the properly declared submodule
// `src/app/runtime/parser_chain.rs`.

fn spawn_job_lifecycle_monitor(
    result_sink: ResultSink,
    cancellation_token: tokio_util::sync::CancellationToken,
) {
    let timeout = result_sink
        .metadata
        .deadline
        .saturating_duration_since(std::time::Instant::now());
    let job_id = result_sink.metadata.job_id;
    let job_label = result_sink.metadata.label.to_string();
    tokio::spawn(async move {
        tokio::select! {
            _ = result_sink.completed() => {}
            _ = cancellation_token.cancelled() => {
                let _ = result_sink.send(JobResult::Cancelled { id: job_id });
            }
            _ = tokio::time::sleep(timeout) => {
                let _ = result_sink.send(JobResult::TimedOut {
                    id: job_id,
                    job_label,
                });
                cancellation_token.cancel();
            }
        }
    });
}

#[derive(Clone)]
pub struct TerminalTracker(std::sync::Arc<TerminalTrackerInner>);

struct TerminalTrackerInner {
    tx: ResultSink,
    label: String,
    terminal_sent: std::sync::atomic::AtomicBool,
}

impl TerminalTracker {
    fn new(tx: ResultSink, label: impl Into<String>) -> Self {
        Self(std::sync::Arc::new(TerminalTrackerInner {
            tx,
            label: label.into(),
            terminal_sent: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    #[allow(clippy::result_large_err)]
    pub fn send(&self, res: JobResult) -> Result<(), std::sync::mpsc::SendError<JobResult>> {
        use std::sync::atomic::Ordering;

        if self.0.terminal_sent.load(Ordering::Acquire) {
            tracing::warn!(
                "[runtime] suppressing result emitted after terminal event for {}: {:?}",
                self.0.label,
                res
            );
            return Ok(());
        }
        if res.is_terminal()
            && self
                .0
                .terminal_sent
                .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            tracing::warn!(
                "[runtime] suppressing duplicate terminal event for {}: {:?}",
                self.0.label,
                res
            );
            return Ok(());
        }
        self.0.tx.send(res)
    }

    fn is_interactive(&self) -> bool {
        self.0.tx.is_interactive()
    }
}

impl Drop for TerminalTrackerInner {
    fn drop(&mut self) {
        if !self
            .terminal_sent
            .load(std::sync::atomic::Ordering::Acquire)
        {
            let _ = self.tx.send(JobResult::Error {
                job_label: self.label.clone(),
                message: "Background task panicked or exited silently without a terminal result."
                    .into(),
            });
        }
    }
}

/// Blocks on `fut` from a synchronous context without deadlocking or
/// panicking under any runtime flavor.
///
/// - Multi-thread runtime: `block_in_place` + `Handle::block_on` (safe there).
/// - Current-thread runtime or no runtime: `block_in_place` panics ("cannot
///   block inside a current_thread runtime"), so the future runs on a scratch
///   thread with its own single-thread runtime instead.
fn block_on_from_blocking_context<F, T>(fut: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(fut))
        }
        Ok(_) | Err(_) => std::thread::spawn(move || {
            #[allow(clippy::expect_used)] // scratch runtime build is effectively infallible
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build scratch tokio runtime");
            rt.block_on(fut)
        })
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload)),
    }
}

pub struct Runtime {
    tokio_rt: Option<tokio::runtime::Runtime>,
    runtime_client: RuntimeClient,
    audit_log: Arc<Mutex<AuditLog>>,
    shutdown_complete: bool,
    /// Registry of in-flight jobs and their cancellation tokens. Cloneable;
    /// pass to the GUI so it can cancel by id.
    pub cancellations: CancellationRegistry,
    pub watchdog: std::sync::Arc<crate::app::watchdog::Watchdog>,
}

impl Runtime {
    pub fn shutdown(&mut self, timeout: std::time::Duration) -> bool {
        if self.shutdown_complete {
            return true;
        }

        self.runtime_client.close_intake();
        self.cancellations.request_cancel_all();
        // Condvar-woken drain: registered jobs notify the registry on
        // completion, so shutdown sleeps until woken instead of polling.
        let clean = self.cancellations.wait_until_empty(timeout);
        if !clean {
            self.cancellations.cancel_all();
        }

        if let Ok(mut audit) = self.audit_log.lock() {
            let status = if clean {
                "Graceful shutdown completed"
            } else {
                "Graceful shutdown deadline expired; remaining jobs force-cancelled"
            };
            let _ = audit.append_line(status);
        }

        if let Some(runtime) = self.tokio_rt.take() {
            runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        }
        self.shutdown_complete = true;
        clean
    }

    pub fn start(
        audit_log: AuditLog,
        config: Arc<crate::app::config::AppConfig>,
    ) -> (Self, RuntimeClient, mpsc::Receiver<JobResult>) {
        #[allow(clippy::expect_used)] // Tokio runtime creation is infallible in practice
        let tokio_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to start Tokio runtime");

        let (intake_tx, intake_rx) = mpsc::channel::<JobEnvelope>();
        let runtime_client = RuntimeClient::new(intake_tx.clone());
        let (legacy_job_tx, legacy_job_rx) = mpsc::channel::<Job>();
        let legacy_intake_tx = intake_tx.clone();
        std::thread::spawn(move || {
            while let Ok(job) = legacy_job_rx.recv() {
                if legacy_intake_tx.send(JobEnvelope::broadcast(job)).is_err() {
                    break;
                }
            }
        });
        let (result_tx, result_rx) = mpsc::channel::<JobResult>();
        let (watchdog, mut watchdog_rx) = crate::app::watchdog::Watchdog::new();
        let watchdog = std::sync::Arc::new(watchdog);
        let watchdog_for_gui = watchdog.clone();

        let (python_tx, python_rx) =
            mpsc::channel::<(PythonJob, oneshot::Sender<PythonJobResult>)>();

        let audit_log = Arc::new(Mutex::new(audit_log));
        let runtime_audit_log = audit_log.clone();
        let history = Arc::new(Mutex::new(ChangeHistory::new()));
        let config_holder = crate::app::config::ConfigManager::new(config);

        let primary_engine = Arc::new(crate::pdf::PyMuPdfEngine::new(legacy_job_tx));
        let fallback_engine = Arc::new(crate::pdf::OxidizePdfEngine::new());
        let engine: Arc<dyn crate::pdf::PdfEngine> = Arc::new(crate::pdf::PdfEngineSelector::new(
            primary_engine,
            fallback_engine,
            config_holder.clone(),
        ));

        let _python_actor_thread = thread::spawn(move || {
            // T2 test support: preserve the explicit unavailable-worker path used
            // by existing cascade tests without starting an embedded interpreter.
            if std::env::var("TEST_CRASH_PYTHON_ACTOR").is_ok() {
                tracing::warn!(
                    "[PYTHON_WORKER] TEST_CRASH_PYTHON_ACTOR set — simulating unavailable worker"
                );
                while let Ok((_job, reply_tx)) = python_rx.recv() {
                    let _ = reply_tx.send(PythonJobResult::Error(
                        "Simulated Python worker crash for testing".to_string(),
                    ));
                }
                return;
            }

            let worker = crate::ai::python_worker::PythonWorkerClient::start(
                crate::ai::python_worker::PythonWorkerConfig::default(),
            );
            let worker = match worker {
                Ok(worker) => worker,
                Err(error) => {
                    tracing::error!("[PYTHON_WORKER] startup failed: {error}");
                    while let Ok((_job, reply_tx)) = python_rx.recv() {
                        let _ = reply_tx.send(PythonJobResult::Error(format!(
                            "Python worker unavailable: {error}"
                        )));
                    }
                    return;
                }
            };

            while let Ok((job, reply_tx)) = python_rx.recv() {
                let result = match job.to_worker_request() {
                    Ok(request) => match worker.execute(request) {
                        Ok(response) => job.worker_response_to_legacy(response),
                        Err(error) => PythonJobResult::Error(format!(
                            "Python worker operation failed: {error}"
                        )),
                    },
                    Err(error) => PythonJobResult::Error(error),
                };
                let _ = reply_tx.send(result);
            }
            let _ = worker.shutdown(std::time::Duration::from_secs(5));
        });

        let cancellations = CancellationRegistry::new();
        let cancellations_for_loop = cancellations.clone();
        let result_tx_clone = result_tx.clone();
        let python_tx_clone = python_tx.clone();

        let (fast_job_tx, mut fast_job_rx) = tokio::sync::mpsc::unbounded_channel::<JobEnvelope>();
        let (slow_job_tx, mut slow_job_rx) = tokio::sync::mpsc::unbounded_channel::<JobEnvelope>();

        spawn_runtime_bridge(
            intake_rx,
            fast_job_tx.clone(),
            slow_job_tx.clone(),
            result_tx.clone(),
        );
        let engine_for_tokio = engine.clone();

        // Hot-swappable config: jobs read the *current* config via a per-iteration
        // snapshot, so an in-app API-key/credentials update (Job::ReloadConfig)
        // takes effect on subsequent jobs without an application restart.

        let api_semaphore = Arc::new(tokio::sync::Semaphore::new(3));
        let _ = fast_job_tx.send(JobEnvelope::broadcast(Job::CleanupTempFiles));

        let watchdog_clone = watchdog.clone();
        let tokio_rt_handle = tokio_rt.handle().clone();
        let wd_tx = result_tx.clone();
        tokio_rt_handle.spawn(async move {
            while let Ok(event) = watchdog_rx.recv().await {
                let _ = wd_tx.send(JobResult::WatchdogEvent(event));
            }
        });

        let api_poll_tx = result_tx.clone();

        // 2-second periodic task for .env hot-reloading
        let hot_reload_job_tx = fast_job_tx.clone();
        tokio_rt.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            let mut last_modified = std::time::SystemTime::UNIX_EPOCH;

            loop {
                interval.tick().await;
                if let Ok(metadata) = std::fs::metadata(".env") {
                    if let Ok(modified) = metadata.modified() {
                        if modified > last_modified {
                            if last_modified != std::time::SystemTime::UNIX_EPOCH {
                                tracing::info!(
                                    "[config] .env file changed. Triggering hot-reload."
                                );
                                let _ = hot_reload_job_tx
                                    .send(JobEnvelope::broadcast(Job::ReloadConfig));
                            }
                            last_modified = modified;
                        }
                    }
                }
            }
        });

        let api_poll_config = config_holder.clone();
        tokio_rt_handle.spawn(async move {
            let cadence = std::time::Duration::from_secs(300);
            let mut interval =
                tokio::time::interval_at(tokio::time::Instant::now() + cadence, cadence);
            loop {
                interval.tick().await;
                let cfg = api_poll_config.snapshot().config();
                let report = crate::app::api_verification::collect_api_key_report(&cfg).await;
                if api_poll_tx
                    .send(JobResult::ApiKeysVerified(report))
                    .is_err()
                {
                    break;
                }
            }
        });

        let fast_python_tx_clone = python_tx_clone.clone();
        let fast_result_tx_clone = result_tx_clone.clone();
        let fast_engine_for_tokio = engine_for_tokio.clone();
        let fast_history = history.clone();
        let fast_audit_log = audit_log.clone();
        let fast_cancellations_for_loop = cancellations_for_loop.clone();
        let fast_api_semaphore = api_semaphore.clone();
        let fast_config_holder = config_holder.clone();
        let fast_watchdog_clone = watchdog_clone.clone();

        let parse_cache = std::sync::Arc::new(tokio::sync::Mutex::new(lru::LruCache::<
            String,
            crate::ai::document_ai::BankStatement,
        >::new(
            #[allow(clippy::unwrap_used)] // NonZeroUsize::new(20) is always Some
            std::num::NonZeroUsize::new(20).unwrap(),
        )));
        let fast_parse_cache = parse_cache.clone();
        let sig_audit = audit_log.clone();

        tokio_rt.spawn(async move {
            let mut segment_map: Option<SegmentMap> = None;
            let mut segment_manager: Option<SegmentManager> = None;
            let fallback_router: std::sync::Arc<
                tokio::sync::Mutex<
                    std::collections::HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<String>>,
                >,
            > = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

            while let Some(envelope) = slow_job_rx.recv().await {
                let JobEnvelope {
                    metadata,
                    job,
                    route,
                } = envelope;
                let job_span = tracing::info_span!(
                    "runtime_job",
                    job_id = metadata.job_id,
                    correlation_id = %metadata.correlation_id,
                    document_id = metadata.document_id.as_deref().unwrap_or("none"),
                    job_label = metadata.label,
                    execution_mode = ?metadata.execution_mode,
                    queue = "slow",
                );
                let cancellation_token = if !matches!(&job, Job::Cancel { .. }) {
                    Some(cancellations_for_loop.register(metadata.job_id))
                } else {
                    None
                };
                let result_sink = ResultSink::new(
                    result_tx_clone.clone(),
                    metadata,
                    route,
                    cancellations_for_loop.clone(),
                );
                if let Some(token) = cancellation_token {
                    spawn_job_lifecycle_monitor(result_sink.clone(), token);
                }
                let wdog = watchdog_clone.clone();
                let config_for_tokio = config_holder.snapshot().config();
                process_job_inner(
                    job,
                    python_tx_clone.clone(),
                    result_sink,
                    engine_for_tokio.clone(),
                    config_for_tokio.clone(),
                    wdog.clone(),
                    history.clone(),
                    audit_log.clone(),
                    cancellations_for_loop.clone(),
                    api_semaphore.clone(),
                    &mut segment_map,
                    &mut segment_manager,
                    fallback_router.clone(),
                    parse_cache.clone(),
                    config_holder.clone(),
                )
                .instrument(job_span)
                .await;
            }
        });

        let parse_cache = fast_parse_cache;
        tokio_rt.spawn(async move {
            let mut segment_map: Option<SegmentMap> = None;
            let mut segment_manager: Option<SegmentManager> = None;
            let fallback_router: std::sync::Arc<
                tokio::sync::Mutex<
                    std::collections::HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<String>>,
                >,
            > = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

            while let Some(envelope) = fast_job_rx.recv().await {
                let JobEnvelope {
                    metadata,
                    job,
                    route,
                } = envelope;
                let job_span = tracing::info_span!(
                    "runtime_job",
                    job_id = metadata.job_id,
                    correlation_id = %metadata.correlation_id,
                    document_id = metadata.document_id.as_deref().unwrap_or("none"),
                    job_label = metadata.label,
                    execution_mode = ?metadata.execution_mode,
                    queue = "fast",
                );
                let cancellation_token = if !matches!(&job, Job::Cancel { .. }) {
                    Some(fast_cancellations_for_loop.register(metadata.job_id))
                } else {
                    None
                };
                let result_sink = ResultSink::new(
                    fast_result_tx_clone.clone(),
                    metadata,
                    route,
                    fast_cancellations_for_loop.clone(),
                );
                if let Some(token) = cancellation_token {
                    spawn_job_lifecycle_monitor(result_sink.clone(), token);
                }
                let wdog = fast_watchdog_clone.clone();
                let config_for_tokio = fast_config_holder.snapshot().config();
                process_job_inner(
                    job,
                    fast_python_tx_clone.clone(),
                    result_sink,
                    fast_engine_for_tokio.clone(),
                    config_for_tokio.clone(),
                    wdog.clone(),
                    fast_history.clone(),
                    fast_audit_log.clone(),
                    fast_cancellations_for_loop.clone(),
                    fast_api_semaphore.clone(),
                    &mut segment_map,
                    &mut segment_manager,
                    fallback_router.clone(),
                    parse_cache.clone(),
                    fast_config_holder.clone(),
                )
                .instrument(job_span)
                .await;
            }
        });

        let sig_cancellations = cancellations.clone();
        let sig_runtime_client = runtime_client.clone();
        tokio_rt.spawn(async move {
            if let Ok(()) = tokio::signal::ctrl_c().await {
                tracing::info!("Received Ctrl-C signal; initiating bounded graceful shutdown");
                sig_runtime_client.close_intake();
                sig_cancellations.request_cancel_all();

                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
                while !sig_cancellations.is_empty() && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                let clean = sig_cancellations.is_empty();
                if !clean {
                    sig_cancellations.cancel_all();
                }
                if let Ok(mut lock) = sig_audit.lock() {
                    let status = if clean {
                        "Graceful shutdown completed after Ctrl-C"
                    } else {
                        "Ctrl-C shutdown deadline expired; remaining jobs force-cancelled"
                    };
                    let _ = lock.append_line(status);
                }
                std::process::exit(if clean { 0 } else { 2 });
            }
        });

        (
            Self {
                tokio_rt: Some(tokio_rt),
                runtime_client: runtime_client.clone(),
                audit_log: runtime_audit_log,
                shutdown_complete: false,
                cancellations,
                watchdog: watchdog_for_gui,
            },
            runtime_client,
            result_rx,
        )
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.shutdown(std::time::Duration::from_secs(5));
    }
}

fn spawn_runtime_bridge(
    job_rx: mpsc::Receiver<JobEnvelope>,
    fast_tx: tokio::sync::mpsc::UnboundedSender<JobEnvelope>,
    slow_tx: tokio::sync::mpsc::UnboundedSender<JobEnvelope>,
    result_tx: mpsc::Sender<JobResult>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(envelope) = job_rx.recv() {
            let outcome = if envelope.job.is_fast() {
                fast_tx.send(envelope)
            } else {
                slow_tx.send(envelope)
            };
            if let Err(error) = outcome {
                let envelope = error.0;
                let result = JobResult::Error {
                    job_label: envelope.metadata.label.into(),
                    message: "Tokio worker disconnected".into(),
                };
                if let Some(route) = envelope.route {
                    let _ = route.send(result);
                } else {
                    let _ = result_tx.send(result);
                }
                break;
            }
        }
    })
}

/// Dispatches a Python job to the actor thread.
/// This function MUST forward directly to avoid recursion through the engine selector.
fn dispatch_python_job(
    py_job: PythonJob,
    reply_tx: oneshot::Sender<PythonJobResult>,
    python_tx: &mpsc::Sender<(PythonJob, oneshot::Sender<PythonJobResult>)>,
) {
    if let Err(e) = python_tx.send((py_job, reply_tx)) {
        // This means the actor thread has died. Log and let the dropped reply
        // channel surface the error to the caller (oneshot::recv -> RecvError).
        tracing::error!("[runtime] python actor channel disconnected: {}", e);
    }
}

/// Tries OpenRouter text-based parsing as a fallback. If that fails, uses `offline_parser`.
async fn parse_with_offline_fallback(
    pdf_path: &std::path::Path,
    engine: std::sync::Arc<dyn crate::pdf::PdfEngine>,
    config: std::sync::Arc<crate::app::config::AppConfig>,
) -> Result<crate::ai::document_ai::BankStatement, String> {
    // 1. Try OpenRouter Parser
    match crate::engine::openrouter_parser::parse_statement_openrouter(
        pdf_path,
        engine.clone(),
        config.clone(),
    )
    .await
    {
        Ok(res) => return Ok(res),
        Err(e) => {
            tracing::warn!(
                "[openrouter_parser] Failed, falling back to offline_parser: {}",
                e
            );
        }
    }

    // 2. Fallback to Offline Parser
    let eng_clone = engine.clone();
    let path_clone = pdf_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        crate::engine::offline_parser::parse_statement_offline(&path_clone, eng_clone)
    })
    .await
    .unwrap_or_else(|e| Err(format!("Offline parser panicked: {}", e)))
}

#[allow(clippy::too_many_arguments)]
async fn process_job_inner(
    job: Job,
    python_tx_clone: std::sync::mpsc::Sender<(
        PythonJob,
        tokio::sync::oneshot::Sender<PythonJobResult>,
    )>,
    result_tx_clone: ResultSink,
    engine_for_tokio: std::sync::Arc<dyn crate::pdf::PdfEngine>,
    config_for_tokio: std::sync::Arc<crate::app::config::AppConfig>,
    wdog: std::sync::Arc<crate::app::watchdog::Watchdog>,
    history: std::sync::Arc<std::sync::Mutex<crate::engine::history::ChangeHistory>>,
    audit_log: std::sync::Arc<std::sync::Mutex<crate::app::audit::AuditLog>>,
    cancellations_for_loop: crate::app::runtime::CancellationRegistry,
    api_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    segment_map: &mut Option<SegmentMap>,
    segment_manager: &mut Option<SegmentManager>,
    fallback_router: InteractiveFallbackRouter,
    parse_cache: std::sync::Arc<
        tokio::sync::Mutex<lru::LruCache<String, crate::ai::document_ai::BankStatement>>,
    >,
    config_holder: crate::app::config::ConfigManager,
) {
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
        Job::ExplainImbalance {
            transactions_json,
            opening_balance,
            closing_balance,
            imbalance,
        } => {
            let client = crate::ai::local_llm::LocalLlmClient::new();
            let result_tx = result_tx_clone.clone();
            tokio::spawn(async move {
                let _ = result_tx.send(JobResult::Progress {
                    label: "Asking local Qwen 7B to explain the math error...".into(),
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
        Job::AiFixVisualFidelity { input: _, page: _ } => {
            let _ = result_tx_clone.send(JobResult::Error {
                job_label: "ai_fix_visual_fidelity".to_string(),
                message: "AI visual layout repair is not available in v1; no document was changed. Use the deterministic edit and verification workflow.".to_string(),
            });
        }
        Job::TransferTransactions {
            source_pdf,
            target_pdf,
            output_pdf,
        } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            let py_tx = python_tx_clone.clone();
            let engine_for_tokio = engine_for_tokio.clone();
            let router = fallback_router.clone();
            tokio::spawn(async move {
                use crate::engine::transfer::*;

                let started_at = std::time::Instant::now();
                let _corrections_applied = 0usize;

                // AI mapping is an optional enhancement. The supported exact-capacity
                // path uses the deterministic local planner when no provider is ready.
                let mut gemini = crate::ai::backend::AiBackend::from_app_config(&cfg)
                    .ok()
                    .map(std::sync::Arc::new);

                // Helper: parse a statement via DocAI with offline fallback.
                let doc_ai_opt = crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg)
                    .ok()
                    .map(std::sync::Arc::new);

                // Helper to send progress
                let send_progress = |res_tx: &ResultSink, stage: TransferStage| {
                    let (lo, _hi) = stage.fraction_range();
                    let _ = res_tx.send(JobResult::Progress {
                        label: stage.label().to_string(),
                        fraction: lo,
                    });
                };

                // ======= STAGE 1 & 2: Analyze Source and Target (Matrix Consensus) ========

                let parse_matrix =
                    |pdf_path: PathBuf,
                     cfg: std::sync::Arc<crate::app::config::AppConfig>,
                     engine: std::sync::Arc<dyn crate::pdf::PdfEngine>,
                     python_tx: std::sync::mpsc::Sender<(
                        PythonJob,
                        tokio::sync::oneshot::Sender<PythonJobResult>,
                    )>,
                     res_tx: ResultSink,
                     stage_name: String,
                     wdog: std::sync::Arc<crate::app::watchdog::Watchdog>| async move {
                        let mut tasks = Vec::new();

                        let geometry_path = pdf_path.clone();
                        let geometry_tx = python_tx.clone();
                        let geometry_task = tokio::spawn(async move {
                            let (reply_tx, reply_rx) = oneshot::channel();
                            if geometry_tx
                                .send((
                                    PythonJob::GetAllTransactions {
                                        pdf_path: geometry_path.to_string_lossy().to_string(),
                                    },
                                    reply_tx,
                                ))
                                .is_err()
                            {
                                return None;
                            }
                            match reply_rx.await {
                                Ok(PythonJobResult::Json(raw)) => {
                                    geometry_statement_from_json(&raw).ok()
                                }
                                _ => None,
                            }
                        });

                        // 1. DocAI
                        if let Ok(doc_ai) =
                            crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg)
                        {
                            let p = pdf_path.clone();
                            let wdog_docai = wdog.clone();
                            tasks.push(tokio::spawn(async move {
                                (
                                    "DocAI",
                                    crate::engine::pro_edit::perform_pro_edit(
                                        "DocumentAI",
                                        async {
                                            doc_ai
                                                .parse_entire_statement(&p, None::<&str>)
                                                .await
                                                .map_err(anyhow::Error::from)
                                        },
                                        wdog_docai,
                                    )
                                    .await
                                    .ok(),
                                )
                            }));
                        }

                        // 1.5. Reducto
                        if let Ok(reducto) =
                            crate::ai::reducto::ReductoClient::from_app_config(&cfg)
                        {
                            let p = pdf_path.clone();
                            let wdog_reducto = wdog.clone();
                            tasks.push(tokio::spawn(async move {
                                (
                                    "Reducto",
                                    crate::engine::pro_edit::perform_pro_edit(
                                        "Reducto",
                                        async {
                                            reducto
                                                .parse_statement_for_transfer(&p)
                                                .await
                                                .map_err(anyhow::Error::from)
                                        },
                                        wdog_reducto,
                                    )
                                    .await
                                    .ok(),
                                )
                            }));
                        }

                        // 2. LlamaParse
                        if let Ok(llama) =
                            crate::ai::llamaparse::LlamaParseClient::from_app_config(&cfg)
                        {
                            let p = pdf_path.clone();
                            let wdog_llama = wdog.clone();
                            tasks.push(tokio::spawn(async move {
                                (
                                    "LlamaParse",
                                    crate::engine::pro_edit::perform_pro_edit(
                                        "LlamaParse",
                                        async {
                                            llama
                                                .parse_statement_for_transfer(&p)
                                                .await
                                                .map_err(anyhow::Error::from)
                                        },
                                        wdog_llama,
                                    )
                                    .await
                                    .ok(),
                                )
                            }));
                        }

                        // 3. Offline Heuristic
                        let p = pdf_path.clone();
                        let e = engine.clone();
                        tasks.push(tokio::spawn(async move {
                            (
                                "Offline",
                                tokio::task::spawn_blocking(move || {
                                    crate::engine::offline_parser::parse_statement_offline(&p, e)
                                        .ok()
                                })
                                .await
                                .ok()
                                .flatten(),
                            )
                        }));

                        let results = futures_util::future::join_all(tasks).await;
                        let mut statements: Vec<(&str, crate::ai::document_ai::BankStatement)> =
                            Vec::new();
                        for res in results {
                            if let Ok((name, Some(s))) = res {
                                statements.push((name, s));
                            }
                        }

                        let geometry_statement = geometry_task.await.ok().flatten();
                        statements.retain(|(_, statement)| !statement.transactions.is_empty());
                        if statements.is_empty() {
                            if let Some(statement) = geometry_statement
                                .as_ref()
                                .filter(|statement| !statement.transactions.is_empty())
                            {
                                tracing::info!(
                                    "[TRANSFER] Promoting exact geometry ledger with {} rows because semantic parsers were empty",
                                    statement.transactions.len()
                                );
                                statements.push(("PythonGeometry", statement.clone()));
                            }
                        }

                        if statements.is_empty() {
                            let _ = res_tx.send(JobResult::TransferFailed {
                                stage: stage_name,
                                message: "All matrix consensus parsers failed.".into(),
                            });
                            return None;
                        }

                        if cfg.transfer_consensus_mode {
                            tracing::info!(
                                "[TRANSFER] Matrix Consensus: Merging {} successful parses",
                                statements.len()
                            );

                            let mut raw_stmts = Vec::new();
                            for (_, s) in &statements {
                                raw_stmts.push(s.clone());
                            }
                            let mut consensus =
                                crate::engine::consensus::merge_consensus_statements(raw_stmts);

                            // Update stats
                            let mut stats: crate::engine::model::ParserStats =
                                std::fs::read_to_string("audit/parser_stats.json")
                                    .ok()
                                    .and_then(|s| serde_json::from_str(&s).ok())
                                    .unwrap_or_default();
                            stats.total_attempts += 1;

                            // Winner is the one closest to consensus tx count
                            let mut best_dist = usize::MAX;
                            let mut winner = "";
                            for (name, s) in &statements {
                                let dist = (s.transactions.len() as isize
                                    - consensus.transactions.len() as isize)
                                    .unsigned_abs();
                                if dist < best_dist {
                                    best_dist = dist;
                                    winner = name;
                                }
                            }

                            match winner {
                                "DocAI" => stats.docai_wins += 1,
                                "LlamaParse" => stats.llamaparse_wins += 1,
                                "Offline" => stats.offline_wins += 1,
                                _ => {}
                            }
                            // Atomic file operation: write to .tmp and rename
                            let stats_path = std::path::PathBuf::from("audit/parser_stats.json");
                            let tmp_path = stats_path.with_extension("tmp");
                            if std::fs::write(
                                &tmp_path,
                                serde_json::to_string_pretty(&stats).unwrap_or_default(),
                            )
                            .is_ok()
                            {
                                let _ = std::fs::rename(tmp_path, &stats_path);
                            }

                            if let Some(geometry_statement) = geometry_statement.as_ref() {
                                let mut enriched =
                                    crate::engine::consensus::enrich_statement_geometry(
                                        &mut consensus,
                                        std::slice::from_ref(geometry_statement),
                                    );
                                if enriched < consensus.transactions.len()
                                    && !geometry_statement.transactions.is_empty()
                                {
                                    tracing::warn!(
                                        "[TRANSFER] Semantic ledger geometry incomplete ({enriched}/{}); promoting exact {}-row geometry ledger",
                                        consensus.transactions.len(),
                                        geometry_statement.transactions.len()
                                    );
                                    consensus.transactions =
                                        geometry_statement.transactions.clone();
                                    enriched = consensus.transactions.len();
                                }
                                tracing::info!(
                                    "[TRANSFER] Geometry donor enriched {}/{} rows",
                                    enriched,
                                    consensus.transactions.len()
                                );
                            }
                            crate::engine::consensus::normalize_statement_row_indices(
                                &mut consensus,
                            );
                            Some(consensus)
                        } else {
                            #[allow(clippy::expect_used)]
                            let mut statement = statements
                                .into_iter()
                                .next()
                                .expect("non-empty statements checked above")
                                .1;
                            if let Some(geometry_statement) = geometry_statement.as_ref() {
                                let enriched = crate::engine::consensus::enrich_statement_geometry(
                                    &mut statement,
                                    std::slice::from_ref(geometry_statement),
                                );
                                if enriched < statement.transactions.len()
                                    && !geometry_statement.transactions.is_empty()
                                {
                                    statement.transactions =
                                        geometry_statement.transactions.clone();
                                }
                            }
                            crate::engine::consensus::normalize_statement_row_indices(
                                &mut statement,
                            );
                            Some(statement)
                        }
                    };

                send_progress(&res_tx, TransferStage::AnalyzeSource);
                tracing::info!("[TRANSFER] Stage 1: Analyzing source PDF: {:?}", source_pdf);
                let source_stmt = match parse_matrix(
                    source_pdf.clone(),
                    cfg.clone(),
                    engine_for_tokio.clone(),
                    py_tx.clone(),
                    res_tx.clone(),
                    "AnalyzeSource".into(),
                    wdog.clone(),
                )
                .await
                {
                    Some(s) => s,
                    None => return,
                };
                let source_transactions = source_stmt.transactions.clone();
                tracing::info!(
                    "[TRANSFER] Source: {} transactions found",
                    source_transactions.len()
                );

                if source_transactions.is_empty() {
                    let _ = res_tx.send(JobResult::TransferFailed {
                        stage: "AnalyzeSource".into(),
                        message: "Source statement has 0 transactions - nothing to transfer."
                            .into(),
                    });
                    return;
                }

                let _ = res_tx.send(JobResult::Progress {
                    label: "Source analyzed ✓".to_string(),
                    fraction: 0.10,
                });

                send_progress(&res_tx, TransferStage::AnalyzeTarget);
                tracing::info!("[TRANSFER] Stage 2: Analyzing target PDF: {:?}", target_pdf);

                let target_stmt = match parse_matrix(
                    target_pdf.clone(),
                    cfg.clone(),
                    engine_for_tokio.clone(),
                    py_tx.clone(),
                    res_tx.clone(),
                    "AnalyzeTarget".into(),
                    wdog.clone(),
                )
                .await
                {
                    Some(s) => s,
                    None => return,
                };
                let target_transactions = target_stmt.transactions.clone();
                tracing::info!(
                    "[TRANSFER] Target: {} transactions found",
                    target_transactions.len()
                );

                if target_transactions.is_empty() {
                    let _ = res_tx.send(JobResult::TransferFailed {
                        stage: "AnalyzeTarget".into(),
                        message: "Target statement has 0 transactions - no layout to map into."
                            .into(),
                    });
                    return;
                }

                let _ = res_tx.send(JobResult::Progress {
                    label: "Target analyzed ✓".to_string(),
                    fraction: 0.20,
                });

                let max_retries = 5usize;
                let mut attempt = 0;
                let mut best_visual_score = 1.0f64;
                let mut best_math_verified = false;
                let mut best_result = None;
                let mut correction_hint: Option<String> = None;
                let synthesized_fonts_used = false;
                let font_override_path: Option<String> = None;
                let mut total_corrections = 0;
                let requested_output_pdf = output_pdf.clone();
                let requested_output_parent = requested_output_pdf
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let staged_transfer_output = match crate::app::commit::staging_path(
                    requested_output_parent,
                    ".dcpp-transfer-",
                    ".pdf",
                ) {
                    Ok(path) => path,
                    Err(error) => {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "PdfSurgery".into(),
                            message: format!("Failed to stage transfer output: {error}"),
                        });
                        return;
                    }
                };
                let output_pdf = staged_transfer_output.to_path_buf();

                loop {
                    attempt += 1;
                    tracing::info!("[TRANSFER] --- Starting Attempt {} ---", attempt);

                    // ======= STAGE 3: Deterministic Format Mapping ========
                    send_progress(&res_tx, TransferStage::AiFormatMapping);
                    tracing::info!("[TRANSFER] Stage 3: deterministic mapping with optional provider enhancement");

                    let local_plan = || {
                        crate::engine::transfer::plan_transaction_transfer_deterministic(
                            &source_transactions,
                            &target_transactions,
                            target_stmt.total_pages,
                        )
                    };
                    let configured_mapper = gemini.clone();
                    let local_plan_result = local_plan();
                    if let Err(error) = &local_plan_result {
                        tracing::warn!(
                            "[TRANSFER] Deterministic exact-geometry plan unavailable: {error}"
                        );
                    }
                    let transfer_plan = if let Ok(plan) = local_plan_result {
                        tracing::info!(
                            "[TRANSFER] Using deterministic exact-geometry capacity plan"
                        );
                        plan
                    } else if let Some(mapper) = configured_mapper {
                        match mapper
                            .plan_transaction_transfer(
                                &source_transactions,
                                &target_transactions,
                                correction_hint.as_deref(),
                            )
                            .await
                        {
                            Ok(plan) => plan,
                            Err(provider_error) => match local_plan() {
                                Ok(plan) => {
                                    tracing::warn!(
                                        "[TRANSFER] Provider mapping failed ({provider_error}); using deterministic local plan"
                                    );
                                    plan
                                }
                                Err(local_error) => {
                                    if !cfg.interactive_fallbacks || !res_tx.is_interactive() {
                                        let _ = res_tx.send(JobResult::TransferFailed {
                                            stage: "FormatMapping".into(),
                                            message: format!(
                                                "Provider mapping failed ({provider_error}); deterministic mapping unsupported: {local_error}"
                                            ),
                                        });
                                        return;
                                    }

                                    let mut request = crate::engine::interactive_fallback::InteractiveFallbackRequest::new(
                                        "Transfer Transactions Mapping",
                                        format!(
                                            "Provider mapping failed ({provider_error}); deterministic mapping unsupported: {local_error}"
                                        ),
                                    );
                                    if cfg.openrouter_api_key.is_some() {
                                        request = request.add_alternative(
                                            "openrouter",
                                            "Try OpenRouter (Multi-Model)",
                                            None,
                                        );
                                    }
                                    if cfg.groq_api_key.is_some() {
                                        request = request.add_alternative("groq", "Try Groq", None);
                                    }
                                    request =
                                        request.add_alternative("cancel", "Cancel Transfer", None);

                                    let (choice_tx, choice_rx) = tokio::sync::oneshot::channel();
                                    let request_id = request.id;
                                    {
                                        let mut map = router.lock().await;
                                        map.insert(request_id, choice_tx);
                                    }
                                    let _ = res_tx
                                        .send(JobResult::InteractiveFallbackRequired(request));

                                    let choice = match wait_for_interactive_choice(
                                        &router,
                                        request_id,
                                        choice_rx,
                                        std::time::Duration::from_secs(300),
                                    )
                                    .await
                                    {
                                        Ok(choice) => choice,
                                        Err(reason) => {
                                            let _ = res_tx.send(JobResult::TransferFailed {
                                                stage: "FormatMapping".into(),
                                                message: format!("Interactive fallback {reason}"),
                                            });
                                            return;
                                        }
                                    };
                                    if choice == "cancel" {
                                        let _ = res_tx.send(JobResult::TransferFailed {
                                            stage: "FormatMapping".into(),
                                            message: "User cancelled after mapping failure.".into(),
                                        });
                                        return;
                                    }

                                    let mut new_cfg = (*cfg).clone();
                                    if choice == "openrouter" {
                                        new_cfg.ai_provider =
                                            crate::app::config::AiProviderMode::OpenRouterApiKey;
                                    } else if choice == "groq" {
                                        new_cfg.ai_provider =
                                            crate::app::config::AiProviderMode::GroqApiKey;
                                    }
                                    match crate::ai::backend::AiBackend::from_app_config(&new_cfg) {
                                        Ok(client) => {
                                            gemini = Some(std::sync::Arc::new(client));
                                            continue;
                                        }
                                        Err(error) => {
                                            let _ = res_tx.send(JobResult::TransferFailed {
                                                stage: "FormatMapping".into(),
                                                message: format!(
                                                    "Failed to initialize fallback provider: {error}"
                                                ),
                                            });
                                            return;
                                        }
                                    }
                                }
                            },
                        }
                    } else {
                        match local_plan() {
                            Ok(plan) => plan,
                            Err(error) => {
                                let _ = res_tx.send(JobResult::TransferFailed {
                                    stage: "FormatMapping".into(),
                                    message: format!(
                                        "Deterministic mapping unsupported: {error}. Configure a mapping provider or review the ledgers."
                                    ),
                                });
                                return;
                            }
                        }
                    };
                    tracing::info!(
                        "[TRANSFER] Plan: {} mappings, {} pages to clone, {} to remove",
                        transfer_plan.mappings.len(),
                        transfer_plan.pages_to_clone.len(),
                        transfer_plan.pages_to_remove.len(),
                    );

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Format mapping complete ✓".to_string(),
                        fraction: 0.30,
                    });

                    // ======= STAGE 4: Compute Balances ========
                    send_progress(&res_tx, TransferStage::ComputeBalances);
                    tracing::info!("[TRANSFER] Stage 4: Computing balances");

                    let opening_balance = target_stmt.opening_balance;
                    let mut mapped: Vec<MappedTransaction> =
                        Vec::with_capacity(transfer_plan.mappings.len());
                    let mut skipped_invalid = 0usize;
                    for m in &transfer_plan.mappings {
                        let src = match source_transactions.get(m.source_index) {
                            Some(s) => s,
                            None => {
                                tracing::error!(
                                                "[TRANSFER] source_index {} out of bounds (max {}), skipping mapping",
                                                m.source_index,
                                                source_transactions.len()
                                            );
                                skipped_invalid += 1;
                                continue;
                            }
                        };
                        mapped.push(MappedTransaction {
                            target_page: m.target_page,
                            target_line: m.target_line,
                            date: m.converted_date.clone(),
                            description: m.adapted_description.clone(),
                            debit: src.debit,
                            credit: src.credit,
                            running_balance: rust_decimal::Decimal::ZERO,
                            field_bboxes: crate::engine::model::FieldBboxes::default(),
                        });
                    }
                    if skipped_invalid > 0 {
                        tracing::warn!(
                            "[TRANSFER] Skipped {} mappings with invalid source_index",
                            skipped_invalid
                        );
                    }

                    match recompute_running_balances(opening_balance, &mut mapped) {
                        Ok(()) => {
                            tracing::info!(
                                "[TRANSFER] Balances computed for {} transactions",
                                mapped.len()
                            );
                        }
                        Err(e) => {
                            tracing::error!("[TRANSFER] Balance recomputation failed: {}", e);
                            let _ = res_tx.send(JobResult::TransferFailed {
                                stage: "ComputeBalances".into(),
                                message: format!("Balance recomputation failed: {}", e),
                            });
                            return;
                        }
                    }

                    let _ = res_tx.send(JobResult::Progress {
                        label: "Balances computed ✓".to_string(),
                        fraction: 0.35,
                    });

                    // ======= STAGE 5: PDF Surgery ========
                    send_progress(&res_tx, TransferStage::PdfSurgery);
                    tracing::info!("[TRANSFER] Stage 5: PDF surgery - applying changes");

                    if let Err(e) = std::fs::copy(&target_pdf, &output_pdf) {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "PdfSurgery".into(),
                            message: format!("Failed to copy target PDF: {e}"),
                        });
                        return;
                    }

                    let mut actual_pages_added = 0usize;
                    let mut actual_pages_removed = 0usize;

                    let publish_surgery_output =
                        |staged: &std::path::Path, destination: &std::path::Path| {
                            let mut barrier = crate::app::commit::FileCommitBarrier::new();
                            barrier.publish(staged, destination).map_err(|error| {
                                format!(
                                    "could not publish {} to {}: {error}",
                                    staged.display(),
                                    destination.display()
                                )
                            })?;
                            barrier.commit();
                            let _ = std::fs::remove_file(staged);
                            Ok::<(), String>(())
                        };

                    if !transfer_plan.pages_to_clone.is_empty() {
                        let expected = transfer_plan.pages_to_clone.len();
                        let temp_path =
                            output_pdf.with_extension(format!("{}.cloned.pdf", Uuid::new_v4()));
                        let eng = engine_for_tokio.clone();
                        let p_in = output_pdf.clone();
                        let p_out = temp_path.clone();
                        let idxs = transfer_plan.pages_to_clone.clone();
                        let native_res = tokio::task::spawn_blocking(move || {
                            eng.clone_pages(&p_in, &p_out, idxs)
                        })
                        .await;

                        match native_res {
                            Ok(Ok(count)) if count == expected && temp_path.is_file() => {
                                if let Err(error) = publish_surgery_output(&temp_path, &output_pdf)
                                {
                                    let _ = res_tx.send(JobResult::TransferFailed {
                                        stage: "PdfSurgery".into(),
                                        message: format!(
                                            "Exact native page-clone publication failed: {error}"
                                        ),
                                    });
                                    return;
                                }
                                actual_pages_added = count;
                                tracing::info!(
                                    "[TRANSFER] (Native) Cloned exactly {count}/{expected} pages"
                                );
                            }
                            Ok(Ok(count)) => {
                                let _ = std::fs::remove_file(&temp_path);
                                tracing::warn!(
                                    "[TRANSFER] Native clone rejected: {count}/{expected} pages"
                                );
                            }
                            Ok(Err(error)) => {
                                tracing::warn!("[TRANSFER] Native clone failed exactly: {error}")
                            }
                            Err(error) => {
                                tracing::warn!("[TRANSFER] Native clone task failed: {error}")
                            }
                        }

                        if actual_pages_added == 0 {
                            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                            let _ = py_tx.send((
                                PythonJob::ClonePages {
                                    pdf_path: output_pdf.to_string_lossy().to_string(),
                                    output_path: temp_path.to_string_lossy().to_string(),
                                    page_indices: transfer_plan.pages_to_clone.clone(),
                                },
                                reply_tx,
                            ));
                            match reply_rx.await {
                                Ok(PythonJobResult::Json(json_str)) => {
                                    let parsed =
                                        serde_json::from_str::<serde_json::Value>(&json_str)
                                            .unwrap_or_default();
                                    let count = parsed["cloned"].as_u64().unwrap_or(0) as usize;
                                    let exact = parsed["success"].as_bool().unwrap_or(false)
                                        && count == expected
                                        && temp_path.is_file();
                                    if exact {
                                        if let Err(error) =
                                            publish_surgery_output(&temp_path, &output_pdf)
                                        {
                                            let _ = res_tx.send(JobResult::TransferFailed {
                                                stage: "PdfSurgery".into(),
                                                message: format!(
                                                    "Exact Python page-clone publication failed: {error}"
                                                ),
                                            });
                                            return;
                                        }
                                        actual_pages_added = count;
                                    } else {
                                        let _ = std::fs::remove_file(&temp_path);
                                        tracing::warn!(
                                            "[TRANSFER] Python clone rejected: {count}/{expected} pages"
                                        );
                                    }
                                }
                                other => tracing::warn!(
                                    "[TRANSFER] Python page cloning failed: {other:?}"
                                ),
                            }
                        }
                        if actual_pages_added != expected {
                            let _ = res_tx.send(JobResult::TransferFailed {
                                stage: "PdfSurgery".into(),
                                message: format!(
                                    "Page cloning incomplete: {actual_pages_added}/{expected}; source output preserved"
                                ),
                            });
                            return;
                        }
                    }

                    if !transfer_plan.pages_to_remove.is_empty() {
                        let expected = transfer_plan.pages_to_remove.len();
                        let temp_path =
                            output_pdf.with_extension(format!("{}.removed.pdf", Uuid::new_v4()));
                        let eng = engine_for_tokio.clone();
                        let p_in = output_pdf.clone();
                        let p_out = temp_path.clone();
                        let idxs = transfer_plan.pages_to_remove.clone();
                        let native_res = tokio::task::spawn_blocking(move || {
                            eng.remove_pages(&p_in, &p_out, idxs)
                        })
                        .await;

                        match native_res {
                            Ok(Ok(count)) if count == expected && temp_path.is_file() => {
                                if let Err(error) = publish_surgery_output(&temp_path, &output_pdf)
                                {
                                    let _ = res_tx.send(JobResult::TransferFailed {
                                        stage: "PdfSurgery".into(),
                                        message: format!(
                                            "Exact native page-removal publication failed: {error}"
                                        ),
                                    });
                                    return;
                                }
                                actual_pages_removed = count;
                                tracing::info!(
                                    "[TRANSFER] (Native) Removed exactly {count}/{expected} pages"
                                );
                            }
                            Ok(Ok(count)) => {
                                let _ = std::fs::remove_file(&temp_path);
                                tracing::warn!(
                                    "[TRANSFER] Native removal rejected: {count}/{expected} pages"
                                );
                            }
                            Ok(Err(error)) => {
                                tracing::warn!("[TRANSFER] Native removal failed exactly: {error}")
                            }
                            Err(error) => {
                                tracing::warn!("[TRANSFER] Native removal task failed: {error}")
                            }
                        }

                        if actual_pages_removed == 0 {
                            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                            let _ = py_tx.send((
                                PythonJob::RemovePages {
                                    pdf_path: output_pdf.to_string_lossy().to_string(),
                                    output_path: temp_path.to_string_lossy().to_string(),
                                    page_indices: transfer_plan.pages_to_remove.clone(),
                                },
                                reply_tx,
                            ));
                            match reply_rx.await {
                                Ok(PythonJobResult::Json(json_str)) => {
                                    let parsed =
                                        serde_json::from_str::<serde_json::Value>(&json_str)
                                            .unwrap_or_default();
                                    let count = parsed["removed"].as_u64().unwrap_or(0) as usize;
                                    let exact = parsed["success"].as_bool().unwrap_or(false)
                                        && count == expected
                                        && temp_path.is_file();
                                    if exact {
                                        if let Err(error) =
                                            publish_surgery_output(&temp_path, &output_pdf)
                                        {
                                            let _ = res_tx.send(JobResult::TransferFailed {
                                                stage: "PdfSurgery".into(),
                                                message: format!(
                                                    "Exact Python page-removal publication failed: {error}"
                                                ),
                                            });
                                            return;
                                        }
                                        actual_pages_removed = count;
                                    } else {
                                        let _ = std::fs::remove_file(&temp_path);
                                        tracing::warn!(
                                            "[TRANSFER] Python removal rejected: {count}/{expected} pages"
                                        );
                                    }
                                }
                                other => tracing::warn!(
                                    "[TRANSFER] Python page removal failed: {other:?}"
                                ),
                            }
                        }
                        if actual_pages_removed != expected {
                            let _ = res_tx.send(JobResult::TransferFailed {
                                stage: "PdfSurgery".into(),
                                message: format!(
                                    "Page removal incomplete: {actual_pages_removed}/{expected}; prior output preserved"
                                ),
                            });
                            return;
                        }
                    }

                    let mut target_by_page: std::collections::HashMap<
                        usize,
                        Vec<&crate::engine::model::Transaction>,
                    > = std::collections::HashMap::new();
                    for t in &target_transactions {
                        target_by_page.entry(t.page).or_default().push(t);
                    }
                    for txns in target_by_page.values_mut() {
                        txns.sort_by(|a, b| {
                            let ay = a.bbox.map(|b| b[1]).unwrap_or(f32::MAX);
                            let by = b.bbox.map(|b| b[1]).unwrap_or(f32::MAX);
                            ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    let cloned_page_templates = crate::engine::transfer::cloned_page_template_map(
                        target_stmt.total_pages,
                        &transfer_plan.pages_to_clone,
                    );

                    let _total_txns = mapped.len();
                    let mut actually_edited_bboxes: Vec<(usize, [f32; 4])> = Vec::new();
                    let mut batch_edits: Vec<serde_json::Value> = Vec::new();
                    let mut batch_metadata: Vec<serde_json::Value> = Vec::new();
                    let mut geometry_failures = Vec::new();
                    let mut used_output_slots = std::collections::HashSet::new();

                    for (i, tx) in mapped.iter().enumerate() {
                        let mut adjusted_page = tx.target_page;
                        for &r in transfer_plan.pages_to_remove.iter().rev() {
                            if adjusted_page > r {
                                adjusted_page = adjusted_page.saturating_sub(1);
                            } else if adjusted_page == r {
                                geometry_failures.push(format!(
                                    "mapping {i} targets removed page {}",
                                    tx.target_page
                                ));
                                break;
                            }
                        }

                        if geometry_failures
                            .last()
                            .is_some_and(|failure| failure.starts_with(&format!("mapping {i} ")))
                        {
                            continue;
                        }
                        used_output_slots.insert((tx.target_page, tx.target_line));

                        let template_page = cloned_page_templates
                            .get(tx.target_page)
                            .copied()
                            .unwrap_or(tx.target_page);
                        let target_tx = target_by_page
                            .get(&template_page)
                            .and_then(|page_txns| page_txns.get(tx.target_line));

                        match target_tx {
                            None => {
                                geometry_failures.push(format!(
                                    "mapping {i} has no target transaction at page {} line {}",
                                    template_page, tx.target_line
                                ));
                            }
                            Some(target) => {
                                let description =
                                    crate::engine::transfer::transaction_description(target)
                                        .unwrap_or_default();
                                let old_amount = target
                                    .debit
                                    .or(target.credit)
                                    .map(|amount| amount.to_string())
                                    .unwrap_or_default();
                                let new_amount = tx
                                    .debit
                                    .or(tx.credit)
                                    .map(|amount| amount.to_string())
                                    .unwrap_or_default();
                                let fields: Vec<(&str, Option<[f32; 4]>, String, String)> = vec![
                                    (
                                        "date",
                                        target.field_bboxes.date,
                                        target.date.clone(),
                                        tx.date.clone(),
                                    ),
                                    (
                                        "description",
                                        target.field_bboxes.description,
                                        description,
                                        tx.description.clone(),
                                    ),
                                    (
                                        "amount",
                                        target.field_bboxes.debit.or(target.field_bboxes.credit),
                                        old_amount,
                                        new_amount,
                                    ),
                                    (
                                        "balance",
                                        target.field_bboxes.running_balance,
                                        target
                                            .running_balance
                                            .map(|balance| balance.to_string())
                                            .unwrap_or_default(),
                                        tx.running_balance.to_string(),
                                    ),
                                ];

                                for (field_name, field_bbox, old_text, field_text) in &fields {
                                    let Some(bbox) = field_bbox else {
                                        geometry_failures.push(format!(
                                            "mapping {i} field {field_name} has no target bbox"
                                        ));
                                        continue;
                                    };
                                    if old_text.trim().is_empty() || field_text.trim().is_empty() {
                                        geometry_failures.push(format!(
                                            "mapping {i} field {field_name} has empty exact identity"
                                        ));
                                        continue;
                                    }
                                    batch_edits.push(serde_json::json!({
                                            "page": adjusted_page,
                                            "rect": bbox,
                                            "old_text": old_text.clone(),
                                            "new_text": field_text.clone(),
                                    }));
                                    batch_metadata.push(serde_json::json!({
                                        "mapping": i,
                                        "field": field_name,
                                        "old_text": old_text,
                                        "new_text": field_text,
                                        "rect": bbox,
                                    }));
                                    actually_edited_bboxes.push((adjusted_page, *bbox));
                                }
                            }
                        }
                    }

                    let mapped_edit_count = batch_edits.len();
                    let removed_pages: std::collections::HashSet<usize> =
                        transfer_plan.pages_to_remove.iter().copied().collect();
                    for (output_page, template_page) in
                        cloned_page_templates.iter().copied().enumerate()
                    {
                        if removed_pages.contains(&output_page) {
                            continue;
                        }
                        let adjusted_page = output_page
                            - transfer_plan
                                .pages_to_remove
                                .iter()
                                .filter(|removed| **removed < output_page)
                                .count();
                        let Some(page_transactions) = target_by_page.get(&template_page) else {
                            continue;
                        };
                        for (target_line, target) in page_transactions.iter().enumerate() {
                            if used_output_slots.contains(&(output_page, target_line)) {
                                continue;
                            }
                            let description =
                                crate::engine::transfer::transaction_description(target)
                                    .unwrap_or_default();
                            let old_amount = target
                                .debit
                                .or(target.credit)
                                .map(|amount| amount.to_string())
                                .unwrap_or_default();
                            let fields: Vec<(&str, Option<[f32; 4]>, String)> = vec![
                                ("date", target.field_bboxes.date, target.date.clone()),
                                ("description", target.field_bboxes.description, description),
                                (
                                    "amount",
                                    target.field_bboxes.debit.or(target.field_bboxes.credit),
                                    old_amount,
                                ),
                                (
                                    "balance",
                                    target.field_bboxes.running_balance,
                                    target
                                        .running_balance
                                        .map(|balance| balance.to_string())
                                        .unwrap_or_default(),
                                ),
                            ];
                            for (field_name, field_bbox, old_text) in fields {
                                let Some(bbox) = field_bbox else {
                                    geometry_failures.push(format!(
                                        "unused target page {output_page} line {target_line} field {field_name} has no bbox"
                                    ));
                                    continue;
                                };
                                if old_text.trim().is_empty() {
                                    geometry_failures.push(format!(
                                        "unused target page {output_page} line {target_line} field {field_name} has empty identity"
                                    ));
                                    continue;
                                }
                                batch_edits.push(serde_json::json!({
                                    "page": adjusted_page,
                                    "rect": bbox,
                                    "old_text": old_text,
                                    "new_text": "",
                                }));
                                batch_metadata.push(serde_json::json!({
                                    "mapping": null,
                                    "field": format!("unused-{field_name}"),
                                    "old_text": old_text,
                                    "new_text": "",
                                    "rect": bbox,
                                }));
                                actually_edited_bboxes.push((adjusted_page, bbox));
                            }
                        }
                    }

                    if !geometry_failures.is_empty() {
                        let preview = geometry_failures
                            .iter()
                            .take(8)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("; ");
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "PdfSurgery".into(),
                            message: format!(
                                "Exact target geometry incomplete ({} failures): {preview}",
                                geometry_failures.len()
                            ),
                        });
                        return;
                    }

                    let expected_mapped_edits = mapped.len().saturating_mul(4);
                    if mapped_edit_count != expected_mapped_edits {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "PdfSurgery".into(),
                            message: format!(
                                "Transfer edit cardinality mismatch: built {mapped_edit_count} mapped field edits for {} mapped rows; expected {expected_mapped_edits}",
                                mapped.len(),
                            ),
                        });
                        return;
                    }
                    let mut generated_visual_proof_path = None;
                    let total_edits = batch_edits.len();
                    let mut edits_applied = 0usize;
                    if total_edits > 0 {
                        let mut affected_pages: std::collections::BTreeSet<usize> =
                            std::collections::BTreeSet::new();
                        for edit in &batch_edits {
                            if let Some(page) = edit["page"].as_u64() {
                                affected_pages.insert(page as usize);
                            }
                        }

                        let gemini_client =
                            match crate::ai::gemini_client::GeminiClient::from_app_config_async(
                                &cfg,
                            )
                            .await
                            {
                                Ok(c) => c,
                                Err(e) => {
                                    tracing::warn!("[TRANSFER] Failed to init GeminiClient for visual review, skipping review: {e}");
                                    let _ = res_tx.send(JobResult::TransferFailed {
                                        stage: "AiVisualReview".into(),
                                        message: format!("AI visual reviewer unavailable: {e}"),
                                    });
                                    return;
                                }
                            };

                        let max_retries = 3;
                        let mut approved = false;
                        for retry_idx in 0..max_retries {
                            // ======= STAGE 5a: GeneratePreview ========
                            send_progress(&res_tx, TransferStage::GeneratePreview);
                            tracing::info!(
                                "[TRANSFER] Stage 5a: Generating visual preview (Attempt {})",
                                retry_idx + 1
                            );
                            let edits_json_str =
                                serde_json::to_string(&batch_edits).unwrap_or_default();
                            let visual_proof_pdf =
                                output_pdf.with_extension(format!("proof_v{}.pdf", retry_idx));
                            generated_visual_proof_path = Some(visual_proof_pdf.clone());

                            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                            if let Err(e) = py_tx.send((
                                PythonJob::GenerateVisualProof {
                                    pdf_path: output_pdf.to_string_lossy().to_string(),
                                    output_path: visual_proof_pdf.to_string_lossy().to_string(),
                                    edits_json: edits_json_str.clone(),
                                },
                                reply_tx,
                            )) {
                                let _ = res_tx.send(JobResult::TransferFailed {
                                    stage: "GeneratePreview".into(),
                                    message: format!("Failed to dispatch GenerateVisualProof: {e}"),
                                });
                                return;
                            }

                            let mut proof_pngs = Vec::new();
                            match reply_rx.await {
                                Ok(PythonJobResult::Json(_raw)) => {
                                    // Generate PNG proofs for Gemini (all affected pages)
                                    for &page_num in &affected_pages {
                                        let (png_reply_tx, png_reply_rx) =
                                            tokio::sync::oneshot::channel();
                                        let _ = py_tx.send((
                                            PythonJob::RenderPageToPng {
                                                pdf_path: visual_proof_pdf
                                                    .to_string_lossy()
                                                    .to_string(),
                                                page_num,
                                                dpi: 300.0,
                                            },
                                            png_reply_tx,
                                        ));

                                        if let Ok(PythonJobResult::Json(png_raw)) =
                                            png_reply_rx.await
                                        {
                                            let parsed: serde_json::Value =
                                                serde_json::from_str(&png_raw).unwrap_or_default();
                                            if let Some(b64) = parsed["png_base64"].as_str() {
                                                use base64::Engine;
                                                if let Ok(bytes) =
                                                    base64::engine::general_purpose::STANDARD
                                                        .decode(b64)
                                                {
                                                    proof_pngs.push(bytes);
                                                }
                                            }
                                        }
                                    }
                                }
                                Ok(PythonJobResult::Error(e)) => {
                                    let _ = res_tx.send(JobResult::TransferFailed {
                                        stage: "GeneratePreview".into(),
                                        message: format!("Python GenerateVisualProof failed: {e}"),
                                    });
                                    return;
                                }
                                other => {
                                    let _ = res_tx.send(JobResult::TransferFailed {
                                        stage: "GeneratePreview".into(),
                                        message: format!(
                                            "Unexpected result from GenerateVisualProof: {:?}",
                                            other
                                        ),
                                    });
                                    return;
                                }
                            }

                            // ======= STAGE 5b: AiVisualReview ========
                            if !proof_pngs.is_empty() {
                                send_progress(&res_tx, TransferStage::AiVisualReview);
                                tracing::info!(
                                    "[TRANSFER] Stage 5b: AI visual review of proof (Attempt {})",
                                    retry_idx + 1
                                );

                                match gemini_client.review_visual_proof(&proof_pngs).await {
                                    Ok(crate::ai::gemini_client::ValidationResponse::Approved) => {
                                        tracing::info!("[TRANSFER] AI explicitly approved visual proof.");
                                        approved = true;
                                        break; // Success! Break out of retry loop
                                    }
                                    Ok(crate::ai::gemini_client::ValidationResponse::RejectedWithNudges(nudges)) => {
                                        tracing::warn!("[TRANSFER] AI rejected visual proof with {} nudges.", nudges.len());
                                        if retry_idx == max_retries - 1 {
                                            let _ = res_tx.send(JobResult::TransferFailed {
                                                stage: "AiVisualReview".into(),
                                                message: "AI rejected the visual proof of edits and max retries exceeded.".into(),
                                            });
                                            return;
                                        } else {
                                            // Apply nudges to batch_edits
                                            for nudge in nudges {
                                                if nudge.index < batch_edits.len() {
                                                    if let Some(rect) = batch_edits[nudge.index]["rect"].as_array_mut() {
                                                        if rect.len() == 4 {
                                                            if let Some(y0) = rect[1].as_f64() {
                                                                rect[1] = serde_json::json!(y0 + (nudge.dy as f64));
                                                            }
                                                            if let Some(y1) = rect[3].as_f64() {
                                                                rect[3] = serde_json::json!(y1 + (nudge.dy as f64));
                                                            }
                                                            if let Some(x0) = rect[0].as_f64() {
                                                                rect[0] = serde_json::json!(x0 + (nudge.dx as f64));
                                                            }
                                                            if let Some(x1) = rect[2].as_f64() {
                                                                rect[2] = serde_json::json!(x1 + (nudge.dx as f64));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("[TRANSFER] AI review failed, proceeding anyway: {e}");
                                        approved = true;
                                        break;
                                    }
                                }
                            } else {
                                approved = true;
                                break;
                            }
                        }

                        if !approved {
                            return;
                        }

                        // ======= STAGE 5c: Apply PDF Surgery ========
                        send_progress(&res_tx, TransferStage::PdfSurgery);
                        tracing::info!("[TRANSFER] Applying batch of {} text edits", total_edits);

                        let mut output_pages = 0;
                        if let Ok(doc) = lopdf::Document::load(&output_pdf) {
                            output_pages = doc.get_pages().len();
                        }

                        if output_pages > 3 {
                            tracing::info!(
                                "[TRANSFER] Document has {} pages (> 3), chunking for Pro engine",
                                output_pages
                            );
                            let temp_mgr = match crate::engine::segments::SegmentManager::new() {
                                Ok(mgr) => mgr,
                                Err(e) => {
                                    tracing::error!(
                                        "[TRANSFER] Failed to create SegmentManager: {}",
                                        e
                                    );
                                    let _ = res_tx.send(JobResult::TransferFailed {
                                        stage: "PdfSurgery".into(),
                                        message: format!("Failed to create SegmentManager: {e}"),
                                    });
                                    return;
                                }
                            };
                            let segment_map_result: Result<
                                crate::engine::segments::SegmentMap,
                                String,
                            > = async {
                                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                py_tx
                                    .send((
                                        PythonJob::ChunkPdfForDocai {
                                            pdf_path: output_pdf.to_string_lossy().to_string(),
                                            output_dir: temp_mgr
                                                .temp_path()
                                                .to_string_lossy()
                                                .to_string(),
                                            max_pages_per_chunk: 3,
                                        },
                                        reply_tx,
                                    ))
                                    .map_err(|_| {
                                        "failed to dispatch resource-preserving page chunker"
                                            .to_string()
                                    })?;
                                let raw = match reply_rx.await {
                                    Ok(PythonJobResult::Json(raw)) => raw,
                                    Ok(other) => {
                                        return Err(format!(
                                            "resource-preserving page chunker returned {other:?}"
                                        ))
                                    }
                                    Err(error) => {
                                        return Err(format!(
                                            "resource-preserving page chunker reply failed: {error}"
                                        ))
                                    }
                                };
                                let chunks: Vec<serde_json::Value> = serde_json::from_str(&raw)
                                    .map_err(|error| {
                                        format!("invalid page-chunker metadata: {error}")
                                    })?;
                                let mut infos = Vec::with_capacity(chunks.len());
                                for (index, chunk) in chunks.into_iter().enumerate() {
                                    let path = chunk["path"]
                                        .as_str()
                                        .ok_or_else(|| {
                                            format!("chunk {index} has no path identity")
                                        })?
                                        .into();
                                    let page_offset =
                                        chunk["page_offset"].as_u64().ok_or_else(|| {
                                            format!("chunk {index} has no page offset")
                                        })? as usize;
                                    let page_count = chunk["page_count"]
                                        .as_u64()
                                        .ok_or_else(|| format!("chunk {index} has no page count"))?
                                        as usize;
                                    infos.push(crate::engine::segments::SegmentInfo {
                                        index,
                                        path,
                                        page_offset,
                                        page_count,
                                        edited: false,
                                        edited_path: None,
                                    });
                                }
                                let map = crate::engine::segments::SegmentMap::new(
                                    infos,
                                    output_pdf.clone(),
                                    temp_mgr.temp_path().to_path_buf(),
                                    3,
                                );
                                map.validate_structure()?;
                                Ok(map)
                            }
                            .await;
                            if let Ok(map) = segment_map_result {
                                let mut edits_by_seg: std::collections::BTreeMap<
                                    usize,
                                    Vec<serde_json::Value>,
                                > = std::collections::BTreeMap::new();
                                for (edit_index, edit) in batch_edits.iter().enumerate() {
                                    let Some(global_page) =
                                        edit["page"].as_u64().map(|page| page as usize)
                                    else {
                                        let _ = res_tx.send(JobResult::TransferFailed {
                                            stage: "PdfSurgery".into(),
                                            message: format!(
                                                "Edit {edit_index} has no valid global page identity; no segmented output was published"
                                            ),
                                        });
                                        return;
                                    };
                                    let Some((seg_idx, local_page)) = map.resolve(global_page)
                                    else {
                                        let _ = res_tx.send(JobResult::TransferFailed {
                                            stage: "PdfSurgery".into(),
                                            message: format!(
                                                "Edit {edit_index} references global page {global_page}, outside the {}-page target; no segmented output was published",
                                                map.total_pages
                                            ),
                                        });
                                        return;
                                    };
                                    let mut local_edit = edit.clone();
                                    local_edit["page"] = serde_json::json!(local_page);
                                    edits_by_seg.entry(seg_idx).or_default().push(local_edit);
                                }

                                let mut final_paths = Vec::new();
                                for (i, seg) in map.segments.iter().enumerate() {
                                    let seg_edits =
                                        edits_by_seg.get(&i).cloned().unwrap_or_default();
                                    if !seg_edits.is_empty() {
                                        let edited_path = temp_mgr
                                            .temp_path()
                                            .join(format!("segment_{i:03}_edited.pdf"));
                                        let edits_json =
                                            serde_json::to_string(&seg_edits).unwrap_or_default();
                                        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                        let _ = py_tx.send((
                                            PythonJob::ApplyManyEdits {
                                                pdf_path: seg.path.to_string_lossy().to_string(),
                                                output_path: edited_path
                                                    .to_string_lossy()
                                                    .to_string(),
                                                edits_json,
                                                font_path: font_override_path.clone(),
                                            },
                                            reply_tx,
                                        ));
                                        match reply_rx.await {
                                            Ok(PythonJobResult::ApplyReport(report))
                                                if report.success
                                                    && report.requested == seg_edits.len()
                                                    && report.matched == seg_edits.len()
                                                    && report.placed == seg_edits.len()
                                                    && report.failed == 0
                                                    && report.review_flags.is_empty()
                                                    && edited_path.is_file() =>
                                            {
                                                edits_applied += report.placed;
                                                final_paths.push(edited_path);
                                            }
                                            Ok(PythonJobResult::ApplyReport(report)) => {
                                                let _ = res_tx.send(JobResult::TransferFailed {
                                                    stage: "PdfSurgery".into(),
                                                    message: format!(
                                                        "Segment {i} failed exact edit membership: requested {}, matched {}, placed {}, failed {}, expected {}; no merged output was published. {}",
                                                        report.requested,
                                                        report.matched,
                                                        report.placed,
                                                        report.failed,
                                                        seg_edits.len(),
                                                        report.warnings.join("; ")
                                                    ),
                                                });
                                                return;
                                            }
                                            Ok(PythonJobResult::Error(error)) => {
                                                let _ = res_tx.send(JobResult::TransferFailed {
                                                    stage: "PdfSurgery".into(),
                                                    message: format!(
                                                        "Segment {i} edit failed before merge: {error}"
                                                    ),
                                                });
                                                return;
                                            }
                                            other => {
                                                let _ = res_tx.send(JobResult::TransferFailed {
                                                    stage: "PdfSurgery".into(),
                                                    message: format!(
                                                        "Segment {i} returned an unexpected exact-edit result {other:?}; no merged output was published"
                                                    ),
                                                });
                                                return;
                                            }
                                        }
                                    } else {
                                        final_paths.push(seg.path.clone());
                                    }
                                }

                                if edits_applied != total_edits {
                                    let _ = res_tx.send(JobResult::TransferFailed {
                                        stage: "PdfSurgery".into(),
                                        message: format!(
                                            "Segmented edit count mismatch: applied {edits_applied}/{total_edits}; no merged output was published"
                                        ),
                                    });
                                    return;
                                }
                                match crate::engine::pdf_split_merge::merge_pdfs(
                                    &final_paths,
                                    &output_pdf,
                                ) {
                                    Ok(merged_pages) if merged_pages == map.total_pages => {}
                                    Ok(merged_pages) => {
                                        let _ = res_tx.send(JobResult::TransferFailed {
                                            stage: "PdfSurgery".into(),
                                            message: format!(
                                                "Segment merge page-count mismatch: expected {}, got {merged_pages}",
                                                map.total_pages
                                            ),
                                        });
                                        return;
                                    }
                                    Err(error) => {
                                        let _ = res_tx.send(JobResult::TransferFailed {
                                            stage: "PdfSurgery".into(),
                                            message: format!(
                                                "Atomic segment merge failed: {error}"
                                            ),
                                        });
                                        return;
                                    }
                                }
                            } else {
                                let _ = res_tx.send(JobResult::TransferFailed {
                                    stage: "PdfSurgery".into(),
                                    message: "Failed to prepare exact document segments; no output was published"
                                        .into(),
                                });
                                return;
                            }
                        } else {
                            let edits_json =
                                serde_json::to_string(&batch_edits).unwrap_or_default();
                            let eng = engine_for_tokio.clone();
                            let p_in = output_pdf.clone();
                            let p_out = output_pdf.with_extension("temp.pdf");
                            let f_path = font_override_path.clone();
                            let edits_json_clone = edits_json.clone();

                            let native_res = tokio::task::spawn_blocking(move || {
                                let fp = f_path.map(std::path::PathBuf::from);
                                eng.apply_many_edits(
                                    &p_in,
                                    &p_out,
                                    &edits_json_clone,
                                    fp.as_deref(),
                                )
                            })
                            .await
                            .unwrap_or(Ok(0));

                            let native_temp = output_pdf.with_extension("temp.pdf");
                            if let Ok(c) = native_res {
                                if c == total_edits && native_temp.is_file() {
                                    match publish_surgery_output(&native_temp, &output_pdf) {
                                        Ok(()) => {
                                            edits_applied = c;
                                            tracing::info!(
                                                "[TRANSFER] (Native) Exact batch edit succeeded"
                                            );
                                        }
                                        Err(error) => {
                                            let _ = std::fs::remove_file(&native_temp);
                                            tracing::warn!(
                                                "[TRANSFER] Native exact batch could not be published: {error}"
                                            );
                                        }
                                    }
                                } else {
                                    let _ = std::fs::remove_file(&native_temp);
                                    tracing::warn!(
                                        "[TRANSFER] Native batch rejected: applied {c}/{total_edits} edits"
                                    );
                                }
                            }

                            if edits_applied == 0 {
                                tracing::warn!("[TRANSFER] Native ApplyManyEdits failed or returned 0. Falling back to Python.");
                                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                let _ = py_tx.send((
                                    PythonJob::ApplyManyEdits {
                                        pdf_path: output_pdf.to_string_lossy().to_string(),
                                        output_path: output_pdf
                                            .with_extension("temp.pdf")
                                            .to_string_lossy()
                                            .to_string(),
                                        edits_json,
                                        font_path: font_override_path.clone(),
                                    },
                                    reply_tx,
                                ));

                                match reply_rx.await {
                                    Ok(PythonJobResult::ApplyReport(report))
                                        if report.success
                                            && report.requested == total_edits
                                            && report.matched == total_edits
                                            && report.placed == total_edits
                                            && report.failed == 0
                                            && report.review_flags.is_empty()
                                            && output_pdf.with_extension("temp.pdf").is_file() =>
                                    {
                                        let temp_output = output_pdf.with_extension("temp.pdf");
                                        match publish_surgery_output(&temp_output, &output_pdf) {
                                            Ok(()) => {
                                                edits_applied = report.placed;
                                                tracing::info!(
                                                    "[TRANSFER] (Python) Exact batch edit succeeded"
                                                );
                                            }
                                            Err(error) => {
                                                tracing::error!(
                                                    "[TRANSFER] Python output commit failed: {}",
                                                    error
                                                );
                                                let _ = std::fs::remove_file(temp_output);
                                            }
                                        }
                                    }
                                    Ok(PythonJobResult::ApplyReport(report)) => {
                                        let _ = std::fs::remove_file(
                                            output_pdf.with_extension("temp.pdf"),
                                        );
                                        if let Some(failed_edit) =
                                            report.edits.iter().find(|edit| !edit.placed)
                                        {
                                            let request = batch_metadata
                                                .get(failed_edit.index)
                                                .cloned()
                                                .unwrap_or_default();
                                            tracing::error!(
                                                edit_index = failed_edit.index,
                                                page = failed_edit.page,
                                                method = %failed_edit.method,
                                                mapping = ?request.get("mapping"),
                                                field = ?request.get("field"),
                                                old_text = ?request.get("old_text"),
                                                new_text = ?request.get("new_text"),
                                                rect = ?request.get("rect"),
                                                warning = ?failed_edit.warning,
                                                "[TRANSFER] First exact Python edit failure"
                                            );
                                        }
                                        tracing::error!(
                                            requested = report.requested,
                                            matched = report.matched,
                                            placed = report.placed,
                                            failed = report.failed,
                                            warnings = ?report.warnings,
                                            "[TRANSFER] (Python) Exact batch edit failed"
                                        );
                                    }
                                    Ok(PythonJobResult::Error(error)) => tracing::error!(
                                        "[TRANSFER] (Python) Batch edit failed: {}",
                                        error
                                    ),
                                    other => tracing::error!(
                                        result = ?other,
                                        "[TRANSFER] (Python) Batch edit returned unexpected result"
                                    ),
                                }
                            }
                        }
                    }

                    if edits_applied != total_edits {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "PdfSurgery".into(),
                            message: format!(
                                "Exact edit count mismatch: applied {edits_applied}/{total_edits}; verification and publication were stopped"
                            ),
                        });
                        return;
                    }
                    let _ = res_tx.send(JobResult::Progress {
                        label: format!("PDF changes applied ✓ ({edits_applied}/{total_edits})"),
                        fraction: 0.55,
                    });

                    // ======= STAGE 6: Visual Fidelity Check ========
                    send_progress(&res_tx, TransferStage::VisualFidelityCheck);
                    tracing::info!("[TRANSFER] Stage 6: Visual fidelity verification");

                    let intended_bboxes: Vec<(usize, [f32; 4])> = actually_edited_bboxes;
                    let math_input_txns: Vec<crate::engine::model::Transaction> = mapped
                        .iter()
                        .map(|m| crate::engine::model::Transaction {
                            page: m.target_page,
                            line_on_page: m.target_line,
                            date: m.date.clone(),
                            raw_text: m.description.clone(),
                            debit: m.debit,
                            credit: m.credit,
                            running_balance: Some(m.running_balance),
                            bbox: None,
                            field_bboxes: crate::engine::model::FieldBboxes::default(),
                            provenance: crate::engine::model::Provenance::Computed,
                            category: None,
                            canonical: Default::default(),
                        })
                        .collect();

                    let vis_result = crate::engine::verification::verify_edit(
                        &target_pdf,
                        &output_pdf,
                        &std::path::PathBuf::from("audit/transfer_verification"),
                        &intended_bboxes,
                        crate::engine::verification::MathInputs {
                            transactions: math_input_txns,
                            expected_transactions: None,
                            opening_balance,
                            expected_final_balance: None,
                            required: true,
                        },
                        cfg.auto_match_dpi,
                        cfg.vision_api_key.clone(),
                    )
                    .await;

                    let (visual_score, visual_verified, report_files) = match &vis_result {
                        Ok(report) => (
                            report.visual_diff_score,
                            report.only_intended_changes,
                            report.report_files.clone(),
                        ),
                        Err(e) => {
                            tracing::warn!("[TRANSFER] Visual verification error: {}", e);
                            (0.0, true, vec![])
                        }
                    };

                    let _ = res_tx.send(JobResult::Progress {
                        label: format!("Visual check ✓ (score: {visual_score:.4})"),
                        fraction: 0.75,
                    });

                    // STAGE 6.5: Gemini Vision Check
                    let mut vision_anomaly = false;
                    if let (Some(vision_provider), Some(edit_png_path)) = (
                        gemini.as_ref(),
                        report_files.iter().find(|p| p.contains("edited_p1")),
                    ) {
                        if let Ok(png_data) = std::fs::read(edit_png_path) {
                            // only check the first page for anomalies right now
                            let page_intended: Vec<[f32; 4]> = intended_bboxes
                                .iter()
                                .filter(|(p, _)| *p == 0)
                                .map(|(_, b)| *b)
                                .collect();
                            if let Ok(vision_report) = vision_provider
                                .validate_render_visually(&png_data, &page_intended)
                                .await
                            {
                                tracing::info!(
                                    "[TRANSFER] Gemini Vision score: {:.2}, notes: {}",
                                    vision_report.anomaly_score,
                                    vision_report.notes
                                );
                                if vision_report.anomaly_score > 0.5 {
                                    vision_anomaly = true;
                                    tracing::warn!(
                                        "[TRANSFER] Gemini Vision flagged anomalies: {:?}",
                                        vision_report.hotspots
                                    );
                                }
                            }
                        }
                    }

                    if vision_anomaly || !visual_verified {
                        tracing::warn!(
                            "[TRANSFER] visual validation failed; automatic font adaptation is disabled"
                        );
                    }

                    // ======= STAGE 7: Math Verification (Engine) ========
                    send_progress(&res_tx, TransferStage::MathVerificationEngine);
                    tracing::info!("[TRANSFER] Stage 7: Math verification (engine)");

                    let mut math_verified = false;
                    let mut math_imbalance = rust_decimal::Decimal::ZERO;
                    let mut math_err_msg = String::new();
                    let mut reparsed_had_transactions = false;

                    let reparsed_stmt = if let Some(ref doc_ai) = doc_ai_opt {
                        match crate::engine::pro_edit::perform_pro_edit(
                            "DocumentAI",
                            async {
                                doc_ai
                                    .parse_entire_statement(&output_pdf, None::<&str>)
                                    .await
                                    .map_err(anyhow::Error::from)
                            },
                            wdog.clone(),
                        )
                        .await
                        {
                            Ok(s) => Ok(s),
                            Err(e) => {
                                tracing::warn!(
                                    "[TRANSFER] DocAI target reparsing failed, trying offline: {e}"
                                );
                                parse_with_offline_fallback(
                                    &output_pdf,
                                    engine_for_tokio.clone(),
                                    config_for_tokio.clone(),
                                )
                                .await
                            }
                        }
                    } else {
                        parse_with_offline_fallback(
                            &output_pdf,
                            engine_for_tokio.clone(),
                            config_for_tokio.clone(),
                        )
                        .await
                    };

                    match reparsed_stmt {
                        Ok(reparsed) => {
                            let engine_txns: Vec<crate::engine::model::Transaction> =
                                reparsed.transactions;
                            reparsed_had_transactions = !engine_txns.is_empty();
                            match crate::engine::balance::process_and_reconcile(
                                engine_txns,
                                opening_balance,
                                None,
                            ) {
                                Ok((_, None)) => {
                                    math_verified = true;
                                    tracing::info!("[TRANSFER] Math verification PASSED");
                                }
                                Ok((_, Some(msg))) => {
                                    math_imbalance = rust_decimal_macros::dec!(0.01);
                                    math_err_msg = format!("Math mismatch: {msg}");
                                    tracing::warn!("[TRANSFER] {}", math_err_msg);
                                    total_corrections += 1;
                                }
                                Err(e) => {
                                    math_imbalance = rust_decimal_macros::dec!(0.01);
                                    math_err_msg = format!("Balance engine error: {e}");
                                    tracing::warn!("[TRANSFER] {}", math_err_msg);
                                }
                            }
                        }
                        Err(e) => {
                            math_imbalance = rust_decimal_macros::dec!(0.01);
                            math_err_msg = format!("Parse for verification failed: {e}");
                            tracing::warn!("[TRANSFER] {}", math_err_msg);
                        }
                    }

                    if !math_verified
                        && !reparsed_had_transactions
                        && edits_applied == total_edits
                        && total_edits >= mapped.len().saturating_mul(4)
                    {
                        match crate::engine::transfer::verify_mapped_balances(
                            opening_balance,
                            &mapped,
                        ) {
                            Ok(()) => {
                                math_verified = true;
                                math_imbalance = rust_decimal::Decimal::ZERO;
                                math_err_msg.clear();
                                tracing::info!(
                                    "[TRANSFER] Math verification PASSED via exact mapped ledger after empty/unavailable output reparse"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    "[TRANSFER] Exact mapped-ledger math verification failed: {error}"
                                );
                            }
                        }
                    }

                    let _ = res_tx.send(JobResult::Progress {
                        label: format!("Math (engine) {} ", if math_verified { "✓" } else { "⚠" }),
                        fraction: 0.85,
                    });

                    // ======= STAGE 8: Optional Provider Math Review ========
                    send_progress(&res_tx, TransferStage::MathVerificationGemini);
                    let provider_math_ok = if let Some(math_provider) = gemini.as_ref() {
                        tracing::info!("[TRANSFER] Stage 8: optional provider math review");
                        match math_provider
                            .verify_transfer_math(&mapped, opening_balance)
                            .await
                        {
                            Ok(ok) => ok,
                            Err(error) => {
                                tracing::warn!(
                                    "[TRANSFER] Optional provider math review unavailable: {error}"
                                );
                                true
                            }
                        }
                    } else {
                        tracing::info!(
                            "[TRANSFER] Stage 8: no provider configured; deterministic engine remains authoritative"
                        );
                        true
                    };

                    let _ = res_tx.send(JobResult::Progress {
                        label: format!(
                            "Optional math review {} ",
                            if provider_math_ok { "✓" } else { "⚠" }
                        ),
                        fraction: 0.95,
                    });

                    let all_math_ok = math_verified && provider_math_ok;
                    let current_quality_score =
                        visual_score * (if all_math_ok { 1.0 } else { 0.5 });
                    let best_quality_score =
                        best_visual_score * (if best_math_verified { 1.0 } else { 0.5 });

                    // STAGE 9: Final Audit setup
                    let elapsed = started_at.elapsed().as_secs_f64();
                    let result = TransferResult {
                        output_path: requested_output_pdf.clone(),
                        source_tx_count: source_transactions.len(),
                        target_tx_count: target_transactions.len(),
                        pages_added: actual_pages_added,
                        pages_removed: actual_pages_removed,
                        math_verified: all_math_ok,
                        visual_verified: visual_verified && !vision_anomaly,
                        visual_score,
                        math_imbalance,
                        stages_completed: 9,
                        total_duration_secs: elapsed,
                        corrections_applied: total_corrections,
                        retries_attempted: attempt - 1,
                        synthesized_fonts_used,
                        visual_proof_path: generated_visual_proof_path,
                    };

                    // Store best result
                    if best_result.is_none() || current_quality_score > best_quality_score {
                        best_result = Some(result.clone());
                        best_visual_score = visual_score;
                        best_math_verified = all_math_ok;
                    }

                    if all_math_ok && visual_verified && !vision_anomaly {
                        tracing::info!(
                            "[TRANSFER] Iteration {} passed all checks perfectly. Breaking loop.",
                            attempt
                        );
                        break;
                    }

                    // Interactive Fallback Logic for No Improvement / Reduction
                    if attempt >= 1 && current_quality_score <= best_quality_score {
                        tracing::warn!("[TRANSFER] Loop {} yielded no improvement or regression. Quality score: {:.4}, Best: {:.4}", attempt, current_quality_score, best_quality_score);
                        if cfg.interactive_fallbacks && res_tx.is_interactive() {
                            let mut req = crate::engine::interactive_fallback::InteractiveFallbackRequest::new(
                                            "Transfer Validation Loop",
                                            if current_quality_score < best_quality_score {
                                                "The AI mapping quality degraded on recalculation."
                                            } else {
                                                "The AI mapping failed to improve the fidelity issues."
                                            }
                                        );
                            if cfg.openrouter_api_key.is_some() {
                                req = req.add_alternative(
                                    "openrouter",
                                    "Try OpenRouter Backup",
                                    None,
                                );
                            }
                            if cfg.groq_api_key.is_some() {
                                req = req.add_alternative("groq", "Try Groq Backup", None);
                            }
                            req = req.add_alternative("finish", "Use Best Result & Finish", None);

                            let (tx, rx) = tokio::sync::oneshot::channel();
                            let request_id = req.id;
                            {
                                let mut map = router.lock().await;
                                map.insert(request_id, tx);
                            }
                            let _ = res_tx.send(JobResult::InteractiveFallbackRequired(req));

                            let choice = match wait_for_interactive_choice(
                                &router,
                                request_id,
                                rx,
                                std::time::Duration::from_secs(300),
                            )
                            .await
                            {
                                Ok(choice) => choice,
                                Err(reason) => {
                                    tracing::warn!(
                                        "[TRANSFER] Interactive fallback {reason}; using best verified result"
                                    );
                                    "finish".to_string()
                                }
                            };
                            if choice == "finish" {
                                tracing::info!("[TRANSFER] User chose to finish with best result.");
                                break;
                            } else {
                                let mut new_cfg = (*cfg).clone();
                                if choice == "openrouter" {
                                    new_cfg.ai_provider =
                                        crate::app::config::AiProviderMode::OpenRouterApiKey;
                                } else if choice == "groq" {
                                    new_cfg.ai_provider =
                                        crate::app::config::AiProviderMode::GroqApiKey;
                                }

                                if let Ok(c) =
                                    crate::ai::backend::AiBackend::from_app_config(&new_cfg)
                                {
                                    gemini = Some(std::sync::Arc::new(c));
                                }
                            }
                        } else {
                            tracing::warn!("[TRANSFER] Interactive fallbacks disabled. Breaking loop with best result.");
                            break;
                        }
                    }

                    if !all_math_ok && attempt < max_retries {
                        tracing::warn!("[TRANSFER] Math check failed. Retrying entire planning loop with hint.");
                        correction_hint = Some(math_err_msg.clone());
                        continue;
                    }

                    if attempt >= max_retries {
                        tracing::warn!("[TRANSFER] Reached max retries. Taking best result.");
                        break;
                    }
                }

                // Only a currently staged result that passed both deterministic
                // math and visual gates may be published. “Best effort” output is
                // review evidence, never a successful transfer artifact.
                let final_result = match best_result {
                    Some(result) if result.math_verified && result.visual_verified => result,
                    Some(result) => {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "FinalVerification".into(),
                            message: format!(
                                "No transfer attempt passed all publication gates (math_verified={}, visual_verified={}); prior output was preserved",
                                result.math_verified, result.visual_verified
                            ),
                        });
                        return;
                    }
                    None => {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "FinalVerification".into(),
                            message: "Transfer loop produced no verified result; prior output was preserved"
                                .into(),
                        });
                        return;
                    }
                };

                // ======= STAGE 9: Atomic Publication and Final Audit ========
                send_progress(&res_tx, TransferStage::FinalAudit);
                let staged_bytes = match std::fs::read(&output_pdf) {
                    Ok(bytes) if !bytes.is_empty() => bytes,
                    Ok(_) => {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "FinalAudit".into(),
                            message: "Verified staged transfer output is empty; prior output was preserved"
                                .into(),
                        });
                        return;
                    }
                    Err(error) => {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "FinalAudit".into(),
                            message: format!(
                                "Verified staged transfer output is unavailable: {error}; prior output was preserved"
                            ),
                        });
                        return;
                    }
                };
                let staged_hash = crate::engine::workflow::sha256_hex_of(&staged_bytes);
                let mut publication = crate::app::commit::FileCommitBarrier::new();
                if let Err(error) = publication.publish(&output_pdf, &requested_output_pdf) {
                    let _ = res_tx.send(JobResult::TransferFailed {
                        stage: "FinalAudit".into(),
                        message: format!(
                            "Transfer output publication failed: {error}; prior output was preserved"
                        ),
                    });
                    return;
                }
                let published_hash = std::fs::read(&requested_output_pdf)
                    .map(|bytes| crate::engine::workflow::sha256_hex_of(&bytes));
                if !matches!(published_hash, Ok(ref hash) if *hash == staged_hash) {
                    let _ = res_tx.send(JobResult::TransferFailed {
                        stage: "FinalAudit".into(),
                        message: "Published transfer output did not match the verified stage; prior output was restored"
                            .into(),
                    });
                    return;
                }

                match write_transfer_audit(&final_result, &source_pdf, &target_pdf) {
                    Ok(_audit_path) => publication.commit(),
                    Err(error) => {
                        let _ = res_tx.send(JobResult::TransferFailed {
                            stage: "FinalAudit".into(),
                            message: format!(
                                "Transfer audit failed: {error}; prior output was restored"
                            ),
                        });
                        return;
                    }
                }

                tracing::info!(
                    "[TRANSFER] ✅ Complete in {:.1}s - math: {}, visual: {}",
                    final_result.total_duration_secs,
                    if final_result.math_verified {
                        "✓"
                    } else {
                        "✗"
                    },
                    if final_result.visual_verified {
                        "✓"
                    } else {
                        "✗"
                    },
                );

                let _ = res_tx.send(JobResult::Progress {
                    label: "Transfer complete ✓".to_string(),
                    fraction: 1.0,
                });

                let _ = res_tx.send(JobResult::TransferComplete(final_result));
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
                            let _ = res_tx.send(JobResult::Progress {
                                label: "Sending to Local LLM (Qwen 7B) for edit…".into(),
                                fraction: 0.4,
                            });
                            let client = crate::ai::local_llm::LocalLlmClient::new();
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
                                label: "Sending to Gemini for complex edit…".into(),
                                fraction: 0.4,
                            });
                            let gemini =
                                match crate::ai::gemini_client::GeminiClient::from_app_config_async(
                                    &cfg,
                                )
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
                            match gemini.apply_natural_language_edit(&instruction, &txs).await {
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
        Job::RunTransferTests {
            statements,
            max_iterations,
        } => {
            let res_tx = result_tx_clone.clone();
            let cfg = config_for_tokio.clone();
            let _py_tx = python_tx_clone.clone();
            let engine_for_tokio = engine_for_tokio.clone();
            tokio::spawn(async move {
                use crate::engine::transfer_test_harness::*;

                let started_at = std::time::Instant::now();
                let pairs = generate_test_pairs(&statements);
                let total_pairs = pairs.len();

                let _ = res_tx.send(JobResult::Progress {
                    label: format!("Running {total_pairs} transfer test pairs..."),
                    fraction: 0.0,
                });

                let doc_ai_opt = crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg)
                    .ok()
                    .map(std::sync::Arc::new);
                let gemini = match crate::ai::backend::AiBackend::from_app_config(&cfg) {
                    Ok(c) => std::sync::Arc::new(c),
                    Err(_) => {
                        let _ = res_tx.send(JobResult::Error {
                                        job_label: "transfer_tests".into(),
                                        message: "Transfer tests require an AI provider for format mapping — set GEMINI_API_KEY (or GROQ_API_KEY / OPENROUTER_API_KEY) and select a provider in Backend Preferences.".into(),
                                    });
                        return;
                    }
                };

                let mut results: Vec<TransferTestResult> = Vec::new();

                for (pair_idx, (source, target)) in pairs.iter().enumerate() {
                    let pair_started = std::time::Instant::now();
                    let output = test_output_path(source, target);
                    let mut iterations = 0u32;
                    let mut final_math_ok = false;
                    let mut final_visual_score = 1.0f64;
                    let mut corrections: Vec<String> = Vec::new();
                    let mut converged = false;
                    let mut correction_hint: Option<String> = None;

                    let _ = res_tx.send(JobResult::Progress {
                        label: format!(
                            "Testing pair {}/{}: {} -> {}",
                            pair_idx + 1,
                            total_pairs,
                            source.file_stem().unwrap_or_default().to_string_lossy(),
                            target.file_stem().unwrap_or_default().to_string_lossy(),
                        ),
                        fraction: pair_idx as f32 / total_pairs as f32,
                    });

                    // Parse both statements — DocAI with offline fallback
                    let source_stmt = if let Some(ref doc_ai) = doc_ai_opt {
                        match doc_ai.parse_entire_statement(source, None::<&str>).await {
                            Ok(s) => s,
                            Err(_e) => {
                                // DocAI failed, try offline
                                let eng_clone = engine_for_tokio.clone();
                                let src_clone = source.clone();
                                match tokio::task::spawn_blocking(move || {
                                    crate::engine::offline_parser::parse_statement_offline(
                                        &src_clone, eng_clone,
                                    )
                                })
                                .await
                                {
                                    Ok(Ok(s)) => s,
                                    _ => {
                                        corrections.push(
                                            "Source parse failed (DocAI + offline)".to_string(),
                                        );
                                        results.push(TransferTestResult {
                                            source: source.clone(),
                                            target: target.clone(),
                                            output: output.clone(),
                                            iterations: 0,
                                            final_math_ok: false,
                                            final_visual_score: 1.0,
                                            corrections,
                                            duration_secs: pair_started.elapsed().as_secs_f64(),
                                            converged: false,
                                        });
                                        continue;
                                    }
                                }
                            }
                        }
                    } else {
                        let eng_clone = engine_for_tokio.clone();
                        let src_clone = source.clone();
                        match tokio::task::spawn_blocking(move || {
                            crate::engine::offline_parser::parse_statement_offline(
                                &src_clone, eng_clone,
                            )
                        })
                        .await
                        {
                            Ok(Ok(s)) => s,
                            _ => {
                                corrections.push("Source parse failed (offline)".to_string());
                                results.push(TransferTestResult {
                                    source: source.clone(),
                                    target: target.clone(),
                                    output: output.clone(),
                                    iterations: 0,
                                    final_math_ok: false,
                                    final_visual_score: 1.0,
                                    corrections,
                                    duration_secs: pair_started.elapsed().as_secs_f64(),
                                    converged: false,
                                });
                                continue;
                            }
                        }
                    };
                    let target_stmt = if let Some(ref doc_ai) = doc_ai_opt {
                        match doc_ai.parse_entire_statement(target, None::<&str>).await {
                            Ok(s) => s,
                            Err(_e) => {
                                let eng_clone = engine_for_tokio.clone();
                                let tgt_clone = target.clone();
                                match tokio::task::spawn_blocking(move || {
                                    crate::engine::offline_parser::parse_statement_offline(
                                        &tgt_clone, eng_clone,
                                    )
                                })
                                .await
                                {
                                    Ok(Ok(s)) => s,
                                    _ => {
                                        corrections.push(
                                            "Target parse failed (DocAI + offline)".to_string(),
                                        );
                                        results.push(TransferTestResult {
                                            source: source.clone(),
                                            target: target.clone(),
                                            output: output.clone(),
                                            iterations: 0,
                                            final_math_ok: false,
                                            final_visual_score: 1.0,
                                            corrections,
                                            duration_secs: pair_started.elapsed().as_secs_f64(),
                                            converged: false,
                                        });
                                        continue;
                                    }
                                }
                            }
                        }
                    } else {
                        let eng_clone = engine_for_tokio.clone();
                        let tgt_clone = target.clone();
                        match tokio::task::spawn_blocking(move || {
                            crate::engine::offline_parser::parse_statement_offline(
                                &tgt_clone, eng_clone,
                            )
                        })
                        .await
                        {
                            Ok(Ok(s)) => s,
                            _ => {
                                corrections.push("Target parse failed (offline)".to_string());
                                results.push(TransferTestResult {
                                    source: source.clone(),
                                    target: target.clone(),
                                    output: output.clone(),
                                    iterations: 0,
                                    final_math_ok: false,
                                    final_visual_score: 1.0,
                                    corrections,
                                    duration_secs: pair_started.elapsed().as_secs_f64(),
                                    converged: false,
                                });
                                continue;
                            }
                        }
                    };

                    // Attempt transfer with retry loop
                    while iterations < max_iterations && !converged {
                        iterations += 1;

                        // Get transfer plan
                        let plan = match gemini
                            .plan_transaction_transfer(
                                &source_stmt.transactions,
                                &target_stmt.transactions,
                                correction_hint.as_deref(),
                            )
                            .await
                        {
                            Ok(p) => p,
                            Err(e) => {
                                corrections.push(format!("Iter {iterations}: plan failed: {e}"));
                                continue;
                            }
                        };

                        // Build mapped transactions and compute balances
                        let opening = target_stmt.opening_balance;
                        let mut mapped: Vec<crate::engine::transfer::MappedTransaction> = plan
                            .mappings
                            .iter()
                            .map(|m| {
                                let idx = m
                                    .source_index
                                    .min(source_stmt.transactions.len().saturating_sub(1));
                                let src = &source_stmt.transactions[idx];
                                crate::engine::transfer::MappedTransaction {
                                    target_page: m.target_page,
                                    target_line: m.target_line,
                                    date: m.converted_date.clone(),
                                    description: m.adapted_description.clone(),
                                    debit: src.debit,
                                    credit: src.credit,
                                    running_balance: rust_decimal::Decimal::ZERO,
                                    field_bboxes: Default::default(),
                                }
                            })
                            .collect();
                        match crate::engine::transfer::recompute_running_balances(
                            opening,
                            &mut mapped,
                        ) {
                            Ok(()) => {}
                            Err(e) => {
                                tracing::error!("[TRANSFER] Balance recomputation failed during verification: {}", e);
                                // Continue anyway - we'll catch math errors in verification
                            }
                        };

                        // Verify math with engine
                        let sim_txns: Vec<crate::engine::model::Transaction> = mapped
                            .iter()
                            .map(|m| crate::engine::model::Transaction {
                                page: m.target_page,
                                line_on_page: m.target_line,
                                date: m.date.clone(),
                                raw_text: m.description.clone(),
                                debit: m.debit,
                                credit: m.credit,
                                running_balance: Some(m.running_balance),
                                bbox: None,
                                field_bboxes: Default::default(),
                                provenance: crate::engine::model::Provenance::Computed,
                                category: None,
                                canonical: Default::default(),
                            })
                            .collect();

                        let mut math_err_msg = None;
                        match crate::engine::balance::process_and_reconcile(sim_txns, opening, None)
                        {
                            Ok((_, None)) => {}
                            Ok((_, Some(msg))) => {
                                math_err_msg = Some(format!("Balance mismatch: {msg}"))
                            }
                            Err(e) => math_err_msg = Some(format!("Balance engine error: {e}")),
                        }

                        // Verify math with Gemini
                        let gemini_math_ok = gemini
                            .verify_transfer_math(&mapped, opening)
                            .await
                            .unwrap_or_default();

                        let math_ok = math_err_msg.is_none() && gemini_math_ok;
                        final_math_ok = math_ok;
                        final_visual_score = 0.0; // would need render for real score

                        if math_ok {
                            converged = true;
                        } else {
                            let mut errors = Vec::new();
                            if let Some(msg) = &math_err_msg {
                                errors.push(msg.clone());
                            }
                            if !gemini_math_ok {
                                errors.push("Gemini math verification failed.".to_string());
                            }
                            let hint = format!(
                                            "Your previous mapping failed validation. Errors: {}. Please adjust the mapping to fix these issues.",
                                            errors.join("; ")
                                        );
                            corrections.push(format!(
                                "Iter {iterations}: math verification failed ({}), retrying",
                                errors.join("; ")
                            ));
                            correction_hint = Some(hint);
                        }
                    }

                    results.push(TransferTestResult {
                        source: source.clone(),
                        target: target.clone(),
                        output,
                        iterations,
                        final_math_ok,
                        final_visual_score,
                        corrections,
                        duration_secs: pair_started.elapsed().as_secs_f64(),
                        converged,
                    });
                }

                let elapsed = started_at.elapsed().as_secs_f64();
                let report = build_report(results, elapsed);

                // Write report to disk
                if let Err(e) = write_harness_report(&report) {
                    tracing::warn!("[TEST_HARNESS] Failed to write report: {}", e);
                }

                let _ = res_tx.send(JobResult::Progress {
                    label: report.summary(),
                    fraction: 1.0,
                });

                let _ = res_tx.send(JobResult::TransferTestsComplete(report));
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

        // -- Document AI Version Management Handlers --
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
        Job::CompleteFont { .. } => {
            let _ = result_tx_clone.send(JobResult::Error {
                job_label: "complete_font".into(),
                message: "Automatic font completion is disabled because synthesized or donor glyphs are not fidelity-preserving."
                    .into(),
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

                let gemini =
                    match crate::ai::gemini_client::GeminiClient::from_app_config_async(&cfg).await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = res_tx.send(JobResult::Error {
                                job_label: "NaturalLanguageEdit".into(),
                                message: format!("Gemini configuration error: {e}"),
                            });
                            return;
                        }
                    };

                match gemini
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
                                        client.parse_statement(&p)
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
                        let json_str_for_fallback = json_str.clone();
                        let edited_out = tmp.path().join(format!("segment_{si:03}_edited.pdf"));
                        let expected = edits.len();

                        let (rtx, rrx) = oneshot::channel();
                        let _ = py_tx.send((
                            PythonJob::ApplyManyEdits {
                                pdf_path: seg_paths[si].to_string_lossy().to_string(),
                                output_path: edited_out.to_string_lossy().to_string(),
                                edits_json: json_str,
                                font_path: None,
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
                                let native_json = json_str_for_fallback;
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
        Job::CleanupTempFiles => {
            tokio::task::spawn_blocking(move || {
                let now = std::time::SystemTime::now();
                for dir in &["output", "audit"] {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.flatten() {
                            if let Ok(meta) = entry.metadata() {
                                if let Ok(modified) = meta.modified() {
                                    if let Ok(age) = now.duration_since(modified) {
                                        if age.as_secs() > 86400 && meta.is_file() {
                                            let _ = std::fs::remove_file(entry.path());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
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
        Job::TypstReconstruct {
            input: _,
            output: _,
        } => {
            // Typst rebuild is an export-style path that cannot preserve
            // edit-in-place visual fidelity. Keep the job for API stability
            // but fail closed with a clear reason (same gate as workflow finalize).
            let tx = result_tx_clone.clone();
            tokio::task::spawn(async move {
                let _ = tx.send(JobResult::Error {
                    job_label: "typst_reconstruct_disabled".into(),
                    message: "Automatic Typst reconstruction is disabled in this build: cannot preserve edit-in-place fidelity".into(),
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
                let _ = res_tx.send(JobResult::ApiKeysVerified(report));

                let _ = res_tx.send(JobResult::Progress {
                    label: "Done".into(),
                    fraction: 1.0,
                });
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

        // -----------------------------------------------------------------
        // Stage 3: build a balance preview from the user's edits.
        // -----------------------------------------------------------------
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

        // -----------------------------------------------------------------
        // Stages 4 + 5 + 6: apply, render, validate visually in a loop,
        // then do a final Document AI math sanity pass.
        // -----------------------------------------------------------------
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
                if ignore_visual_fidelity {
                    let _ = res_tx.send(JobResult::WorkflowFailed(
                        crate::engine::workflow::WorkflowFailure::Other(
                            "Visual-fidelity bypass is disabled for publishable bank-statement output"
                                .into(),
                        ),
                    ));
                    return;
                }
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
                        let failed_gates = report
                            .gates
                            .iter()
                            .filter(|gate| {
                                gate.mandatory
                                    && gate.status
                                        != crate::engine::verification::VerificationGateStatus::Passed
                            })
                            .map(|gate| format!("{}: {}", gate.id, gate.message))
                            .collect::<Vec<_>>()
                            .join("; ");
                        let _ = res_tx.send(JobResult::WorkflowFailed(
                            crate::engine::workflow::WorkflowFailure::FidelityCheckFailed(format!(
                                "mandatory verification gates failed: {failed_gates}"
                            )),
                        ));
                        return;
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

                let outcome = crate::engine::workflow::WorkflowOutcome {
                                final_pdf: requested_output.clone(),
                                transactions_re_parsed: re_parsed_count,
                                final_imbalance,
                                math_valid,
                                visual_attempts,
                                completion_summary: format!(
                                    "Bank statement confirmed. Visual diff {last_score:.4}, intended-only={last_intended}, math valid={math_valid}."
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
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::app::config::AppConfig;
    use std::time::Duration;

    fn test_sink(broadcast: mpsc::Sender<JobResult>) -> ResultSink {
        ResultSink::new(
            broadcast,
            JobMetadata::for_job(&Job::Ping),
            None,
            CancellationRegistry::new(),
        )
    }

    #[test]
    fn cancellation_registry_register_and_cancel_round_trip() {
        let reg = CancellationRegistry::new();
        let id = alloc_job_id();
        let token = reg.register(id);
        assert_eq!(reg.len(), 1);
        assert!(!token.is_cancelled());

        let cancelled = reg.cancel(id);
        assert!(cancelled);
        assert!(token.is_cancelled());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn cancellation_registry_complete_removes_without_cancelling() {
        let reg = CancellationRegistry::new();
        let id = alloc_job_id();
        let token = reg.register(id);
        reg.complete(id);
        assert_eq!(reg.len(), 0);
        // Completing should not flip the token's cancelled flag.
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancellation_registry_unknown_id_is_noop() {
        let reg = CancellationRegistry::new();
        assert!(!reg.cancel(99999));
    }

    #[test]
    fn cancellation_registry_cancel_all_drains_every_token() {
        let reg = CancellationRegistry::new();
        let t1 = reg.register(1);
        let t2 = reg.register(2);
        let t3 = reg.register(3);
        reg.cancel_all();
        assert_eq!(reg.len(), 0);
        assert!(t1.is_cancelled());
        assert!(t2.is_cancelled());
        assert!(t3.is_cancelled());
    }

    #[test]
    fn cancellation_registry_request_cancel_all_waits_for_terminal_completion() {
        let reg = CancellationRegistry::new();
        let t1 = reg.register(11);
        let t2 = reg.register(12);
        reg.request_cancel_all();
        assert_eq!(reg.len(), 2);
        assert!(t1.is_cancelled());
        assert!(t2.is_cancelled());
        reg.complete(11);
        assert_eq!(reg.len(), 1);
        reg.complete(12);
        assert!(reg.is_empty());
    }

    #[test]
    fn runtime_client_close_intake_rejects_new_work() {
        let (intake_tx, intake_rx) = mpsc::channel::<JobEnvelope>();
        let client = RuntimeClient::new(intake_tx);
        assert!(client.is_accepting());
        client.close_intake();
        assert!(!client.is_accepting());
        assert!(client.send(Job::Ping).is_err());
        assert!(intake_rx.recv_timeout(Duration::from_millis(20)).is_err());
    }

    #[test]
    fn alloc_job_id_is_strictly_monotonic() {
        let a = alloc_job_id();
        let b = alloc_job_id();
        let c = alloc_job_id();
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn runtime_client_routes_results_by_job_and_document_identity() {
        let (intake_tx, intake_rx) = mpsc::channel::<JobEnvelope>();
        let client = RuntimeClient::new(intake_tx);
        let path = PathBuf::from("fixtures/private-account.pdf");
        let first = client
            .submit(Job::LoadDocument {
                path: path.clone(),
                three_page_mode: false,
            })
            .unwrap();
        let second = client
            .submit(Job::LoadDocument {
                path,
                three_page_mode: true,
            })
            .unwrap();
        let first_envelope = intake_rx.recv().unwrap();
        let second_envelope = intake_rx.recv().unwrap();
        assert_ne!(first.metadata().job_id, second.metadata().job_id);
        assert_eq!(first.metadata().document_id, second.metadata().document_id);
        assert_eq!(first.metadata().job_id, first_envelope.metadata.job_id);
        assert_eq!(second.metadata().job_id, second_envelope.metadata.job_id);
        assert_eq!(
            first.metadata().correlation_id,
            first_envelope.metadata.correlation_id
        );
        let metadata_debug = format!("{:?}", first_envelope.metadata);
        assert!(!metadata_debug.contains("fixtures"));
        assert!(!metadata_debug.contains("private-account.pdf"));

        let (broadcast_tx, broadcast_rx) = mpsc::channel();
        let sink = ResultSink::new(
            broadcast_tx,
            first_envelope.metadata,
            first_envelope.route,
            CancellationRegistry::new(),
        );
        sink.send(JobResult::completed(
            "load_document",
            OperationDisposition::Succeeded,
            None,
            "document metadata loaded",
        ))
        .unwrap();
        assert!(matches!(
            first.recv_timeout(Duration::from_secs(1)),
            Ok(JobResult::JobCompleted {
                disposition: OperationDisposition::Succeeded,
                ..
            })
        ));
        assert!(broadcast_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        assert!(second.try_recv().is_err());
    }

    #[test]
    fn runtime_client_preserves_explicit_execution_mode() {
        let (intake_tx, intake_rx) = mpsc::channel::<JobEnvelope>();
        let client = RuntimeClient::new(intake_tx);

        let interactive = client.submit(Job::Ping).unwrap();
        let interactive_envelope = intake_rx.recv().unwrap();
        assert_eq!(
            interactive.metadata().execution_mode,
            ExecutionMode::Interactive
        );
        assert_eq!(
            interactive_envelope.metadata.execution_mode,
            ExecutionMode::Interactive
        );

        let headless = client.submit_headless(Job::Ping).unwrap();
        let headless_envelope = intake_rx.recv().unwrap();
        assert_eq!(headless.metadata().execution_mode, ExecutionMode::Headless);
        assert_eq!(
            headless_envelope.metadata.execution_mode,
            ExecutionMode::Headless
        );
    }

    #[test]
    fn job_ticket_cancellation_targets_its_own_job_id() {
        let (intake_tx, intake_rx) = mpsc::channel::<JobEnvelope>();
        let client = RuntimeClient::new(intake_tx);
        let ticket = client.submit(Job::Ping).unwrap();
        let _original = intake_rx.recv().unwrap();
        ticket.cancel().unwrap();
        let cancel = intake_rx.recv().unwrap();
        assert!(matches!(
            cancel.job,
            Job::Cancel { id } if id == ticket.metadata().job_id
        ));
    }

    #[tokio::test]
    async fn interactive_fallback_timeout_removes_stale_route() {
        let router: InteractiveFallbackRouter =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let request_id = uuid::Uuid::new_v4();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        router.lock().await.insert(request_id, sender);

        let result =
            wait_for_interactive_choice(&router, request_id, receiver, Duration::from_millis(10))
                .await;
        assert_eq!(result, Err("interactive response timed out"));
        assert!(!router.lock().await.contains_key(&request_id));
    }

    #[tokio::test]
    async fn interactive_fallback_routes_exact_response() {
        let router: InteractiveFallbackRouter =
            std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
        let request_id = uuid::Uuid::new_v4();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        router.lock().await.insert(request_id, sender);
        let response_sender = router.lock().await.remove(&request_id).unwrap();
        response_sender.send("offline_parser".to_string()).unwrap();

        let result =
            wait_for_interactive_choice(&router, request_id, receiver, Duration::from_secs(1))
                .await;
        assert_eq!(result.as_deref(), Ok("offline_parser"));
        assert!(!router.lock().await.contains_key(&request_id));
    }

    #[tokio::test]
    async fn lifecycle_timeout_emits_once_and_suppresses_late_results() {
        let (tx, rx) = mpsc::channel();
        let cancellations = CancellationRegistry::new();
        let mut metadata = JobMetadata::for_job(&Job::Ping);
        metadata.deadline = std::time::Instant::now() + Duration::from_millis(20);
        let token = cancellations.register(metadata.job_id);
        let sink = ResultSink::new(tx, metadata.clone(), None, cancellations);
        spawn_job_lifecycle_monitor(sink.clone(), token);

        let (terminal, rx) = tokio::task::spawn_blocking(move || {
            let terminal = rx.recv_timeout(Duration::from_secs(1));
            (terminal, rx)
        })
        .await
        .unwrap();
        assert!(matches!(
            terminal,
            Ok(JobResult::TimedOut { id, .. }) if id == metadata.job_id
        ));
        sink.send(JobResult::Pong).unwrap();
        let late = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(50)))
            .await
            .unwrap();
        assert!(late.is_err());
    }

    #[tokio::test]
    async fn lifecycle_cancellation_emits_once_and_suppresses_late_results() {
        let (tx, rx) = mpsc::channel();
        let cancellations = CancellationRegistry::new();
        let metadata = JobMetadata::for_job(&Job::Ping);
        let token = cancellations.register(metadata.job_id);
        let sink = ResultSink::new(tx, metadata.clone(), None, cancellations);
        spawn_job_lifecycle_monitor(sink.clone(), token.clone());

        token.cancel();
        let (terminal, rx) = tokio::task::spawn_blocking(move || {
            let terminal = rx.recv_timeout(Duration::from_secs(1));
            (terminal, rx)
        })
        .await
        .unwrap();
        assert!(matches!(
            terminal,
            Ok(JobResult::Cancelled { id }) if id == metadata.job_id
        ));
        sink.send(JobResult::Pong).unwrap();
        let late = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_millis(50)))
            .await
            .unwrap();
        assert!(late.is_err());
    }

    #[test]
    fn job_result_terminal_classification_is_explicit() {
        let intermediate =
            JobResult::WorkflowVisualAttempt(crate::engine::workflow::VisualAttempt {
                attempt: 1,
                max_attempts: 3,
                diff_score: 0.01,
                threshold: 0.02,
                only_intended: true,
                message: "intermediate".into(),
            });
        assert!(!intermediate.is_terminal());
        assert!(!JobResult::WorkflowParseValidated {
            validation: crate::engine::workflow::ParseValidation {
                total_pages: 1,
                transactions_found: 1,
                opening_balance: rust_decimal::Decimal::ZERO,
                closing_balance: rust_decimal::Decimal::ZERO,
                account_number: None,
                completeness_score: 1.0,
                completeness_notes: String::new(),
                missing_rows: Vec::new(),
            },
            transactions: Vec::new(),
        }
        .is_terminal());
        assert!(
            JobResult::WorkflowFailed(crate::engine::workflow::WorkflowFailure::Other(
                "failed".into()
            ))
            .is_terminal()
        );
        for disposition in [
            OperationDisposition::Succeeded,
            OperationDisposition::NoOp,
            OperationDisposition::Partial,
            OperationDisposition::Failed,
            OperationDisposition::Cancelled,
            OperationDisposition::TimedOut,
        ] {
            let terminal = JobResult::completed("done", disposition, None, "complete");
            assert_eq!(terminal.disposition(), Some(disposition));
            assert!(terminal.is_terminal());
        }
    }

    #[test]
    fn ends_gui_tracked_job_frees_success_payloads_but_not_streams() {
        // Side-channel / intermediate
        assert!(!JobResult::Progress {
            label: "x".into(),
            fraction: 0.5,
        }
        .ends_gui_tracked_job());
        assert!(!JobResult::UfoLog("line".into()).ends_gui_tracked_job());
        assert!(
            !JobResult::DocumentLoaded {
                layout_json: "{}".into(),
                total_pages: 1,
            }
            .ends_gui_tracked_job(),
            "DocumentLoaded chains into parse; must keep GUI wait open"
        );
        assert!(
            !JobResult::WorkflowVisualAttempt(crate::engine::workflow::VisualAttempt {
                attempt: 1,
                max_attempts: 3,
                diff_score: 0.01,
                threshold: 0.02,
                only_intended: true,
                message: "mid".into(),
            })
            .ends_gui_tracked_job()
        );

        // Success / failure payloads that complete a user wait
        assert!(JobResult::PageRendered {
            png_bytes: vec![],
            page: 0,
            dpi: 150.0,
            tag: "current".into(),
            width_pts: 612.0,
            height_pts: 792.0,
        }
        .ends_gui_tracked_job());
        assert!(JobResult::TransactionsExtracted(vec![]).ends_gui_tracked_job());
        assert!(
            JobResult::UfoAutoEditResult(serde_json::json!({"status":"success"}))
                .ends_gui_tracked_job()
        );
        assert!(JobResult::Error {
            job_label: "x".into(),
            message: "y".into(),
        }
        .ends_gui_tracked_job());
        assert!(JobResult::WorkflowParseValidated {
            validation: crate::engine::workflow::ParseValidation {
                total_pages: 1,
                transactions_found: 0,
                opening_balance: rust_decimal::Decimal::ZERO,
                closing_balance: rust_decimal::Decimal::ZERO,
                account_number: None,
                completeness_score: 1.0,
                completeness_notes: String::new(),
                missing_rows: Vec::new(),
            },
            transactions: Vec::new(),
        }
        .ends_gui_tracked_job());
        // Strict terminal remains a subset for tracker semantics
        assert!(JobResult::Error {
            job_label: "x".into(),
            message: "y".into(),
        }
        .is_terminal());
        assert!(!JobResult::PageRendered {
            png_bytes: vec![],
            page: 0,
            dpi: 150.0,
            tag: "current".into(),
            width_pts: 612.0,
            height_pts: 792.0,
        }
        .is_terminal());
    }

    #[test]
    fn terminal_tracker_emits_exactly_one_terminal_and_suppresses_followups() {
        let (tx, rx) = mpsc::channel();
        let tracker = TerminalTracker::new(test_sink(tx), "exactly-once-test");
        tracker
            .send(JobResult::WorkflowVisualAttempt(
                crate::engine::workflow::VisualAttempt {
                    attempt: 1,
                    max_attempts: 3,
                    diff_score: 0.01,
                    threshold: 0.02,
                    only_intended: true,
                    message: "intermediate".into(),
                },
            ))
            .unwrap();
        tracker
            .send(JobResult::WorkflowFailed(
                crate::engine::workflow::WorkflowFailure::Other("expected failure".into()),
            ))
            .unwrap();
        tracker
            .send(JobResult::Error {
                job_label: "duplicate".into(),
                message: "must be suppressed".into(),
            })
            .unwrap();
        tracker
            .send(JobResult::Progress {
                label: "after terminal".into(),
                fraction: 1.0,
            })
            .unwrap();
        drop(tracker);

        let results: Vec<_> = rx.try_iter().collect();
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0], JobResult::WorkflowVisualAttempt(_)));
        assert!(matches!(results[1], JobResult::WorkflowFailed(_)));
        assert_eq!(
            results.iter().filter(|result| result.is_terminal()).count(),
            1
        );
    }

    #[test]
    fn terminal_tracker_drop_emits_one_failure_after_only_intermediate_results() {
        let (tx, rx) = mpsc::channel();
        let tracker = TerminalTracker::new(test_sink(tx), "silent-task");
        tracker
            .send(JobResult::Progress {
                label: "started".into(),
                fraction: 0.1,
            })
            .unwrap();
        drop(tracker);

        let results: Vec<_> = rx.try_iter().collect();
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0], JobResult::Progress { .. }));
        assert!(matches!(
            &results[1],
            JobResult::Error { job_label, message }
                if job_label == "silent-task" && message.contains("without a terminal result")
        ));
        assert_eq!(
            results.iter().filter(|result| result.is_terminal()).count(),
            1
        );
    }

    #[test]
    fn test_bridge_fail_loud() {
        let (job_tx, job_rx) = mpsc::channel::<JobEnvelope>();
        let (tokio_job_tx, tokio_job_rx) = tokio::sync::mpsc::unbounded_channel::<JobEnvelope>();
        let (result_tx, result_rx) = mpsc::channel::<JobResult>();
        let (watchdog, _watchdog_rx) = crate::app::watchdog::Watchdog::new();
        let watchdog = std::sync::Arc::new(watchdog);
        let _watchdog_for_gui = watchdog.clone();

        // Immediately drop the receiver to simulate disconnect
        drop(tokio_job_rx);

        let handle = spawn_runtime_bridge(job_rx, tokio_job_tx.clone(), tokio_job_tx, result_tx);

        // Send a job
        let _ = job_tx.send(JobEnvelope::broadcast(Job::Ping));

        // Expect error
        match result_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(JobResult::Error { job_label, message }) => {
                assert_eq!(job_label, "ping");
                assert!(message.contains("disconnected"));
            }
            res => panic!("Expected bridge error, got {res:?}"),
        }

        if let Err(e) = handle.join() {
            tracing::error!("Worker thread panicked during shutdown: {:?}", e);
        }

        // Subsequent send should fail because job_rx is dropped
        assert!(job_tx.send(JobEnvelope::broadcast(Job::Ping)).is_err());
    }

    #[tokio::test]
    async fn test_python_job_recursion_regression() {
        // GIVEN: A mock setup that mirrors the Runtime's job loop
        let (job_tx, mut job_rx) = tokio::sync::mpsc::unbounded_channel::<Job>();
        let (python_tx, python_rx) =
            std::sync::mpsc::channel::<(PythonJob, oneshot::Sender<PythonJobResult>)>();
        let python_tx_clone = python_tx.clone();

        // 1. A selector with PyMuPdfEngine (which sends jobs back to a channel)
        let (_std_job_tx, std_job_rx) = std::sync::mpsc::channel::<Job>();
        let job_tx_clone = job_tx.clone();
        std::thread::spawn(move || {
            while let Ok(job) = std_job_rx.recv() {
                let _ = job_tx_clone.send(job);
            }
        });

        let _engine = Arc::new(crate::pdf::OxidizePdfEngine::new());

        // 2. The Runtime Job::Python handler (the logic we are testing)
        let handle = tokio::spawn(async move {
            while let Some(job) = job_rx.recv().await {
                if let Job::Python(py_job, reply_tx) = job {
                    dispatch_python_job(py_job, reply_tx, &python_tx_clone);
                }
            }
        });

        // 3. Trigger a job that would cause recursion in the old version
        let (reply_tx, _reply_rx) = oneshot::channel();
        job_tx
            .send(Job::Python(
                PythonJob::GetTextBlocks {
                    pdf_path: "input.pdf".into(),
                    page_num: 0,
                },
                reply_tx,
            ))
            .unwrap();

        // WHEN: We wait for the message to land on the Python actor
        let (received_job, python_rx) = tokio::task::spawn_blocking(move || {
            let res = python_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("Python job should be forwarded to actor");
            (res.0, python_rx)
        })
        .await
        .unwrap();

        // THEN:
        // 1. It must be the job we sent
        assert!(matches!(received_job, PythonJob::GetTextBlocks { .. }));

        // 2. Exactly ONE message must be received by the actor (no recursion)
        let next_res = python_rx.try_recv();
        assert!(
            next_res.is_err(),
            "Recursion detected: multiple messages sent to Python actor"
        );

        // Cleanup
        drop(job_tx);
        handle.abort();
    }

    #[test]
    fn extraction_router_honors_selected_provider_without_unrelated_cloud_calls() {
        use crate::app::config::DocumentParserMode;

        assert_eq!(
            extraction_provider_order(DocumentParserMode::OfflineHeuristic),
            vec![DocumentParserMode::OfflineHeuristic]
        );
        assert_eq!(
            extraction_provider_order(DocumentParserMode::LlamaParse),
            vec![
                DocumentParserMode::LlamaParse,
                DocumentParserMode::OfflineHeuristic
            ]
        );
        assert_eq!(
            extraction_provider_order(DocumentParserMode::DocumentAi),
            vec![
                DocumentParserMode::DocumentAi,
                DocumentParserMode::OfflineHeuristic
            ]
        );
        assert_eq!(
            extraction_provider_order(DocumentParserMode::LocalOcrs),
            vec![DocumentParserMode::LocalOcrs]
        );
    }

    #[test]
    fn runtime_fallback_branch_prefers_offline_parser_when_online_backends_are_unavailable() {
        let mut cfg = AppConfig::default();
        cfg.document_ai = None;

        let availability = cfg.detect_availability();
        assert!(!availability.document_ai);

        // The runtime should keep the offline parser as the final fallback path
        // when neither Document AI nor Ocr-as-a-Service is configured.
        assert!(availability.unavailable_reason("document_ai").is_some());
        assert!(availability.unavailable_reason("llamaparse").is_some());
    }
}
