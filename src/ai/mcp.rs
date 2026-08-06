use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

/// A foundational MCP Server implementation for BankFidelity.
/// This allows external clients (like Claude Desktop) to connect to `bankfidelity`
/// via stdio and invoke our PDF capabilities natively.
pub struct McpServer;

impl McpServer {
    pub fn start() {
        tracing::info!("Starting BankFidelity MCP Server on stdio...");
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        // Send initialization capability (MCP handshake)
        // Usually, the client sends "initialize", but we just listen for standard JSON-RPC.
        
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(&line) {
                let response = Self::handle_request(req);
                let response_str = serde_json::to_string(&response).unwrap();
                writeln!(stdout, "{}", response_str).unwrap();
                stdout.flush().unwrap();
            }
        }
    }

    fn handle_request(req: JsonRpcRequest) -> Value {
        match req.method.as_str() {
            "initialize" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {
                        "protocolVersion": "2026-07-28",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "BankFidelity MCP",
                            "version": "1.0.0"
                        }
                    }
                })
            }
            "tools/list" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {
                        "tools": [
                            {
                                "name": "balance_statement",
                                "description": "Balances the entire statement (T8 + T9) by fixing mathematical inaccuracies.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "input": { "type": "string", "description": "Absolute path to the input PDF." },
                                        "output": { "type": "string", "description": "Absolute path to save the balanced PDF." },
                                        "auto_approve": { "type": "boolean", "description": "Whether to auto-approve the balance modifications." }
                                    },
                                    "required": ["input", "output"]
                                }
                            },
                            {
                                "name": "modify_text",
                                "description": "Modifies text on a specific page with high visual fidelity.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "input": { "type": "string", "description": "Absolute path to the input PDF." },
                                        "output": { "type": "string", "description": "Absolute path to save the modified PDF." },
                                        "old": { "type": "string", "description": "The old text to replace." },
                                        "new": { "type": "string", "description": "The new text to insert." },
                                        "bbox": { "type": "string", "description": "The bounding box (x0,y0,x1,y1)." },
                                        "page": { "type": "integer", "description": "The page number (1-indexed)." }
                                    },
                                    "required": ["input", "output", "old", "new", "bbox"]
                                }
                            },
                            {
                                "name": "extract_data",
                                "description": "Extracts document-level tabular data as JSON.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "input": { "type": "string", "description": "Absolute path to the input PDF." },
                                        "output": { "type": "string", "description": "Absolute path to save the extracted JSON." }
                                    },
                                    "required": ["input", "output"]
                                }
                            },
                            {
                                "name": "verify_layout",
                                "description": "Verifies visual and mathematical integrity of an edited document.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "original": { "type": "string", "description": "Absolute path to the original PDF." },
                                        "edited": { "type": "string", "description": "Absolute path to the edited PDF." },
                                        "output_dir": { "type": "string", "description": "Directory to save verification artifacts." }
                                    },
                                    "required": ["original", "edited", "output_dir"]
                                }
                            }
                        ]
                    }
                })
            }
            "tools/call" => {
                let params = req.params.unwrap_or(json!({}));
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let default_args = json!({});
                let args = params.get("arguments").unwrap_or(&default_args);

                let mut cmd = std::process::Command::new(std::env::current_exe().unwrap_or_default());

                match name {
                    "balance_statement" => {
                        cmd.arg("balance");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) { cmd.arg("--input").arg(i); }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) { cmd.arg("--output").arg(o); }
                        if args.get("auto_approve").and_then(|v| v.as_bool()).unwrap_or(false) {
                            cmd.arg("--auto-approve");
                        }
                    }
                    "modify_text" => {
                        cmd.arg("text");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) { cmd.arg("--input").arg(i); }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) { cmd.arg("--output").arg(o); }
                        if let Some(old) = args.get("old").and_then(|v| v.as_str()) { cmd.arg("--old").arg(old); }
                        if let Some(new) = args.get("new").and_then(|v| v.as_str()) { cmd.arg("--new").arg(new); }
                        if let Some(b) = args.get("bbox").and_then(|v| v.as_str()) { cmd.arg("--bbox").arg(b); }
                        if let Some(p) = args.get("page").and_then(|v| v.as_u64()) { cmd.arg("--page").arg(p.to_string()); }
                    }
                    "extract_data" => {
                        cmd.arg("extract");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) { cmd.arg("--input").arg(i); }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) { cmd.arg("--output").arg(o); }
                    }
                    "verify_layout" => {
                        cmd.arg("verify");
                        if let Some(orig) = args.get("original").and_then(|v| v.as_str()) { cmd.arg("--original").arg(orig); }
                        if let Some(edit) = args.get("edited").and_then(|v| v.as_str()) { cmd.arg("--edited").arg(edit); }
                        if let Some(out) = args.get("output_dir").and_then(|v| v.as_str()) { cmd.arg("--output-dir").arg(out); }
                    }
                    _ => {
                        return json!({
                            "jsonrpc": "2.0",
                            "id": req.id,
                            "error": {
                                "code": -32601,
                                "message": format!("Tool '{}' not found", name)
                            }
                        });
                    }
                }

                match cmd.output() {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        let result_text = if output.status.success() {
                            format!("Success.\nStdout:\n{}\nStderr:\n{}", stdout, stderr)
                        } else {
                            format!("Failed with exit code {}.\nStdout:\n{}\nStderr:\n{}", output.status.code().unwrap_or(-1), stdout, stderr)
                        };

                        json!({
                            "jsonrpc": "2.0",
                            "id": req.id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": result_text
                                    }
                                ],
                                "isError": !output.status.success()
                            }
                        })
                    }
                    Err(e) => {
                        json!({
                            "jsonrpc": "2.0",
                            "id": req.id,
                            "error": {
                                "code": -32000,
                                "message": format!("Failed to spawn process: {}", e)
                            }
                        })
                    }
                }
            }
            "prompts/list" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {
                        "prompts": [
                            {
                                "name": "bankfidelity_agent_instructions",
                                "description": "Core instructions for agents interacting with BankFidelity.",
                                "arguments": []
                            }
                        ]
                    }
                })
            }
            "prompts/get" => {
                let params = req.params.unwrap_or(json!({}));
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                
                if name == "bankfidelity_agent_instructions" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": req.id,
                        "result": {
                            "description": "Core instructions for agents interacting with BankFidelity.",
                            "messages": [
                                {
                                    "role": "user",
                                    "content": {
                                        "type": "text",
                                        "text": "You are connected to BankFidelity, a highly advanced Dual-Core AI system. You have two primary directives: 1. Maximize High Visual Fidelity: When modifying PDFs, ALWAYS sequence `modify_text` followed immediately by `verify_layout` to ensure perfect typography. 2. Prioritize Speed & Data: When processing bulk directories or requesting analysis, use `extract_data` for rapid, structured tabular data retrieval. Balance both flawless aesthetics and high-performance extraction based on the user's workflow."
                                    }
                                }
                            ]
                        }
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": req.id,
                        "error": { "code": -32602, "message": format!("Prompt '{}' not found", name) }
                    })
                }
            }
            "resources/list" => {
                let brain_dir = "C:/Users/zbook/.gemini/antigravity-ide/brain/e7e6cf9f-8c56-43f7-8528-1d362f4a7acb";
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {
                        "resources": [
                            {
                                "uri": format!("file://{}/task.md", brain_dir),
                                "name": "Current Task List",
                                "mimeType": "text/markdown"
                            },
                            {
                                "uri": format!("file://{}/walkthrough.md", brain_dir),
                                "name": "Project Walkthrough",
                                "mimeType": "text/markdown"
                            }
                        ]
                    }
                })
            }
            "resources/read" => {
                let params = req.params.unwrap_or(json!({}));
                let uri = params.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                
                if uri.starts_with("file://") {
                    let path = uri.trim_start_matches("file://");
                    match std::fs::read_to_string(path) {
                        Ok(content) => {
                            json!({
                                "jsonrpc": "2.0",
                                "id": req.id,
                                "result": {
                                    "contents": [
                                        {
                                            "uri": uri,
                                            "mimeType": "text/markdown",
                                            "text": content
                                        }
                                    ]
                                }
                            })
                        }
                        Err(e) => {
                            json!({
                                "jsonrpc": "2.0",
                                "id": req.id,
                                "error": { "code": -32602, "message": format!("Failed to read resource: {}", e) }
                            })
                        }
                    }
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": req.id,
                        "error": { "code": -32602, "message": "Invalid resource URI" }
                    })
                }
            }
            _ => {
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "error": {
                        "code": -32601,
                        "message": "Method not found"
                    }
                })
            }
        }
    }
}
