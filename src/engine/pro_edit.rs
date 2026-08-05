use std::future::Future;

pub async fn perform_pro_edit<T, Fut>(
    _task_name: &str, // Telemetry ready
    task: Fut,
    watchdog: std::sync::Arc<crate::app::watchdog::Watchdog>,
) -> Result<T, anyhow::Error>
where
    Fut: Future<Output = Result<T, anyhow::Error>>,
{
    watchdog.start_pro_edit();
    let res = task.await;
    watchdog.end_pro_edit();
    res
}
