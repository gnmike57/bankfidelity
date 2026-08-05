use std::process::Command;
use serde_json::Value;

pub struct UfoClient;

impl UfoClient {
    /// Dispatches a UI automation task to Microsoft UFO via the Python bridge.
    pub fn dispatch_task(task: &str, app: &str) -> Result<Value, String> {
        let script = r#"
import sys
import json
try:
    from python.ufo_integration import run_ufo_task
    result = run_ufo_task(sys.argv[1], sys.argv[2])
    print(json.dumps(result))
except Exception as e:
    print(json.dumps({"status": "error", "message": str(e)}))
        "#;
        
        let output = Command::new("python")
            .arg("-c")
            .arg(script)
            .arg(task)
            .arg(app)
            .current_dir(std::env::current_dir().unwrap_or_default())
            .output()
            .map_err(|e| format!("Failed to invoke python: {}", e))?;
            
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("Failed to parse UFO output: {} - Output: {}", e, stdout))?;
            
        Ok(parsed)
    }
}
