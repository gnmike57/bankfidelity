//! Preflight system to verify environment constraints before launching GUI.

use crate::app::config::AppConfig;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn verify_environment_rejects_headless_fallback_override() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("FORCE_HEADLESS_FALLBACK", "1");

        let err = verify_environment(&AppConfig::default()).unwrap_err();
        assert!(matches!(err, PreflightError::HeadlessEnvironment));

        std::env::remove_var("FORCE_HEADLESS_FALLBACK");
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PreflightError {
    #[error(
        "Insufficient available memory. Required: {required_mb} MB, Available: {available_mb} MB"
    )]
    InsufficientMemory { required_mb: u64, available_mb: u64 },
    #[error("Display server is unavailable (Headless environment)")]
    HeadlessEnvironment,
}

pub fn verify_environment(_config: &AppConfig) -> Result<(), PreflightError> {
    // 1. Verify Memory using sysinfo
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    let available_mb = sys.available_memory() / 1024 / 1024;
    let required_mb = 512; // Minimum MB to safely launch GUI + Python
    if available_mb < required_mb {
        return Err(PreflightError::InsufficientMemory {
            required_mb,
            available_mb,
        });
    }

    // 2. Verify Display Server availability
    #[cfg(target_os = "linux")]
    {
        if std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() {
            return Err(PreflightError::HeadlessEnvironment);
        }
    }

    // Auto-Heal test injection support: If testing fallback, we can set an env var
    if std::env::var("FORCE_HEADLESS_FALLBACK").is_ok() {
        return Err(PreflightError::HeadlessEnvironment);
    }

    Ok(())
}
