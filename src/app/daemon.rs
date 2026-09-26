//! 24/7 Native Rust Guardian & Watchdog Daemon.
//!
//! Orchestrates:
//! 1. Continuous polling of Windows desktop interactive context.
//! 2. Hung application detection (`IsHungAppWindow`) & automatic focus recovery.
//! 3. Port conflict monitoring & zombie process cleanup.
//! 4. Continuous learning: periodic bank statement template studying into SQLite.

use crate::app::watchdog_win32::Win32Watchdog;
use crate::engine::template_study::TemplateStudyEngine;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info, warn};

pub struct DaemonConfig {
    pub watchdog_enabled: bool,
    pub template_study_enabled: bool,
    pub interval_secs: u64,
    pub templates_db_path: PathBuf,
    pub incoming_dir: PathBuf,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            watchdog_enabled: true,
            template_study_enabled: true,
            interval_secs: 5,
            templates_db_path: PathBuf::from("C:/bankfidelity/data/templates.db"),
            incoming_dir: PathBuf::from("C:/bankfidelity/statements/incoming"),
        }
    }
}

pub struct GuardianDaemon {
    cfg: DaemonConfig,
    template_engine: Option<TemplateStudyEngine>,
}

impl GuardianDaemon {
    pub fn new(cfg: DaemonConfig) -> Self {
        let template_engine = if cfg.template_study_enabled {
            match TemplateStudyEngine::open_or_create(&cfg.templates_db_path) {
                Ok(engine) => {
                    info!(
                        "[DAEMON] Template studying database active at {:?}",
                        cfg.templates_db_path
                    );
                    Some(engine)
                }
                Err(e) => {
                    error!("[DAEMON] Failed to open template DB: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            cfg,
            template_engine,
        }
    }

    /// Runs the 24/7 guardian loop until Ctrl+C is received.
    pub async fn run_forever(mut self) {
        info!("=================================================================");
        info!(" BankFidelity 24/7 Guardian & Self-Healing Daemon Active         ");
        info!(
            " Interval: {}s | Watchdog: {} | Template Studying: {}",
            self.cfg.interval_secs, self.cfg.watchdog_enabled, self.cfg.template_study_enabled
        );
        info!("=================================================================");

        let mut interval = tokio::time::interval(Duration::from_secs(self.cfg.interval_secs));
        let mut loop_count: u64 = 0;

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("[DAEMON] Received termination signal. Gracefully shutting down...");
                    break;
                }
                _ = interval.tick() => {
                    loop_count += 1;
                    self.perform_heartbeat_cycle(loop_count).await;
                }
            }
        }

        info!("[DAEMON] 24/7 Guardian Daemon exited cleanly.");
    }

    async fn perform_heartbeat_cycle(&mut self, cycle: u64) {
        // 1. Win32 Desktop Health Check
        if self.cfg.watchdog_enabled {
            let health = Win32Watchdog::inspect_desktop();
            if health.foreground_is_hung {
                warn!(
                    "[WATCHDOG] Foreground window (HWND: {}) is HUNG! Attempting recovery...",
                    health.foreground_hwnd
                );
                Win32Watchdog::unfreeze_foreground(health.foreground_hwnd);
            }

            // Check ports every 12 cycles (~60 seconds)
            if cycle % 12 == 0 {
                for (port, free) in &health.ports_available {
                    if !free {
                        info!(
                            "[WATCHDOG] Port {} is actively bound by an active service.",
                            port
                        );
                    }
                }
            }
        }

        // 2. Periodic Template Study Crawl every 60 cycles (~5 minutes)
        if self.cfg.template_study_enabled && (cycle % 60 == 0 || cycle == 1) {
            if let Some(engine) = &mut self.template_engine {
                if self.cfg.incoming_dir.exists() {
                    let indexed = engine.study_incoming_directory(&self.cfg.incoming_dir);
                    if indexed > 0 {
                        info!(
                            "[DAEMON] Template Study: Indexed {} new statement archetypes into SQLite.",
                            indexed
                        );
                    }
                }
            }
        }
    }
}
