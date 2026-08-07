use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub struct UfoClient;

static UFO_ACTIVE_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Default install root for Microsoft UFO on Windows (user-configured path).
pub const DEFAULT_UFO_DIR: &str = "C:\\UFO";

impl UfoClient {
    /// Returns true when a UFO child process PID is currently tracked.
    pub fn is_running() -> bool {
        UFO_ACTIVE_PID.load(std::sync::atomic::Ordering::SeqCst) != 0
    }

    /// Cancels the currently running UFO task (if any) by terminating the process tree.
    pub fn cancel_task() {
        let pid = UFO_ACTIVE_PID.swap(0, std::sync::atomic::Ordering::SeqCst);
        if pid == 0 {
            return;
        }
        tracing::warn!("Cancelling UFO Task with PID: {}", pid);
        kill_process_tree(pid);
    }

    /// Resolves the UFO install directory (override via `BANKFIDELITY_UFO_DIR`).
    pub fn ufo_dir() -> std::path::PathBuf {
        std::env::var_os("BANKFIDELITY_UFO_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_UFO_DIR))
    }

    /// Dispatches a UI automation task to Microsoft UFO.
    /// Injects BankFidelity state context to maximize common understanding.
    ///
    /// Returns `Err` when UFO is missing, the process fails to start, or the
    /// UFO process exits non-zero. Concurrent dispatches cancel the previous task.
    pub fn dispatch_task<F>(request: &str, mut on_log: Option<F>) -> Result<Value, String>
    where
        F: FnMut(String) + Send + 'static,
    {
        let ufo_dir = Self::ufo_dir();

        if !ufo_dir.exists() {
            return Err(format!(
                "UFO framework not found at {:?}. Install Microsoft UFO or set BANKFIDELITY_UFO_DIR.",
                ufo_dir
            ));
        }

        // Avoid orphaning a previous UFO process when a new task starts.
        if Self::is_running() {
            tracing::warn!("UFO task already active; cancelling previous process before dispatch");
            Self::cancel_task();
        }

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let task_id = format!("bankfidelity_{timestamp}");

        let context_prompt = format!(
            "{request}\n\n[BANKFIDELITY CONTEXT]\nActive Directory: {:?}\nApp State: BankFidelity Local LLM Orchestrator Pipeline Running",
            std::env::current_dir().unwrap_or_default()
        );

        let python_exe = ufo_dir.join("python_env").join("python.exe");
        let python_cmd = if python_exe.exists() {
            python_exe.to_string_lossy().to_string()
        } else {
            "python".to_string()
        };

        let mut child = Command::new(&python_cmd)
            .arg("-m")
            .arg("ufo")
            .arg("--task")
            .arg(&task_id)
            .arg("--request")
            .arg(&context_prompt)
            .current_dir(&ufo_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!("Failed to execute UFO python process ({python_cmd}): {e}")
            })?;

        UFO_ACTIVE_PID.store(child.id(), std::sync::atomic::Ordering::SeqCst);

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to open UFO stdout pipe".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Failed to open UFO stderr pipe".to_string())?;

        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let stderr_buf_thread = Arc::clone(&stderr_buf);
        let stderr_thread = std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                tracing::warn!("[UFO-STDERR] {line}");
                if let Ok(mut buf) = stderr_buf_thread.lock() {
                    buf.push_str(&line);
                    buf.push('\n');
                }
            }
        });

        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            tracing::info!("[UFO] {line}");
            if let Some(cb) = on_log.as_mut() {
                cb(line);
            }
        }

        let status = child
            .wait()
            .map_err(|e| format!("Failed to wait on UFO process: {e}"))?;
        UFO_ACTIVE_PID.store(0, std::sync::atomic::Ordering::SeqCst);
        let _ = stderr_thread.join();

        let stderr_str = stderr_buf
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();

        let log_path = ufo_dir.join("logs").join(&task_id).join("output.md");
        let ufo_result = if log_path.exists() {
            std::fs::read_to_string(&log_path)
                .unwrap_or_else(|e| format!("Failed to read output.md: {e}"))
        } else {
            "UFO did not generate an output.md file.".into()
        };

        if !status.success() {
            return Err(format!(
                "UFO task failed (exit={status}).\nStderr:\n{stderr_str}\n\nLog Output:\n{ufo_result}"
            ));
        }

        Ok(json!({
            "status": "success",
            "output": ufo_result,
            "task_id": task_id
        }))
    }
}

fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .arg("/F")
            .arg("/T")
            .arg("/PID")
            .arg(pid.to_string())
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_ufo_dir_returns_error_not_ok_json() {
        // Force a non-existent install path via env override.
        std::env::set_var("BANKFIDELITY_UFO_DIR", "__bankfidelity_missing_ufo_dir__");
        let result = UfoClient::dispatch_task("noop", None::<fn(String)>);
        std::env::remove_var("BANKFIDELITY_UFO_DIR");
        let err = result.expect_err("missing UFO must be Err");
        assert!(
            err.contains("UFO framework not found"),
            "unexpected error: {err}"
        );
        assert!(!UfoClient::is_running());
    }

    #[test]
    fn cancel_with_no_active_task_is_noop() {
        UFO_ACTIVE_PID.store(0, std::sync::atomic::Ordering::SeqCst);
        UfoClient::cancel_task();
        assert!(!UfoClient::is_running());
    }

    #[test]
    fn ufo_dir_env_override_is_respected() {
        std::env::set_var("BANKFIDELITY_UFO_DIR", "D:\\custom\\ufo");
        let dir = UfoClient::ufo_dir();
        std::env::remove_var("BANKFIDELITY_UFO_DIR");
        assert_eq!(dir, std::path::PathBuf::from("D:\\custom\\ufo"));
    }
}
