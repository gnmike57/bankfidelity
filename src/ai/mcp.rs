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

            match serde_json::from_str::<JsonRpcRequest>(&line) {
                Ok(req) => {
                    let response = Self::handle_request(req);
                    let response_str = serde_json::to_string(&response).unwrap();
                    writeln!(stdout, "{}", response_str).unwrap();
                    stdout.flush().unwrap();
                }
                Err(e) => {
                    tracing::warn!("MCP Server received malformed payload: '{}' - Error: {}", line, e);
                }
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
                            },
                            {
                                "name": "extract_batch",
                                "description": "Extracts tabular data from an entire directory of PDFs concurrently.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "input_dir": { "type": "string", "description": "Absolute path to the input directory of PDFs." },
                                        "output_dir": { "type": "string", "description": "Absolute path to the output directory for JSON files." },
                                        "max_concurrency": { "type": "integer", "description": "Maximum concurrent workers (default: 4)." },
                                        "retries": { "type": "integer", "description": "Number of retries for failures (default: 1)." }
                                    },
                                    "required": ["input_dir", "output_dir"]
                                }
                            },
                            {
                                "name": "typst_reconstruct",
                                "description": "Programmatically rebuilds a BankStatement PDF using a declarative Typst layout.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "input": { "type": "string", "description": "Absolute path to the input BankStatement JSON (or original PDF to extract from)." },
                                        "output": { "type": "string", "description": "Absolute path to save the reconstructed PDF." }
                                    },
                                    "required": ["input", "output"]
                                }
                            },
                            {
                                "name": "local_ai_chat",
                                "description": "Delegates complex NLU intent back to the localized Qwen 7B model for offline analysis.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "instruction": { "type": "string", "description": "The complex natural language instruction." },
                                        "target_pdf": { "type": "string", "description": "Optional absolute path to the target PDF for context." }
                                    },
                                    "required": ["instruction"]
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
                    "extract_batch" => {
                        cmd.arg("extract-batch");
                        if let Some(i) = args.get("input_dir").and_then(|v| v.as_str()) { cmd.arg("--input-dir").arg(i); }
                        if let Some(o) = args.get("output_dir").and_then(|v| v.as_str()) { cmd.arg("--output-dir").arg(o); }
                        if let Some(c) = args.get("max_concurrency").and_then(|v| v.as_u64()) { cmd.arg("--max-concurrency").arg(c.to_string()); }
                        if let Some(r) = args.get("retries").and_then(|v| v.as_u64()) { cmd.arg("--retries").arg(r.to_string()); }
                    }
                    "typst_reconstruct" => {
                        cmd.arg("typst-reconstruct");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) { cmd.arg("--input").arg(i); }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) { cmd.arg("--output").arg(o); }
                    }
                    "local_ai_chat" => {
                        cmd.arg("chat");
                        if let Some(i) = args.get("instruction").and_then(|v| v.as_str()) { cmd.arg("--instruction").arg(i); }
                        if let Some(t) = args.get("target_pdf").and_then(|v| v.as_str()) { cmd.arg("--target-pdf").arg(t); }
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
                                        "text": "You are connected to BankFidelity, a highly advanced Dual-Core AI system. You have three primary directives:\n1. Maximize High Visual Fidelity: When modifying PDFs, ALWAYS sequence `modify_text` followed immediately by `verify_layout` to ensure perfect typography. If a layout fails completely, use `typst_reconstruct` to declaratively rebuild it.\n2. Prioritize Speed & Data: When processing bulk directories or requesting analysis, use `extract_data` for a single file or `extract_batch` for an entire directory to rapidly retrieve structured tabular data.\n3. Bank Statement Operations: When directed to process, audit, or operate on a bank statement, you must automatically ingest using `extract_data`, verify financial intent using `local_ai_chat`, and if modifications are requested, strictly follow Directive 1. Do not manually read financial PDFs by eye.\nAdditionally, if you encounter a complex semantic or financial intent, you can delegate the task using `local_ai_chat` to leverage BankFidelity's offline Qwen 7B model."
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
                let home_dir = dirs::home_dir().unwrap_or_default();
                let brain_base = home_dir.join(".gemini").join("antigravity").join("brain");
                
                let mut brain_dir = brain_base.to_string_lossy().to_string();
                if let Ok(entries) = std::fs::read_dir(&brain_base) {
                    let mut latest_time = std::time::SystemTime::UNIX_EPOCH;
                    for entry in entries.flatten() {
                        if let Ok(meta) = entry.metadata() {
                            if meta.is_dir() {
                                if let Ok(modified) = meta.modified() {
                                    if modified > latest_time {
                                        latest_time = modified;
                                        brain_dir = entry.path().to_string_lossy().replace("\\", "/");
                                    }
                                }
                            }
                        }
                    }
                }
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
                
                if uri.starts_with("pdf-page://") {
                    let path_and_query = uri.trim_start_matches("pdf-page://");
                    let mut parts = path_and_query.split("?page=");
                    let path = parts.next().unwrap_or("").trim();
                    let page: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
                    
                    if path.is_empty() {
                        return json!({
                            "jsonrpc": "2.0",
                            "id": req.id,
                            "error": { "code": -32602, "message": "Invalid URI format: missing PDF path" }
                        });
                    }

                    let exe = match std::env::current_exe() {
                        Ok(exe_path) => exe_path,
                        Err(e) => {
                            return json!({
                                "jsonrpc": "2.0",
                                "id": req.id,
                                "error": { "code": -32603, "message": format!("Internal error: could not determine executable path: {}", e) }
                            });
                        }
                    };

                    let mut cmd = std::process::Command::new(exe);
                    cmd.arg("mcp-render-page").arg("--input").arg(path).arg("--page").arg(page.to_string());
                    
                    match cmd.output() {
                        Ok(output) if output.status.success() => {
                            let base64_png = String::from_utf8_lossy(&output.stdout).trim().to_string();
                            json!({
                                "jsonrpc": "2.0",
                                "id": req.id,
                                "result": {
                                    "contents": [
                                        {
                                            "uri": uri,
                                            "mimeType": "image/png",
                                            "text": base64_png
                                        }
                                    ]
                                }
                            })
                        }
                        Ok(output) => {
                            let err = String::from_utf8_lossy(&output.stderr);
                            json!({
                                "jsonrpc": "2.0",
                                "id": req.id,
                                "error": { "code": -32602, "message": format!("Failed to render PDF page: {}", err) }
                            })
                        }
                        Err(e) => {
                            json!({
                                "jsonrpc": "2.0",
                                "id": req.id,
                                "error": { "code": -32602, "message": format!("Failed to spawn render process: {}", e) }
                            })
                        }
                    }
                } else if uri.starts_with("file://") {
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
