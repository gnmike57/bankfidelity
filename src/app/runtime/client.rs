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
use crate::app::runtime::jobs::*;
use crate::app::runtime::python_job::*;
use crate::app::runtime::tracking::*;

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
    pub(crate) intake: Arc<Mutex<Option<mpsc::Sender<JobEnvelope>>>>,
}

impl RuntimeClient {
    pub(crate) fn new(intake: mpsc::Sender<JobEnvelope>) -> Self {
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

