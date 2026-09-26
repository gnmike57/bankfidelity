use super::cancellation::CancellationRegistry;
use super::ids::{ExecutionMode, JobMetadata};
use super::results::JobResult;
use std::sync::mpsc;

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

pub(crate) fn spawn_job_lifecycle_monitor(
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
pub struct TerminalTracker(pub(crate) std::sync::Arc<TerminalTrackerInner>);

pub(crate) struct TerminalTrackerInner {
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

pub(crate) fn block_on_from_blocking_context<F, T>(fut: F) -> T
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::app::runtime::{Job, JobMetadata};

    fn test_sink(broadcast: mpsc::Sender<JobResult>) -> ResultSink {
        ResultSink::new(
            broadcast,
            JobMetadata::for_job(&Job::Ping),
            None,
            CancellationRegistry::new(),
        )
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
}
