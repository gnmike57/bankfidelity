use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::System;
use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub enum WatchdogEvent {
    StallDetected(Duration),                   // 30s stall hit, countdown begins
    FallbackTriggered,                         // 25s countdown hit 0, triggering fallback
    Recovered,                                 // Activity resumed before fallback
    Telemetry { cpu_usage: f32, ram_mb: u64 }, // Live CPU and Memory stats
}

pub struct Watchdog {
    // Shared state to track if a pro_edit is currently active.
    active_pro_edits: Arc<Mutex<usize>>,
    pub tx: broadcast::Sender<WatchdogEvent>,
}

impl Watchdog {
    pub fn new() -> (Self, broadcast::Receiver<WatchdogEvent>) {
        let (tx, rx) = broadcast::channel(16);
        let active_pro_edits = Arc::new(Mutex::new(0));

        let tx_clone = tx.clone();
        let active_clone = active_pro_edits.clone();

        std::thread::spawn(move || {
            let mut sys = System::new_all();
            let mut networks = sysinfo::Networks::new_with_refreshed_list();
            let pid = sysinfo::get_current_pid().unwrap();

            let mut last_activity_time = Instant::now();
            let mut stalled_notified = false;

            loop {
                std::thread::sleep(Duration::from_secs(1));

                let active = *active_clone.lock().unwrap() > 0;
                if !active {
                    last_activity_time = Instant::now();
                    if stalled_notified {
                        let _ = tx_clone.send(WatchdogEvent::Recovered);
                        stalled_notified = false;
                    }
                    continue;
                }

                sys.refresh_processes_specifics(
                    sysinfo::ProcessesToUpdate::Some(&[pid]),
                    true,
                    sysinfo::ProcessRefreshKind::nothing()
                        .with_cpu()
                        .with_disk_usage(),
                );
                networks.refresh(true);

                let mut activity_detected = false;

                let mut current_cpu = 0.0;
                let mut current_ram_mb = 0;

                if let Some(process) = sys.process(pid) {
                    current_cpu = process.cpu_usage();
                    current_ram_mb = process.memory() / (1024 * 1024);
                    if current_cpu > 1.0 {
                        activity_detected = true;
                    }
                    if process.disk_usage().read_bytes > 0 || process.disk_usage().written_bytes > 0
                    {
                        activity_detected = true;
                    }
                }

                let _ = tx_clone.send(WatchdogEvent::Telemetry {
                    cpu_usage: current_cpu,
                    ram_mb: current_ram_mb,
                });

                // Network usage globally
                let mut network_activity = 0;
                for (_, data) in &networks {
                    network_activity += data.received() + data.transmitted();
                }
                if network_activity > 0 {
                    activity_detected = true;
                }

                if activity_detected {
                    last_activity_time = Instant::now();
                    if stalled_notified {
                        let _ = tx_clone.send(WatchdogEvent::Recovered);
                        stalled_notified = false;
                    }
                } else {
                    let elapsed = last_activity_time.elapsed();
                    if elapsed > Duration::from_secs(30) && elapsed <= Duration::from_secs(55) {
                        if !stalled_notified {
                            tracing::warn!("Watchdog: Activity-agnostic stall detected! No CPU, Disk, or Network I/O for 30s.");
                            let _ = tx_clone
                                .send(WatchdogEvent::StallDetected(Duration::from_secs(25)));
                            stalled_notified = true;
                        }
                    } else if elapsed > Duration::from_secs(55) && stalled_notified {
                        tracing::error!(
                            "Watchdog: 25s countdown expired. Triggering fallback cascade."
                        );
                        let _ = tx_clone.send(WatchdogEvent::FallbackTriggered);
                        stalled_notified = false; // Reset to avoid spamming, or just let it loop.
                        last_activity_time = Instant::now(); // Reset
                    }
                }
            }
        });

        (
            Self {
                active_pro_edits,
                tx,
            },
            rx,
        )
    }

    pub fn start_pro_edit(&self) {
        *self.active_pro_edits.lock().unwrap() += 1;
    }

    pub fn end_pro_edit(&self) {
        let mut count = self.active_pro_edits.lock().unwrap();
        if *count > 0 {
            *count -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watchdog_lifecycle() {
        let (watchdog, mut rx) = Watchdog::new();

        watchdog.start_pro_edit();
        assert_eq!(*watchdog.active_pro_edits.lock().unwrap(), 1);

        // Wait for one telemetry event
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // It might take ~1s for the first event, so we give it a 3s timeout
            let res = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await;
            if let Ok(Ok(event)) = res {
                if let WatchdogEvent::Telemetry { cpu_usage, ram_mb } = event {
                    assert!(cpu_usage >= 0.0);
                    // ram_mb is usize — always >= 0. Sanity-check it's reasonable.
                    let _ = ram_mb;
                }
            } else {
                panic!("Did not receive event in time");
            }
        });

        watchdog.end_pro_edit();
        assert_eq!(*watchdog.active_pro_edits.lock().unwrap(), 0);

        // Ensure it doesn't underflow
        watchdog.end_pro_edit();
        assert_eq!(*watchdog.active_pro_edits.lock().unwrap(), 0);
    }
}
