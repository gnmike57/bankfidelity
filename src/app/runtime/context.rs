use crate::app::audit::AuditLog;
use crate::app::config::ConfigManager;
use crate::app::runtime::cancellation::CancellationRegistry;
use crate::app::runtime::parser_chain::InteractiveFallbackRouter;
use crate::app::runtime::python_job::{PythonJob, PythonJobResult};
use crate::app::runtime::tracking::ResultSink;
use crate::engine::history::ChangeHistory;
use crate::engine::segments::{SegmentManager, SegmentMap};
use crate::pdf::engine::PdfEngine;
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::oneshot;

pub(crate) struct JobContext<'a> {
    pub(crate) python_tx_clone: mpsc::Sender<(PythonJob, oneshot::Sender<PythonJobResult>)>,
    pub(crate) result_tx_clone: ResultSink,
    pub(crate) engine_for_tokio: Arc<dyn PdfEngine>,
    pub(crate) config_for_tokio: Arc<crate::app::config::AppConfig>,
    pub(crate) wdog: Arc<crate::app::watchdog::Watchdog>,
    pub(crate) history: Arc<Mutex<ChangeHistory>>,
    pub(crate) audit_log: Arc<Mutex<AuditLog>>,
    pub(crate) cancellations_for_loop: CancellationRegistry,
    pub(crate) api_semaphore: Arc<tokio::sync::Semaphore>,
    pub(crate) segment_map: &'a mut Option<SegmentMap>,
    pub(crate) segment_manager: &'a mut Option<SegmentManager>,
    pub(crate) fallback_router: InteractiveFallbackRouter,
    pub(crate) parse_cache:
        Arc<tokio::sync::Mutex<lru::LruCache<String, crate::ai::document_ai::BankStatement>>>,
    pub(crate) config_holder: ConfigManager,
}
