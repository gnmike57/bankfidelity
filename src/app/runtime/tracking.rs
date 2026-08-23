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
use crate::app::runtime::python_job::*;

#[derive(Clone, Default)]
pub struct CancellationRegistry {
    pub(crate) inner: Arc<Mutex<HashMap<JobId, CancellationToken>>>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new token under `id`. Returns the token (so the caller
    /// can pass it into the spawned task).
    pub fn register(&self, id: JobId) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut g) = self.inner.lock() {
            g.insert(id, token.clone());
        }
        token
    }

    /// Cancel and remove the token for `id`. No-op if unknown.
    pub fn cancel(&self, id: JobId) -> bool {
        let token = self.inner.lock().ok().and_then(|mut g| g.remove(&id));
        if let Some(t) = token {
            t.cancel();
            true
        } else {
            false
        }
    }

    /// Drop the token for `id` (job has finished naturally).
    pub fn complete(&self, id: JobId) {
        if let Ok(mut g) = self.inner.lock() {
            g.remove(&id);
        }
    }

    /// Request cancellation for every in-flight job while retaining registry
    /// entries until their exactly-once terminal result confirms completion.
    pub fn request_cancel_all(&self) {
        if let Ok(g) = self.inner.lock() {
            for token in g.values() {
                token.cancel();
            }
        }
    }

    /// Force-clear every job token after a bounded graceful wait has expired.
    pub fn cancel_all(&self) {
        if let Ok(mut g) = self.inner.lock() {
            for (_, token) in g.drain() {
                token.cancel();
            }
        }
    }

    /// How many jobs are currently registered.
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone)]
pub(crate) struct ResultSink {
    pub(crate) broadcast: mpsc::Sender<JobResult>,
    pub(crate) metadata: JobMetadata,
    pub(crate) route: Option<mpsc::Sender<JobResult>>,
    pub(crate) cancellations: CancellationRegistry,
    pub(crate) terminal_sent: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) completion: std::sync::Arc<tokio::sync::Notify>,
}

impl ResultSink {
    pub(crate) fn new(
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
    pub(crate) fn send(&self, result: JobResult) -> Result<(), mpsc::SendError<JobResult>> {
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

    pub(crate) fn is_interactive(&self) -> bool {
        self.metadata.execution_mode == ExecutionMode::Interactive
    }

    pub(crate) async fn completed(&self) {
        if self
            .terminal_sent
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        self.completion.notified().await;
    }
}

#[derive(Clone)]
pub struct TerminalTracker(std::sync::Arc<TerminalTrackerInner>);

struct TerminalTrackerInner {
    pub(crate) tx: ResultSink,
    pub(crate) label: String,
    pub(crate) terminal_sent: std::sync::atomic::AtomicBool,
}

impl TerminalTracker {
    pub(crate) fn new(tx: ResultSink, label: impl Into<String>) -> Self {
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

    pub(crate) fn is_interactive(&self) -> bool {
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

