//! Headless HTTP server — additive entry point for container/cloud
//! deployments (e.g. Railway, Fly, Cloud Run) and local MCP/agent use.
//!
//! This module does NOT change the GUI, the CLI job model, or the
//! [`crate::app::runtime::Runtime`]. It wraps the existing `Job`/`JobResult`
//! channel in a minimal HTTP/1.1 surface.
//!
//! ## Endpoints
//!
//! ### Health
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET | `/health`, `/healthz`, `/livez` | Liveness — 200 as soon as the listener is up |
//! | GET | `/readyz`, `/ready` | Readiness — pings the worker actor |
//! | GET | `/` | Status page listing all endpoints |
//!
//! ### NLP / Chat
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | POST | `/chat` | Natural-language command dispatched as `Job::AiCommand` |
//!
//! ### Document operations
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | POST | `/extract` | Extract transactions (`Job::ExtractTransactions`) |
//! | POST | `/verify` | Pixel-perfect fidelity verification (`Job::Verify`) |
//! | POST | `/transfer` | Transfer transactions between PDFs (`Job::TransferTransactions`) |
//! | POST | `/balance` | Auto-balance a statement (`Job::BalanceStatement`) |
//!
//! ### Configuration
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET | `/keys` | List which API keys are configured (names only, no values) |
//! | POST | `/keys` | Update one or more API keys at runtime (hot-reload) |
//! | POST | `/reload` | Hot-reload AppConfig from the current `.env` |
//!
//! All POST endpoints accept and return `application/json`.
//! CORS headers are included so the API can be called from browser tooling.

use crate::app::runtime::{Job, JobResult, JobTicket, RuntimeClient};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

/// Readiness uses a job-scoped result ticket so concurrent probes cannot
/// consume results owned by the GUI, CLI, or other server requests.
type RuntimeChannel = Arc<RuntimeClient>;

/// Default listen port when `$PORT` is unset.
const DEFAULT_PORT: u16 = 8080;

/// How long a readiness probe waits for the worker to answer `Ping`.
const READY_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a POST endpoint waits for a job result before timing out.
const JOB_TIMEOUT: Duration = Duration::from_secs(120);

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Run the blocking accept loop. Returns only on a fatal listener error;
/// in normal operation it runs for the lifetime of the process.
pub fn run_server(
    job_tx: RuntimeClient,
    _job_rx: Receiver<JobResult>,
    _config: Arc<crate::app::config::AppConfig>,
) -> std::io::Result<()> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr)?;
    tracing::info!("[serve] listening on http://{addr}");
    println!(
        "[serve] listening on http://{addr}\n\
         \u{2022} liveness:  GET  /health\n\
         \u{2022} readiness: GET  /readyz\n\
         \u{2022} chat:      POST /chat       {{\"message\":\"...\",\"pdf_path\":\"...\"}}\n\
         \u{2022} extract:   POST /extract    {{\"pdf_path\":\"...\",\"provider\":\"offline\"}}\n\
         \u{2022} verify:    POST /verify     {{\"original\":\"...\",\"edited\":\"...\"}}\n\
         \u{2022} balance:   POST /balance    {{\"pdf_path\":\"...\"}}\n\
         \u{2022} transfer:  POST /transfer   {{\"source_pdf\":\"...\",\"target_pdf\":\"...\"}}\n\
         \u{2022} keys:      GET  /keys       (list configured providers)\n\
         \u{2022} keys:      POST /keys       {{\"GEMINI_API_KEY\":\"...\"}}\n\
         \u{2022} reload:    POST /reload     (hot-reload .env)"
    );
    let channel: RuntimeChannel = Arc::new(job_tx);
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let ch = Arc::clone(&channel);
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(s, &ch) {
                        tracing::debug!("[serve] connection error: {e}");
                    }
                });
            }
            Err(e) => tracing::warn!("[serve] accept error: {e}"),
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Connection handler
// ─────────────────────────────────────────────────────────────────────────────

fn handle_connection(mut stream: TcpStream, channel: &RuntimeChannel) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).unwrap_or(0);
    let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
    let (method, path, body) = parse_request(&raw);
    let (status, content_type, resp_body) = route(method, path, body, channel);
    write_response(&mut stream, status, content_type, &resp_body)
}

/// Parse method, path, and body from a raw HTTP/1.1 request.
fn parse_request(req: &str) -> (&str, &str, &str) {
    let first_line = req.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");
    let body = if let Some(pos) = req.find("\r\n\r\n") {
        &req[pos + 4..]
    } else if let Some(pos) = req.find("\n\n") {
        &req[pos + 2..]
    } else {
        ""
    };
    (method, path, body)
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

fn route(
    method: &str,
    path: &str,
    body: &str,
    channel: &RuntimeChannel,
) -> (&'static str, &'static str, String) {
    let path = path.split('?').next().unwrap_or(path);

    match (method, path) {
        // ── Liveness ──────────────────────────────────────────────────────────
        ("GET", "/health" | "/healthz" | "/livez") => (
            "200 OK",
            "application/json",
            r#"{"status":"ok"}"#.to_string(),
        ),
        // ── Readiness ─────────────────────────────────────────────────────────
        ("GET", "/readyz" | "/ready") => {
            if ping_worker(channel) {
                ("200 OK", "application/json", r#"{"status":"ready"}"#.to_string())
            } else {
                ("503 Service Unavailable", "application/json", r#"{"status":"not-ready"}"#.to_string())
            }
        }
        // ── NLP Chat ──────────────────────────────────────────────────────────
        ("POST", "/chat") => handle_chat(body, channel),
        // ── Document operations ───────────────────────────────────────────────
        ("POST", "/extract") => handle_extract(body, channel),
        ("POST", "/verify") => handle_verify(body, channel),
        ("POST", "/balance") => handle_balance(body, channel),
        ("POST", "/transfer") => handle_transfer(body, channel),
        // ── Key management ────────────────────────────────────────────────────
        ("GET", "/keys") => handle_keys_get(),
        ("POST", "/keys") => handle_keys_post(body, channel),
        ("POST", "/reload") => handle_reload(channel),
        // ── CORS preflight ────────────────────────────────────────────────────
        ("OPTIONS", _) => ("204 No Content", "text/plain", String::new()),
        // ── Status page ───────────────────────────────────────────────────────
        ("GET", "/") => ("200 OK", "text/html; charset=utf-8", status_page()),
        // ── Method not allowed ────────────────────────────────────────────────
        (m, _) if m != "GET" && m != "POST" && m != "OPTIONS" => (
            "405 Method Not Allowed",
            "application/json",
            r#"{"error":"method not allowed"}"#.to_string(),
        ),
        // ── Not found ─────────────────────────────────────────────────────────
        _ => ("404 Not Found", "application/json", r#"{"error":"not found"}"#.to_string()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /chat
// ─────────────────────────────────────────────────────────────────────────────

fn handle_chat(body: &str, channel: &RuntimeChannel) -> (&'static str, &'static str, String) {
    let message = json_str(body, "message").unwrap_or_default();
    let pdf_path = json_str(body, "pdf_path").unwrap_or_default();
    if message.is_empty() {
        return ("400 Bad Request", "application/json", r#"{"error":"'message' field is required"}"#.to_string());
    }
    let path = if pdf_path.is_empty() { PathBuf::new() } else { PathBuf::from(&pdf_path) };
    let ticket = match channel.submit_headless(Job::AiCommand { prompt: message, path }) {
        Ok(t) => t,
        Err(e) => return ("503 Service Unavailable", "application/json", format!(r#"{{"error":"runtime unavailable: {}"}}"#, e)),
    };
    collect_results(ticket, "chat")
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /extract
// ─────────────────────────────────────────────────────────────────────────────

fn handle_extract(body: &str, channel: &RuntimeChannel) -> (&'static str, &'static str, String) {
    let pdf_path = match json_str(body, "pdf_path") {
        Some(p) if !p.is_empty() => p,
        _ => return ("400 Bad Request", "application/json", r#"{"error":"'pdf_path' field is required"}"#.to_string()),
    };
    let provider_str = json_str(body, "provider").unwrap_or_else(|| "offline".to_string());
    let parser_mode = match provider_str.to_lowercase().as_str() {
        "llamaparse" => crate::app::config::DocumentParserMode::LlamaParse,
        "documentai" | "document_ai" => crate::app::config::DocumentParserMode::DocumentAi,
        "ocr" | "localocrs" => crate::app::config::DocumentParserMode::LocalOcrs,
        _ => crate::app::config::DocumentParserMode::OfflineHeuristic,
    };
    let ticket = match channel.submit_headless(Job::ExtractTransactions {
        path: PathBuf::from(&pdf_path),
        parser_mode,
    }) {
        Ok(t) => t,
        Err(e) => return ("503 Service Unavailable", "application/json", format!(r#"{{"error":"runtime unavailable: {}"}}"#, e)),
    };
    collect_results(ticket, "extract")
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /verify
// ─────────────────────────────────────────────────────────────────────────────

fn handle_verify(body: &str, channel: &RuntimeChannel) -> (&'static str, &'static str, String) {
    let original = match json_str(body, "original").or_else(|| json_str(body, "pdf_path")) {
        Some(p) if !p.is_empty() => p,
        _ => return ("400 Bad Request", "application/json", r#"{"error":"'original' or 'pdf_path' field is required"}"#.to_string()),
    };
    let edited = json_str(body, "edited").unwrap_or_else(|| original.clone());
    let output_dir = json_str(body, "output_dir").unwrap_or_else(|| "/tmp/bankfidelity_verify".to_string());
    let _ = std::fs::create_dir_all(&output_dir);
    let ticket = match channel.submit_headless(Job::Verify {
        original: PathBuf::from(&original),
        edited: PathBuf::from(&edited),
        output_dir: PathBuf::from(&output_dir),
        intended_edits: vec![],
        use_pdfrest: false,
        pdfrest_key: None,
        auto_match_dpi: true,
    }) {
        Ok(t) => t,
        Err(e) => return ("503 Service Unavailable", "application/json", format!(r#"{{"error":"runtime unavailable: {}"}}"#, e)),
    };
    collect_results(ticket, "verify")
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /balance
// ─────────────────────────────────────────────────────────────────────────────

fn handle_balance(body: &str, channel: &RuntimeChannel) -> (&'static str, &'static str, String) {
    let pdf_path = match json_str(body, "pdf_path") {
        Some(p) if !p.is_empty() => p,
        _ => return ("400 Bad Request", "application/json", r#"{"error":"'pdf_path' field is required"}"#.to_string()),
    };
    let ticket = match channel.submit_headless(Job::BalanceStatement {
        path: PathBuf::from(&pdf_path),
    }) {
        Ok(t) => t,
        Err(e) => return ("503 Service Unavailable", "application/json", format!(r#"{{"error":"runtime unavailable: {}"}}"#, e)),
    };
    collect_results(ticket, "balance")
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /transfer
// ─────────────────────────────────────────────────────────────────────────────

fn handle_transfer(body: &str, channel: &RuntimeChannel) -> (&'static str, &'static str, String) {
    let source = match json_str(body, "source_pdf").or_else(|| json_str(body, "pdf_path")) {
        Some(p) if !p.is_empty() => p,
        _ => return ("400 Bad Request", "application/json", r#"{"error":"'source_pdf' field is required"}"#.to_string()),
    };
    let target = match json_str(body, "target_pdf") {
        Some(p) if !p.is_empty() => p,
        _ => return ("400 Bad Request", "application/json", r#"{"error":"'target_pdf' field is required"}"#.to_string()),
    };
    let output = json_str(body, "output_pdf").unwrap_or_else(|| "/tmp/bankfidelity_transfer_out.pdf".to_string());
    let ticket = match channel.submit_headless(Job::TransferTransactions {
        source_pdf: PathBuf::from(&source),
        target_pdf: PathBuf::from(&target),
        output_pdf: PathBuf::from(&output),
    }) {
        Ok(t) => t,
        Err(e) => return ("503 Service Unavailable", "application/json", format!(r#"{{"error":"runtime unavailable: {}"}}"#, e)),
    };
    collect_results(ticket, "transfer")
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /keys
// ─────────────────────────────────────────────────────────────────────────────

fn handle_keys_get() -> (&'static str, &'static str, String) {
    let providers = [
        ("gemini", "GEMINI_API_KEY"),
        ("pymupdf_pro", "PYMUPDF_PRO_KEY"),
        ("llamaparse", "LLAMAPARSE_API_KEY"),
        ("pdfrest", "PDFREST_API_KEY"),
        ("mistral", "MISTRAL_API_KEY"),
        ("openrouter", "OPENROUTER_API_KEY"),
        ("document_ai", "DOCUMENT_AI_API_KEY"),
        ("groq", "GROQ_API_KEY"),
        ("mindee", "MINDEE_API_KEY"),
    ];
    let entries: Vec<String> = providers.iter().map(|(name, env_var)| {
        let configured = std::env::var(env_var).map(|v| !v.trim().is_empty()).unwrap_or(false);
        format!(r#"{{"provider":"{name}","configured":{configured}}}"#)
    }).collect();
    ("200 OK", "application/json", format!(r#"{{"providers":[{}]}}"#, entries.join(",")))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /keys
// ─────────────────────────────────────────────────────────────────────────────

fn handle_keys_post(body: &str, channel: &RuntimeChannel) -> (&'static str, &'static str, String) {
    let allowed = [
        "GEMINI_API_KEY", "GEMINI_AUTH_MODE", "PYMUPDF_PRO_KEY",
        "LLAMAPARSE_API_KEY", "PDFREST_API_KEY", "MISTRAL_API_KEY",
        "MISTRAL_MODEL", "OPENROUTER_API_KEY", "OPENROUTER_MODEL",
        "DOCUMENT_AI_API_KEY", "DOCUMENT_AI_PROJECT_ID", "DOCUMENT_AI_LOCATION",
        "DOCUMENT_AI_PROCESSOR_ID", "GOOGLE_APPLICATION_CREDENTIALS",
        "GROQ_API_KEY", "MINDEE_API_KEY",
    ];
    let mut updated: Vec<String> = Vec::new();
    for key in &allowed {
        if let Some(val) = json_str(body, key) {
            if val.is_empty() {
                std::env::remove_var(key);
            } else {
                std::env::set_var(key, &val);
            }
            let _ = upsert_env_file(std::path::Path::new(".env"), &[(key, val)]);
            updated.push(key.to_string());
        }
    }
    if updated.is_empty() {
        return ("400 Bad Request", "application/json", r#"{"error":"no recognised key fields in request body"}"#.to_string());
    }
    let _ = channel.send(Job::ReloadConfig);
    ("200 OK", "application/json", format!(r#"{{"ok":true,"updated":{}}}"#, json_arr(&updated)))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /reload
// ─────────────────────────────────────────────────────────────────────────────

fn handle_reload(channel: &RuntimeChannel) -> (&'static str, &'static str, String) {
    match channel.send(Job::ReloadConfig) {
        Ok(_) => ("200 OK", "application/json", r#"{"ok":true,"message":"ReloadConfig dispatched"}"#.to_string()),
        Err(e) => ("503 Service Unavailable", "application/json", format!(r#"{{"error":"runtime unavailable: {}"}}"#, e)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Collect job results into a JSON response
// ─────────────────────────────────────────────────────────────────────────────

fn collect_results(ticket: JobTicket, job_name: &str) -> (&'static str, &'static str, String) {
    let mut messages: Vec<String> = Vec::new();
    let mut ok = true;
    let deadline = std::time::Instant::now() + JOB_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            ok = false;
            messages.push("job timed out".to_string());
            break;
        }
        match ticket.recv_timeout(remaining.min(Duration::from_secs(2))) {
            Ok(JobResult::Progress { label, .. }) => { messages.push(label); }
            Ok(JobResult::Error { message, .. }) => { ok = false; messages.push(message); break; }
            Ok(JobResult::NaturalLanguageEditReady(edits)) => {
                messages.push(format!("{} proposed change(s) ready", edits.len()));
                break;
            }
            Ok(JobResult::TransactionsExtracted(txns)) => {
                messages.push(format!("{} transaction(s) extracted", txns.len()));
                break;
            }
            Ok(JobResult::VerificationReport(r)) => {
                messages.push(format!("verification {}", if r.mandatory_local_pass() { "PASS" } else { "FAIL" }));
                break;
            }
            Ok(JobResult::BalanceProposed { imbalance, changes }) => {
                messages.push(format!("balance: imbalance={imbalance}, {} change(s) proposed", changes.len()));
                break;
            }
            Ok(JobResult::ConfigReloaded { .. }) => {
                messages.push("config reloaded".to_string());
                break;
            }
            Ok(JobResult::TransferComplete(r)) => {
                messages.push(format!(
                    "transfer complete: src={} tx, tgt={} tx, pages_added={}, math_verified={}, visual_score={:.2}",
                    r.source_tx_count, r.target_tx_count, r.pages_added, r.math_verified, r.visual_score
                ));
                break;
            }
            Ok(JobResult::WorkflowComplete(summary)) => {
                messages.push(format!("workflow complete: {}", summary.completion_summary));
                break;
            }
            Ok(JobResult::ProposedChangesApplied { changes_applied, .. }) => {
                messages.push(format!("{} change(s) applied", changes_applied));
                break;
            }
            Ok(JobResult::ApiKeysVerified(report)) => {
                messages.push(format!("api keys verified: {} provider(s) checked", report.results.len()));
                break;
            }

            Ok(_) => {}  // Progress or other non-terminal variants — keep looping
            Err(_) => break,
        }
    }
    let status = if ok { "200 OK" } else { "500 Internal Server Error" };
    let body = format!(r#"{{"ok":{ok},"job":"{job_name}","messages":{}}}"#, json_arr(&messages));
    (status, "application/json", body)
}

// ─────────────────────────────────────────────────────────────────────────────
// Ping worker
// ─────────────────────────────────────────────────────────────────────────────

fn ping_worker(channel: &RuntimeChannel) -> bool {
    let ticket = match channel.submit_headless(Job::Ping) {
        Ok(t) => t,
        Err(_) => return false,
    };
    loop {
        match ticket.recv_timeout(READY_TIMEOUT) {
            Ok(JobResult::Pong) => return true,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Minimal JSON helpers (no external crate)
// ─────────────────────────────────────────────────────────────────────────────

fn json_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let colon = after.find(':')? + 1;
    let val = after[colon..].trim_start();
    if let Some(inner) = val.strip_prefix('"') {
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else {
        None
    }
}

fn json_arr(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| {
        let e = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        format!("\"{e}\"")
    }).collect();
    format!("[{}]", inner.join(","))
}

fn upsert_env_file(path: &std::path::Path, pairs: &[(&str, String)]) -> std::io::Result<()> {
    use std::io::BufRead;
    let existing: Vec<String> = if path.exists() {
        let f = std::fs::File::open(path)?;
        std::io::BufReader::new(f).lines().collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };
    let mut output: Vec<String> = existing.into_iter().map(|line| {
        let t = line.trim();
        if t.starts_with('#') || !t.contains('=') { return line; }
        let k = t.split('=').next().unwrap_or("").trim();
        if let Some((_, v)) = pairs.iter().find(|(pk, _)| *pk == k) {
            format!("{}={}", k, v)
        } else {
            line
        }
    }).collect();
    for (key, val) in pairs {
        let present = output.iter().any(|l| {
            let t = l.trim();
            !t.starts_with('#') && t.split('=').next().map(|k| k.trim()) == Some(key)
        });
        if !present && !val.is_empty() {
            output.push(format!("{}={}", key, val));
        }
    }
    std::fs::write(path, output.join("\n") + "\n")
}

// ─────────────────────────────────────────────────────────────────────────────
// Write HTTP response
// ─────────────────────────────────────────────────────────────────────────────

fn write_response(
    stream: &mut TcpStream,
    status_line: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status_line}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

// ─────────────────────────────────────────────────────────────────────────────
// Status page HTML
// ─────────────────────────────────────────────────────────────────────────────

fn status_page() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>BankFidelity API</title>
<style>
  body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#0d1117;color:#e6edf3;margin:0;padding:40px}
  h1{color:#58a6ff;margin-bottom:4px}
  h2{color:#8b949e;font-weight:400;margin-top:0}
  table{border-collapse:collapse;width:100%;max-width:860px;margin:24px 0}
  th,td{padding:10px 14px;text-align:left;border:1px solid #30363d}
  th{background:#161b22;color:#8b949e;font-size:.85em;text-transform:uppercase}
  code{background:#161b22;padding:2px 6px;border-radius:4px;font-size:.9em;color:#a3be8c}
  .get{background:#1f6feb;color:#fff;padding:2px 8px;border-radius:4px;font-size:.8em;font-weight:600}
  .post{background:#238636;color:#fff;padding:2px 8px;border-radius:4px;font-size:.8em;font-weight:600}
</style>
</head>
<body>
<h1>BankFidelity</h1>
<h2>Headless API — Bank Statement Fidelity Editor</h2>
<table>
<tr><th>Method</th><th>Path</th><th>Body / Description</th></tr>
<tr><td><span class="get">GET</span></td><td><code>/health</code></td><td>Liveness probe</td></tr>
<tr><td><span class="get">GET</span></td><td><code>/readyz</code></td><td>Readiness probe (pings worker)</td></tr>
<tr><td><span class="post">POST</span></td><td><code>/chat</code></td><td><code>{"message":"...","pdf_path":"..."}</code> — NLP command</td></tr>
<tr><td><span class="post">POST</span></td><td><code>/extract</code></td><td><code>{"pdf_path":"...","provider":"offline|llamaparse|documentai"}</code></td></tr>
<tr><td><span class="post">POST</span></td><td><code>/verify</code></td><td><code>{"original":"...","edited":"...","output_dir":"..."}</code></td></tr>
<tr><td><span class="post">POST</span></td><td><code>/balance</code></td><td><code>{"pdf_path":"..."}</code></td></tr>
<tr><td><span class="post">POST</span></td><td><code>/transfer</code></td><td><code>{"source_pdf":"...","target_pdf":"...","output_pdf":"..."}</code></td></tr>
<tr><td><span class="get">GET</span></td><td><code>/keys</code></td><td>List configured providers (no key values)</td></tr>
<tr><td><span class="post">POST</span></td><td><code>/keys</code></td><td><code>{"GEMINI_API_KEY":"...","MISTRAL_API_KEY":"..."}</code></td></tr>
<tr><td><span class="post">POST</span></td><td><code>/reload</code></td><td>Hot-reload .env into runtime config</td></tr>
</table>
</body>
</html>"#.to_string()
}
