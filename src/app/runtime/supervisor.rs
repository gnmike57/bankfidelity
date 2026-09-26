use super::cancellation::CancellationRegistry;
use super::client::RuntimeClient;
use super::ids::JobEnvelope;
use super::jobs::Job;
use super::python_job::{PythonJob, PythonJobResult};
use super::results::JobResult;
use super::tracking::{spawn_job_lifecycle_monitor, ResultSink};
use crate::app::audit::AuditLog;
use crate::engine::history::ChangeHistory;
use crate::engine::segments::{SegmentManager, SegmentMap};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tokio::sync::oneshot;
use tracing::Instrument;

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
                super::process_job_inner(
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
                super::process_job_inner(
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

pub(crate) fn spawn_runtime_bridge(
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
