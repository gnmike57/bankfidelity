use std::process::Command;
use serde_json::{json, Value};
use std::time::SystemTime;

pub struct UfoClient;

impl UfoClient {
    /// Dispatches a UI automation task to Microsoft UFO directly.
    /// Injects BankFidelity state context to maximize common understanding.
    pub fn dispatch_task(request: &str) -> Result<Value, String> {
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

        let output = Command::new("python")
            .arg("-m")
            .arg("ufo")
            .arg("--task")
            .arg(&task_id)
            .arg("--request")
            .arg(&context_prompt)
            .current_dir(&ufo_dir)
            .output()
            .map_err(|e| format!("Failed to execute UFO python process: {}", e))?;

        let stderr_str = String::from_utf8_lossy(&output.stderr);
        
        // Try to read the output.md
        let log_path = ufo_dir.join("logs").join(&task_id).join("output.md");
        let ufo_result = if log_path.exists() {
            std::fs::read_to_string(&log_path).unwrap_or_else(|_| "Failed to read output.md".into())
        } else {
            "UFO did not generate an output.md file.".into()
        };

        if !output.status.success() {
            return Ok(json!({
                "status": "error",
                "message": format!("UFO task failed. StdErr: {}\n\nLog Output: {}", stderr_str, ufo_result)
            }));
        }

        Ok(json!({
            "status": "success",
            "output": ufo_result,
            "task_id": task_id
        }))
    }
}
