use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

/// The MCP protocol revision this server speaks. Must be a real, published
/// protocol date from the Model Context Protocol specification — clients
/// negotiate on this value during the `initialize` handshake.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut stdout = io::stdout();
        Self::serve(&mut reader, &mut stdout);
    }

    /// Core stdio loop. Generic over reader/writer so tests can inject pipes.
    ///
    /// # Panic freedom
    ///
    /// This loop must never panic: malformed input is answered with a
    /// JSON-RPC parse-error envelope and skipped; a failed stdout write means
    /// the client is gone, so the loop logs and shuts down cleanly instead of
    /// panicking (a panic here would take down the whole orchestrator).
    fn serve<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) {
        tracing::info!("Starting BankFidelity MCP Server on stdio...");

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("MCP: stdin read failed ({e}); shutting down cleanly");
                    break;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<JsonRpcRequest>(&line) {
                Ok(req) => {
                    let is_notification = req.id.is_none();
                    let response = Self::handle_request(req);
                    if is_notification {
                        continue;
                    }
                    if !Self::write_response(writer, &response) {
                        // Broken/closed stdout: the client is gone. There is
                        // nothing sensible left to serve — exit cleanly.
                        tracing::warn!("MCP: stdout unavailable; shutting down cleanly");
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "MCP Server received malformed payload: '{}' - Error: {}",
                        line,
                        e
                    );
                    // JSON-RPC 2.0: parse errors get an error envelope with a
                    // null id. Best effort — if stdout is also gone, stop.
                    let envelope = json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": -32700, "message": "Parse error" }
                    });
                    if !Self::write_response(writer, &envelope) {
                        tracing::warn!("MCP: stdout unavailable; shutting down cleanly");
                        break;
                    }
                }
            }
        }

        tracing::info!("MCP Server stdio loop ended.");
    }

    /// Serialize and write one JSON-RPC message followed by a newline.
    ///
    /// # Panic freedom
    ///
    /// Serialization failure falls back to a JSON-RPC internal-error envelope;
    /// if even that cannot be serialized the message is dropped (logged).
    /// Returns `false` only when the underlying stream is unusable.
    fn write_response<S: serde::Serialize, W: Write>(writer: &mut W, response: &S) -> bool {
        let payload = match serde_json::to_string(response) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    "MCP: response serialization failed ({e}); falling back to error envelope"
                );
                match serde_json::to_string(&json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32603,
                        "message": "Internal error: response could not be serialized"
                    }
                })) {
                    Ok(s) => s,
                    Err(e2) => {
                        tracing::error!(
                            "MCP: error envelope also failed to serialize ({e2}); dropping response"
                        );
                        return true;
                    }
                }
            }
        };

        match writeln!(writer, "{payload}").and_then(|()| writer.flush()) {
            Ok(()) => true,
            Err(e) => {
                tracing::error!("MCP: stdout write failed ({e})");
                false
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
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "BankFidelity MCP",
                            "version": env!("CARGO_PKG_VERSION")
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
                            },
                            {
                                "name": "transfer_transactions",
                                "description": "Transfers transactions from a Source PDF to a Target PDF, adapting to the target's visual layout.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "source": { "type": "string", "description": "Absolute path to the source PDF." },
                                        "target": { "type": "string", "description": "Absolute path to the target PDF layout." },
                                        "output": { "type": "string", "description": "Absolute path to save the transferred PDF." }
                                    },
                                    "required": ["source", "target", "output"]
                                }
                            },
                            {
                                "name": "export_history",
                                "description": "Cryptographically extracts the immutable .audit history from a modified BankFidelity PDF.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "input": { "type": "string", "description": "Absolute path to the modified PDF." },
                                        "output_dir": { "type": "string", "description": "Absolute path to the directory to save the history." }
                                    },
                                    "required": ["input", "output_dir"]
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

                let mut cmd =
                    std::process::Command::new(std::env::current_exe().unwrap_or_default());

                match name {
                    "balance_statement" => {
                        cmd.arg("balance");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) {
                            cmd.arg("--input").arg(i);
                        }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) {
                            cmd.arg("--output").arg(o);
                        }
                        if args
                            .get("auto_approve")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                        {
                            cmd.arg("--auto-approve");
                        }
                    }
                    "modify_text" => {
                        cmd.arg("text");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) {
                            cmd.arg("--input").arg(i);
                        }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) {
                            cmd.arg("--output").arg(o);
                        }
                        if let Some(old) = args.get("old").and_then(|v| v.as_str()) {
                            cmd.arg("--old").arg(old);
                        }
                        if let Some(new) = args.get("new").and_then(|v| v.as_str()) {
                            cmd.arg("--new").arg(new);
                        }
                        if let Some(b) = args.get("bbox").and_then(|v| v.as_str()) {
                            cmd.arg("--bbox").arg(b);
                        }
                        if let Some(p) = args.get("page").and_then(|v| v.as_u64()) {
                            cmd.arg("--page").arg(p.to_string());
                        }
                    }
                    "extract_data" => {
                        cmd.arg("extract");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) {
                            cmd.arg("--input").arg(i);
                        }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) {
                            cmd.arg("--output").arg(o);
                        }
                    }
                    "verify_layout" => {
                        cmd.arg("verify");
                        if let Some(orig) = args.get("original").and_then(|v| v.as_str()) {
                            cmd.arg("--original").arg(orig);
                        }
                        if let Some(edit) = args.get("edited").and_then(|v| v.as_str()) {
                            cmd.arg("--edited").arg(edit);
                        }
                        if let Some(out) = args.get("output_dir").and_then(|v| v.as_str()) {
                            cmd.arg("--output-dir").arg(out);
                        }
                    }
                    "extract_batch" => {
                        cmd.arg("extract-batch");
                        if let Some(i) = args.get("input_dir").and_then(|v| v.as_str()) {
                            cmd.arg("--input-dir").arg(i);
                        }
                        if let Some(o) = args.get("output_dir").and_then(|v| v.as_str()) {
                            cmd.arg("--output-dir").arg(o);
                        }
                        if let Some(c) = args.get("max_concurrency").and_then(|v| v.as_u64()) {
                            cmd.arg("--max-concurrency").arg(c.to_string());
                        }
                        if let Some(r) = args.get("retries").and_then(|v| v.as_u64()) {
                            cmd.arg("--retries").arg(r.to_string());
                        }
                    }
                    "typst_reconstruct" => {
                        cmd.arg("typst-reconstruct");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) {
                            cmd.arg("--input").arg(i);
                        }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) {
                            cmd.arg("--output").arg(o);
                        }
                    }
                    "local_ai_chat" => {
                        cmd.arg("chat");
                        if let Some(i) = args.get("instruction").and_then(|v| v.as_str()) {
                            cmd.arg("--instruction").arg(i);
                        }
                        if let Some(t) = args.get("target_pdf").and_then(|v| v.as_str()) {
                            cmd.arg("--target-pdf").arg(t);
                        }
                    }
                    "transfer_transactions" => {
                        cmd.arg("transfer-transactions");
                        if let Some(s) = args.get("source").and_then(|v| v.as_str()) {
                            cmd.arg("--source-pdf").arg(s);
                        }
                        if let Some(t) = args.get("target").and_then(|v| v.as_str()) {
                            cmd.arg("--target-pdf").arg(t);
                        }
                        if let Some(o) = args.get("output").and_then(|v| v.as_str()) {
                            cmd.arg("--output").arg(o);
                        }
                    }
                    "export_history" => {
                        cmd.arg("export-history");
                        if let Some(i) = args.get("input").and_then(|v| v.as_str()) {
                            cmd.arg("--input").arg(i);
                        }
                        if let Some(o) = args.get("output_dir").and_then(|v| v.as_str()) {
                            cmd.arg("--output-dir").arg(o);
                        }
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
                            format!(
                                "Failed with exit code {}.\nStdout:\n{}\nStderr:\n{}",
                                output.status.code().unwrap_or(-1),
                                stdout,
                                stderr
                            )
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
                                        "text": "You are connected to BankFidelity, a highly advanced Dual-Core AI system. You have complete E2E operational capability with these primary directives:\n1. Maximize High Visual Fidelity: When modifying PDFs, ALWAYS sequence `modify_text` followed immediately by `verify_layout` to ensure perfect typography. Prefer re-edit + verify over reconstruction for edit-in-place fidelity. `typst_reconstruct` is fail-closed for routine recovery because it cannot preserve edit-in-place visual fidelity; only use it as an explicit export-style rebuild when the user requests a full reconstruct.\n2. Prioritize Speed & Data: When processing bulk directories or requesting analysis, use `extract_data` for a single file or `extract_batch` for an entire directory to rapidly retrieve structured tabular data.\n3. Bank Statement Operations: When directed to process, audit, or operate on a bank statement, you must automatically ingest using `extract_data`, verify financial intent using `local_ai_chat`, and if modifications are requested, strictly follow Directive 1. Do not manually read financial PDFs by eye.\n4. Multimodal Vision: If you need to physically see a bank statement page to verify visual anomalies, request the resource `pdf-page://<absolute_path_to_pdf>?page=<page_number>` via `resources/read`. BankFidelity will natively rasterize and return a Base64 PNG.\n5. Advanced Layout Orchestration: If instructed to adapt or move data between different bank formats, use `transfer_transactions`.\n6. Audit Verification: To cryptographically prove past manipulations, use `export_history` to pull the immutable `.audit` trail.\nAdditionally, if you encounter a complex semantic or financial intent, you can delegate the task using `local_ai_chat` to leverage BankFidelity's offline Qwen 7B model."
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
                // Advertise only BankFidelity-owned capabilities. This arm previously
                // scanned ~/.gemini/antigravity/brain and exposed unrelated third-party
                // agent state to every MCP client; that leak has been removed.
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {
                        "resources": [
                            {
                                "uriTemplate": "pdf-page://{path}?page={page}",
                                "name": "PDF Page Raster",
                                "description": "Rasterized statement page. Read via resources/read using uri pdf-page://<absolute_path_to_pdf>?page=<page_number> to receive a Base64 PNG.",
                                "mimeType": "image/png"
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
                    cmd.arg("mcp-render-page")
                        .arg("--input")
                        .arg(path)
                        .arg("--page")
                        .arg(page.to_string());

                    match cmd.output() {
                        Ok(output) if output.status.success() => {
                            let base64_png =
                                String::from_utf8_lossy(&output.stdout).trim().to_string();
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
            "ping" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {}
                })
            }
            "notifications/initialized" => {
                // Notifications don't expect a response, but our loop requires returning a Value.
                // We'll return a special 'null' response which the write loop can filter out,
                // or just return a dummy response that MCP clients will ignore.
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {}
                })
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Writer that always fails like a closed/broken stdout pipe and counts
    /// how many times a write was attempted.
    struct BrokenPipeWriter {
        attempts: Arc<Mutex<usize>>,
    }

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            *self.attempts.lock().unwrap() += 1;
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "client gone"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_stdout_pipe_shuts_down_cleanly_without_panicking() {
        let attempts = Arc::new(Mutex::new(0));
        let mut writer = BrokenPipeWriter {
            attempts: Arc::clone(&attempts),
        };
        // Two valid requests: if the loop kept going after the broken pipe,
        // a second write would be attempted.
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\
                      {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let mut reader: &[u8] = input;

        // Must return normally (no panic, no hang).
        McpServer::serve(&mut reader, &mut writer);

        assert_eq!(
            *attempts.lock().unwrap(),
            1,
            "loop must stop after the first failed stdout write"
        );
    }

    #[test]
    fn malformed_input_yields_parse_error_envelope_and_loop_continues() {
        let mut out: Vec<u8> = Vec::new();
        let input = b"this is not json\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n";
        let mut reader: &[u8] = input;

        McpServer::serve(&mut reader, &mut out);

        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "parse error envelope + ping response");
        assert!(
            lines[0].contains("-32700"),
            "first reply must be a JSON-RPC parse error"
        );
        assert!(
            lines[1].contains("\"id\":7"),
            "the request after the malformed line must still be served"
        );
    }

    #[test]
    fn serialization_failure_falls_back_to_error_envelope_not_panic() {
        struct Unserializable;
        impl serde::Serialize for Unserializable {
            fn serialize<S: serde::Serializer>(&self, _s: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("nope"))
            }
        }

        let mut out: Vec<u8> = Vec::new();
        let ok = McpServer::write_response(&mut out, &Unserializable);
        assert!(ok, "serialization fallback must not report stream failure");
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("-32603"),
            "fallback must be an internal-error envelope, got: {text}"
        );
    }

    #[test]
    fn initialize_advertises_package_version_and_supported_protocol() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "initialize".into(),
            params: None,
        };
        let resp = McpServer::handle_request(req);
        assert_eq!(resp["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(
            resp["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION"),
            "serverInfo.version must come from Cargo.toml, not a hardcoded string"
        );
        assert_eq!(resp["result"]["serverInfo"]["name"], "BankFidelity MCP");
    }
}
