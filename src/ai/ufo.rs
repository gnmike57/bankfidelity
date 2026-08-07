use std::process::Command;
use serde_json::{json, Value};
use std::time::SystemTime;

pub struct UfoClient;

static UFO_ACTIVE_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl UfoClient {
    /// Cancels the currently running UFO task (if any) by forcefully terminating the python process
    pub fn cancel_task() {
        let pid = UFO_ACTIVE_PID.swap(0, std::sync::atomic::Ordering::SeqCst);
        if pid != 0 {
            tracing::warn!("Cancelling UFO Task with PID: {}", pid);
            let _ = Command::new("taskkill")
                .arg("/F")
                .arg("/T")
                .arg("/PID")
                .arg(pid.to_string())
                .output();
        }
    }

    /// Dispatches a UI automation task to Microsoft UFO directly.
    /// Injects BankFidelity state context to maximize common understanding.
    pub fn dispatch_task<F>(request: &str, mut on_log: Option<F>) -> Result<Value, String> 
    where F: FnMut(String) + Send + 'static
    {
        // Microsoft UFO is installed directly on C:\UFO per user configuration
        let ufo_dir = std::path::PathBuf::from("C:\\UFO");
            
        if !ufo_dir.exists() {
            return Ok(json!({
                "status": "error",
                "message": format!("UFO framework not found at {:?}. Please setup UFO.", ufo_dir)
            }));
        }

        // Generate a task ID
        let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
        let task_id = format!("bankfidelity_{}", timestamp);

        // Construct the contextual request to maximize common understanding
        let context_prompt = format!(
            "{}\n\n[BANKFIDELITY CONTEXT]\nActive Directory: {:?}\nApp State: BankFidelity Local LLM Orchestrator Pipeline Running", 
            request, 
            std::env::current_dir().unwrap_or_default()
        );

        let python_exe = ufo_dir.join("python_env").join("python.exe");
        let python_cmd = if python_exe.exists() {
            python_exe.to_string_lossy().to_string()
        } else {
            "python".to_string()
        };

        let mut child = Command::new(python_cmd)
            .arg("-m")
            .arg("ufo")
            .arg("--task")
            .arg(&task_id)
            .arg("--request")
            .arg(&context_prompt)
            .current_dir(&ufo_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to execute UFO python process: {}", e))?;

        UFO_ACTIVE_PID.store(child.id(), std::sync::atomic::Ordering::SeqCst);

        let stdout = child.stdout.take().expect("Failed to open stdout");
        let stderr = child.stderr.take().expect("Failed to open stderr");

        let stderr_thread = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                tracing::warn!("[UFO-STDERR] {}", line);
            }
        });

        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(stdout);
        for line in reader.lines().flatten() {
            tracing::info!("[UFO] {}", line);
            if let Some(cb) = on_log.as_mut() {
                cb(line);
            }
        }

        let status = child.wait().map_err(|e| format!("Failed to wait on UFO process: {}", e))?;
        UFO_ACTIVE_PID.store(0, std::sync::atomic::Ordering::SeqCst);
        let _ = stderr_thread.join();
        
        let stderr_str = if status.success() { String::new() } else { "See tracing logs for stderr".into() };
        
        // Try to read the output.md
        let log_path = ufo_dir.join("logs").join(&task_id).join("output.md");
        let ufo_result = if log_path.exists() {
            std::fs::read_to_string(&log_path).unwrap_or_else(|_| "Failed to read output.md".into())
        } else {
            "UFO did not generate an output.md file.".into()
        };

        if !status.success() {
            return Err(format!("UFO task failed. StdErr: {}\n\nLog Output: {}", stderr_str, ufo_result));
        }

        Ok(json!({
            "status": "success",
            "output": ufo_result,
            "task_id": task_id
        }))
    }
}
