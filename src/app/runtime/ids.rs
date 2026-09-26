use super::jobs::Job;
use super::results::JobResult;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use uuid::Uuid;

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
    pub(crate) fn for_job(job: &Job) -> Self {
        Self::for_job_with_mode(job, ExecutionMode::Interactive)
    }

    pub(crate) fn for_job_with_mode(job: &Job, execution_mode: ExecutionMode) -> Self {
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

pub(crate) fn document_id_for_path(path: &Path) -> String {
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

pub(crate) struct JobEnvelope {
    pub(crate) metadata: JobMetadata,
    pub(crate) job: Job,
    pub(crate) route: Option<mpsc::Sender<JobResult>>,
}

impl JobEnvelope {
    pub(crate) fn broadcast(job: Job) -> Self {
        Self {
            metadata: JobMetadata::for_job(&job),
            job,
            route: None,
        }
    }

    pub(crate) fn broadcast_with_mode(job: Job, execution_mode: ExecutionMode) -> Self {
        Self {
            metadata: JobMetadata::for_job_with_mode(&job, execution_mode),
            job,
            route: None,
        }
    }

    pub(crate) fn routed_with_mode(
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
