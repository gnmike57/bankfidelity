use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct UfoTaskResult {
    pub status: String,
    pub task_id: String,
    pub output: Option<String>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub traceback: Option<String>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum UfoError {
    #[error("Hallucination: {0}")]
    Hallucination(String),
    #[error("Crash: {0}")]
    Crash(String),
    #[error("Dependency: {0}")]
    Dependency(String),
    #[error("Unknown: {0}")]
    Unknown(String),
}

impl From<String> for UfoError {
    fn from(err: String) -> Self {
        UfoError::Unknown(err)
    }
}

impl UfoTaskResult {
    /// Programmatically intercepts generated PDF artifacts from the UFO output log / result string
    /// using strict regex: `(?i)[a-z]:\\[^<>\x22\|\?\*]+\.pdf`
    pub fn extract_pdf_artifacts(&self) -> Vec<std::path::PathBuf> {
        let mut paths = Vec::new();
        let text = match (&self.output, &self.error_message) {
            (Some(out), Some(err)) => format!("{}\n{}", out, err),
            (Some(out), None) => out.clone(),
            (None, Some(err)) => err.clone(),
            (None, None) => return paths,
        };

        // Strict path extraction: Windows drive-letter paths and POSIX absolute paths.
        let windows_re = regex::Regex::new(r"(?i)[a-zA-Z]:\\[^<>\x22\|\?\*\n\r]+\.pdf");
        let posix_re = regex::Regex::new(r"/[A-Za-z0-9._~/-]+\.pdf");
        for re in [windows_re, posix_re].into_iter().flatten() {
            for cap in re.find_iter(&text) {
                let p = std::path::PathBuf::from(cap.as_str().trim());
                if p.exists() && !paths.contains(&p) {
                    paths.push(p);
                }
            }
        }
        paths
    }
}

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
    pub fn dispatch_task<F>(request: &str, mut on_log: Option<F>) -> Result<UfoTaskResult, UfoError>
    where
        F: FnMut(String) + Send + 'static,
    {
        let mut attempts = 0;
        let max_attempts = 2;
        loop {
            attempts += 1;
            let res = Self::execute_single_attempt(request, &mut on_log);
            match res {
                Ok(r) => return Ok(r),
                Err(e) => {
                    if attempts >= max_attempts {
                        return Err(e);
                    }
                    if let UfoError::Hallucination(msg) = &e {
                        tracing::warn!(
                            "UFO Hallucinated (attempt {}/{}): {}. Retrying...",
                            attempts,
                            max_attempts,
                            msg
                        );
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    /// Resolves the Python interpreter used to launch UFO.
    /// Priority: PYO3_PYTHON > PYTHON_EXE > BANKFIDELITY_UFO_PYTHON >
    /// UFO's bundled `<ufo_dir>/ufo/python_env/python.exe` > `python` on PATH.
    /// No machine-specific absolute paths are hardcoded; configure via env vars.
    fn resolve_python_command(ufo_dir: &std::path::Path) -> String {
        for var in ["PYO3_PYTHON", "PYTHON_EXE", "BANKFIDELITY_UFO_PYTHON"] {
            if let Ok(val) = std::env::var(var) {
                if !val.trim().is_empty() {
                    return val;
                }
            }
        }
        let candidate1 = ufo_dir.join("ufo").join("python_env").join("python.exe");
        if candidate1.exists() {
            return candidate1.to_string_lossy().to_string();
        }
        let candidate2 = ufo_dir.join("python_env").join("python.exe");
        if candidate2.exists() {
            return candidate2.to_string_lossy().to_string();
        }
        "python".to_string()
    }

    fn execute_single_attempt<F>(
        request: &str,
        on_log: &mut Option<F>,
    ) -> Result<UfoTaskResult, UfoError>
    where
        F: FnMut(String) + Send + 'static,
    {
        let ufo_dir = Self::ufo_dir();

        if !ufo_dir.exists() {
            return Err(UfoError::Unknown(format!(
                "UFO framework not found at {:?}. Install Microsoft UFO or set BANKFIDELITY_UFO_DIR.",
                ufo_dir
            )));
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

        let python_cmd = Self::resolve_python_command(&ufo_dir);
        let work_dir = if ufo_dir.join("ufo").join("__main__.py").exists() {
            ufo_dir.clone()
        } else if ufo_dir.join("__main__.py").exists() {
            ufo_dir.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| ufo_dir.clone())
        } else {
            ufo_dir.clone()
        };

        let mut child = Command::new(&python_cmd)
            .arg("-m")
            .arg("ufo")
            .arg("--task")
            .arg(&task_id)
            .arg("--request")
            .arg(&context_prompt)
            .current_dir(&work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to execute UFO python process ({python_cmd}): {e}"))?;

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

        // Pump stdout on a dedicated thread so a hung UFO process that keeps
        // its stdout open can never starve the watchdog poll below.
        let mut on_log = on_log.take();
        let stdout_thread = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                tracing::info!("[UFO] {line}");
                if let Some(cb) = on_log.as_mut() {
                    cb(line);
                }
            }
        });

        let start_wait = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(600);
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => {
                    if start_wait.elapsed() > timeout {
                        tracing::error!(
                            "UFO task timed out after 10 minutes. Killing process tree."
                        );
                        kill_process_tree(child.id());
                        return Err(UfoError::Crash(
                            "UFO task timed out (indefinite hang detected)".into(),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                Err(e) => return Err(UfoError::Crash(format!("Failed to wait on UFO: {e}"))),
            }
        };
        UFO_ACTIVE_PID.store(0, std::sync::atomic::Ordering::SeqCst);
        // Exiting/killing the child closes its pipes, releasing the pump threads.
        let _ = stdout_thread.join();
        let _ = stderr_thread.join();

        let stderr_str = stderr_buf.lock().map(|s| s.clone()).unwrap_or_default();

        let candidate_log_dirs = [
            ufo_dir.join("logs").join(&task_id),
            ufo_dir.join("ufo").join("logs").join(&task_id),
            work_dir.join("logs").join(&task_id),
        ];

        let mut found_result_json = None;
        let mut found_output_md = None;
        for dir in &candidate_log_dirs {
            let r = dir.join("result.json");
            if r.exists() && found_result_json.is_none() {
                found_result_json = Some(r);
            }
            let o = dir.join("output.md");
            if o.exists() && found_output_md.is_none() {
                found_output_md = Some(o);
            }
        }

        if let Some(result_json_path) = found_result_json {
            let json_str = std::fs::read_to_string(&result_json_path).unwrap_or_default();
            match serde_json::from_str::<UfoTaskResult>(&json_str) {
                Ok(res) => {
                    if res.status == "error" {
                        let err_msg = res.error_message.clone().unwrap_or_default();
                        let err_type = res.error_type.as_deref().unwrap_or("");
                        match err_type {
                            "ValueError" | "AssertionError" => {
                                return Err(UfoError::Hallucination(err_msg))
                            }
                            "ImportError" | "ModuleNotFoundError" => {
                                return Err(UfoError::Dependency(err_msg))
                            }
                            _ => return Err(UfoError::Crash(err_msg)),
                        }
                    }
                    return Ok(res);
                }
                Err(e) => {
                    tracing::warn!("result.json was corrupted. Synthesizing crash report.");
                    return Err(UfoError::Crash(format!(
                        "UFO payload corrupted ({e}). Stderr snapshot: {}",
                        stderr_str
                    )));
                }
            }
        }

        // Fallback if result.json wasn't written
        let ufo_result = if let Some(log_path) = found_output_md {
            std::fs::read_to_string(&log_path).unwrap_or_default()
        } else {
            "".into()
        };

        if !status.success() {
            return Err(UfoError::Crash(format!(
                "Exit {status}. Stderr: {stderr_str}"
            )));
        }

        Ok(UfoTaskResult {
            status: "success".into(),
            task_id,
            output: Some(ufo_result),
            error_type: None,
            error_message: None,
            traceback: None,
        })
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn missing_ufo_dir_returns_error_not_ok_json() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Force a non-existent install path via env override.
        std::env::set_var("BANKFIDELITY_UFO_DIR", "__bankfidelity_missing_ufo_dir__");
        let result = UfoClient::dispatch_task("noop", None::<fn(String)>);
        std::env::remove_var("BANKFIDELITY_UFO_DIR");
        let err = result.expect_err("missing UFO must be Err");
        let err = match err {
            UfoError::Unknown(s) => s,
            _ => String::new(),
        };
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
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("BANKFIDELITY_UFO_DIR", "D:\\custom\\ufo");
        let dir = UfoClient::ufo_dir();
        std::env::remove_var("BANKFIDELITY_UFO_DIR");
        assert_eq!(dir, std::path::PathBuf::from("D:\\custom\\ufo"));
    }

    #[test]
    fn python_env_override_is_respected_for_interpreter() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("BANKFIDELITY_UFO_PYTHON", "D:\\custom\\python.exe");
        let cmd = UfoClient::resolve_python_command(std::path::Path::new("C:\\UFO"));
        std::env::remove_var("BANKFIDELITY_UFO_PYTHON");
        assert_eq!(cmd, "D:\\custom\\python.exe");
    }

    #[test]
    fn empty_python_env_override_falls_through() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("BANKFIDELITY_UFO_PYTHON", "  ");
        let cmd = UfoClient::resolve_python_command(std::path::Path::new("__missing__"));
        std::env::remove_var("BANKFIDELITY_UFO_PYTHON");
        assert_eq!(cmd, "python");
    }

    #[test]
    fn extract_pdf_artifacts_finds_existing_files() {
        let pdf = std::env::temp_dir().join("bankfidelity_ufo_artifact_test.pdf");
        std::fs::write(&pdf, b"%PDF-1.4\n").expect("write temp pdf stub");
        let result = UfoTaskResult {
            status: "success".into(),
            task_id: "t".into(),
            output: Some(format!("Saved document to {}", pdf.display())),
            error_type: None,
            error_message: None,
            traceback: None,
        };
        let found = result.extract_pdf_artifacts();
        let _ = std::fs::remove_file(&pdf);
        assert!(
            found.iter().any(|p| p == &pdf),
            "expected {:?} in {:?}",
            pdf,
            found
        );
    }
}

