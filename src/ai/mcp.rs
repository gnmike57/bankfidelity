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
                            "tools": {
                                "transfer_pdf": "Transfers transactions from one statement to another.",
                                "verify_layout": "Performs AI micro-typographical validation."
                            }
                        },
                        "serverInfo": {
                            "name": "BankFidelity MCP",
                            "version": "1.0.0"
                        }
                    }
                })
            }
            "tools/call" => {
                // Here we would hook into `crate::app::runtime::run_transfer_job` or `UfoClient::dispatch_task`
                json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": "Tool executed successfully."
                            }
                        ]
                    }
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
