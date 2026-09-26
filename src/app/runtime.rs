// Python operations run only through the supervised versioned worker process.
use crate::engine::segments::{SegmentManager, SegmentMap};
use std::sync::mpsc;
use tokio::sync::oneshot;

mod cancellation;
mod client;
mod context;
mod handler_docai;
mod handler_document;
mod handler_editing;
mod handler_extraction;
mod handler_system;
mod handler_transfer;
mod handler_verification;
mod handler_workflow;
mod ids;
mod jobs;
mod parser_chain;
mod python_job;
mod results;
mod supervisor;
mod tracking;

pub use cancellation::CancellationRegistry;
pub use client::{JobTicket, RuntimeClient, RuntimeSubmitError};
#[cfg(test)]
pub(crate) use ids::JobEnvelope;
pub use ids::{alloc_job_id, ExecutionMode, JobId, JobMetadata};
pub use jobs::Job;
pub use python_job::{PythonJob, PythonJobResult};
pub use results::{JobResult, OperationDisposition};
#[cfg(test)]
pub(crate) use supervisor::spawn_runtime_bridge;
pub use supervisor::Runtime;
#[cfg(test)]
pub(crate) use tracking::spawn_job_lifecycle_monitor;
pub(crate) use tracking::ResultSink;
pub use tracking::TerminalTracker;

use self::parser_chain::InteractiveFallbackRouter;

/// Dispatches a Python job to the actor thread.
/// This function MUST forward directly to avoid recursion through the engine selector.
pub(crate) fn dispatch_python_job(
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
pub(crate) async fn parse_with_offline_fallback(
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
#[allow(clippy::too_many_arguments)]
pub(crate) async fn process_job_inner(
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
    let mut ctx = context::JobContext {
        python_tx_clone,
        result_tx_clone,
        engine_for_tokio,
        config_for_tokio,
        wdog,
        history,
        audit_log,
        cancellations_for_loop,
        api_semaphore,
        segment_map,
        segment_manager,
        fallback_router,
        parse_cache,
        config_holder,
    };

    match job {
        job @ (Job::LoadDocument { .. }
        | Job::AnalyzeFonts { .. }
        | Job::RenderPage { .. }
        | Job::CompleteFont { .. }
        | Job::ExportChangeHistory { .. }
        | Job::McpRenderPage { .. }
        | Job::LoadHistory { .. }) => {
            handler_document::handle(job, &mut ctx).await;
        }

        job @ (Job::AiFixVisualFidelity { .. }
        | Job::AdjustDatePeriods { .. }
        | Job::ApplyChange { .. }
        | Job::Undo
        | Job::Redo
        | Job::ApplyProposedChanges { .. }
        | Job::GenerateVisualAlternatives { .. }
        | Job::TypstReconstruct { .. }) => {
            handler_editing::handle(job, &mut ctx).await;
        }

        job @ (Job::ExplainImbalance { .. }
        | Job::NaturalLanguageEdit { .. }
        | Job::CategorizeTransactions { .. }
        | Job::ExtractTransactions { .. }
        | Job::BalanceStatement { .. }
        | Job::BalanceAndApplyAll { .. }) => {
            handler_extraction::handle(job, &mut ctx).await;
        }

        job @ (Job::AiConfirmationResponse(_)
        | Job::InteractiveFallbackResponse(_)
        | Job::WorkflowParseAndValidate { .. }
        | Job::WorkflowPreview { .. }
        | Job::WorkflowConfirmAndRender { .. }) => {
            handler_workflow::handle(job, &mut ctx).await;
        }

        job @ Job::Verify { .. } => {
            handler_verification::handle(job, &mut ctx).await;
        }

        job @ (Job::TransferTransactions { .. } | Job::RunTransferTests { .. }) => {
            handler_transfer::handle(job, &mut ctx).await;
        }

        job @ (Job::ListDocAiVersions
        | Job::DeployDocAiVersion { .. }
        | Job::UndeployDocAiVersion { .. }
        | Job::SetDefaultDocAiVersion { .. }
        | Job::TrainDocAiVersion { .. }) => {
            handler_docai::handle(job, &mut ctx).await;
        }

        job @ (Job::Ping
        | Job::CancelUfo
        | Job::UfoAutoEdit { .. }
        | Job::SubmitBugReport { .. }
        | Job::Python(..)
        | Job::AiCommand { .. }
        | Job::CleanupTempFiles
        | Job::Cancel { .. }
        | Job::ReloadConfig
        | Job::ValidateCredentials) => {
            handler_system::handle(job, &mut ctx).await;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::parser_chain::{extraction_provider_order, wait_for_interactive_choice};
    use super::*;
    use crate::app::config::AppConfig;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

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
