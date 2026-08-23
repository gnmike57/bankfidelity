//! Unified CLI Implementation
//! Provides parity between GUI and CLI capabilities by sharing the same Runtime Job interface.

use crate::app::audit::AuditLogParser;
use crate::app::env_spec::{self, Requirement};
use crate::app::runtime::{Job, JobResult, OperationDisposition, RuntimeClient};
use crate::engine::history::ChangeHistory;
use crate::error::exit_code;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

#[derive(Parser)]
#[command(name = "dual-core-pdf-pipeline")]
#[command(version)]
#[command(
    about = "Bank Statement Fidelity Editor - high-fidelity PDF editing toolkit",
    long_about = "Bank Statement Fidelity Editor CLI\n\n\
        A toolkit for rendering, extracting, and verifying PDF documents with the \
        same capabilities as the GUI.\n\n\
        FIRST-TIME SETUP:\n  \
        1. Copy .env.example to .env and fill in the required values.\n  \
        2. Run `dual-core-pdf-pipeline doctor` to verify your configuration.\n  \
        3. Use `dual-core-pdf-pipeline <command> --help` for command-specific options.\n\n\
        EXIT CODES:\n  \
        0 success · 1 general error · 2 config · 3 invalid input · \
        4 not found · 5 I/O · 6 partial success"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Launch the GUI (recommended)
    Gui,

    /// Start the Model Context Protocol (MCP) Server loop over stdio.
    Mcp,

    /// Dispatch a UI automation task to Microsoft UFO
    Ufo {
        /// The instruction for UFO (e.g. "download bank statement from chrome")
        request: String,
    },

    /// Chat with the Local LLM orchestrator to perform edits or commands via natural language
    Chat {
        /// The natural language instruction (e.g., "shift dates by 5 days")
        #[arg(short, long)]
        instruction: String,
        /// Optional target PDF for context
        #[arg(short = 't', long)]
        target_pdf: Option<PathBuf>,
    },

    /// Run headless and expose an HTTP health surface (for containers /
    /// cloud platforms like Railway). Binds 0.0.0.0:$PORT (default 8080)
    /// and keeps the worker runtime alive. Reuses the same Job/JobResult
    /// runtime as the GUI and CLI - no separate code path.
    Serve,

    /// Modify text with high visual fidelity
    Text {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        old: String,
        #[arg(long)]
        new: String,
        #[arg(short, long)]
        page: Option<usize>,
        #[arg(long)]
        bbox: String,
    },

    /// Balance the entire statement (T8 + T9)
    Balance {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value_t = false)]
        auto_approve: bool,
    },

    /// Extract document-level data as JSON (T8)
    Extract {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Extract every PDF under a directory with bounded workers and per-file results.
    #[command(name = "extract-batch")]
    ExtractBatch {
        #[arg(long)]
        input_dir: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, default_value_t = 4)]
        max_concurrency: usize,
        #[arg(long, default_value_t = 1)]
        retries: usize,
    },

    /// High-Fidelity Typst layout reconstruction and reflowing
    TypstReconstruct {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// MCP Internal: Renders a page of a PDF to base64 PNG
    McpRenderPage {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        page: usize,
    },

    /// Verify visual and mathematical integrity (T7)
    Verify {
        #[arg(short, long)]
        original: PathBuf,
        #[arg(short, long)]
        edited: PathBuf,
        /// Directory for the verification report and diff renders.
        /// Long flag only - `-o` would collide with `--original`.
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long)]
        use_pdfrest: bool,
    },

    /// Render a specific page to PNG
    Render {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output_dir: PathBuf,
        #[arg(short, long)]
        page: usize,
        #[arg(long, default_value_t = 300.0)]
        dpi: f32,
    },

    /// Complete missing characters in a font (T5)
    #[command(name = "font-complete")]
    FontComplete {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        font: String,
    },

    /// Reconstruct history from an audit log (AC#6)
    #[command(name = "export-history")]
    ExportHistory {
        #[arg(long)]
        from_log: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Hidden ping for runtime verification
    #[command(hide = true)]
    Ping,

    /// Hidden end-to-end self-test: render -> edit -> re-render -> verify on a
    /// bundled example PDF, asserting the edit lands and is visually localized.
    /// Exits 0 on PASS, non-zero on FAIL. Useful for CI and quick sanity checks.
    #[command(hide = true)]
    Selftest {
        /// PDF to exercise. Defaults to examples/sample.pdf.
        #[arg(long)]
        input: Option<PathBuf>,
    },

    /// Print configuration health check (env vars, file paths, runtime ping)
    Doctor,

    /// Verify all API keys can make successful API calls with fallback chains
    #[command(name = "verify-api-keys")]
    VerifyApiKeys {
        /// Output results in JSON format for CI/CD
        #[arg(long)]
        json: bool,
    },

    /// Document AI training orchestration (Stage 4 / Item #12).
    ///
    /// Reports labelled-document count and, when the dataset has at least
    /// `--min-labelled` documents (default 8), kicks off training of a new
    /// processor version. Polls the operation until it completes.
    DocaiTrain {
        /// Human-readable display name for the new processor version.
        /// Auto-generated from a timestamp when omitted.
        #[arg(long)]
        display_name: Option<String>,
        /// Minimum labelled documents required before training is permitted.
        #[arg(long, default_value_t = 8)]
        min_labelled: usize,
        /// After training, set the new version as the processor's default.
        #[arg(long, default_value_t = false)]
        set_default: bool,
        /// Skip the actual training step; just report the dataset state.
        #[arg(long, default_value_t = false)]
        report_only: bool,
    },

    /// Stage 12 / Item #1: bootstrap the font cache used by the Stage 11
    /// donor cascade.
    ///
    /// Downloads a curated seed of Google Fonts to `cache/fonts/` and
    /// writes a manifest mapping canonical typeface names to local TTF
    /// paths. Without this the cascade's Tier 2 (subset extension from
    /// donor) and Tier 3 (Gemini Vision typeface ID + donor lookup) are
    /// inert.
    FontcacheInit {
        /// Force re-download even if a font is already cached.
        #[arg(long, default_value_t = false)]
        force: bool,
        /// Override the cache directory. Defaults to `./cache/fonts`.
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
    },

    /// Stage 8.5: Standalone font analysis trigger
    #[command(name = "analyze-fonts")]
    AnalyzeFonts {
        #[arg(short, long)]
        input: PathBuf,
    },

    /// Run the Smart Balance Engine and apply all proposed adjustments
    #[command(name = "auto-balance")]
    AutoBalance {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Use AI to fix text box issues and visual fidelity differences
    #[command(name = "ai-fix-visual")]
    AiFixVisual {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        page: usize,
    },

    /// Transfer transactions from one bank statement to another
    #[command(name = "transfer-transactions")]
    TransferTransactions {
        #[arg(long)]
        source_pdf: PathBuf,
        #[arg(long)]
        target_pdf: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Bulk-shift or remap all transaction dates
    #[command(name = "adjust-dates")]
    AdjustDates {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Mode (e.g., 'shift-forward-1-month', 'randomize-days')
        #[arg(long)]
        mode: String,
    },

    /// Run cross-statement transfer tests on a set of PDFs
    #[command(name = "run-transfer-tests")]
    RunTransferTests {
        /// List of PDFs to test transfer matrix
        #[arg(long, value_delimiter = ',')]
        statements: Vec<PathBuf>,
        #[arg(long, default_value_t = 10)]
        max_iterations: u32,
    },
}

/// Parses a bounding box string in the format "x0,y0,x1,y1".
///
/// # Errors
/// Returns an error if the string is malformed or contains invalid numbers.
fn parse_bbox(bbox: &str) -> Result<[f32; 4], String> {
    let parts: Vec<&str> = bbox.split(',').collect();
    if parts.len() != 4 {
        return Err(format!(
            "bbox must have 4 comma-separated values (x0,y0,x1,y1), got {} parts",
            parts.len()
        ));
    }

    let mut coords = [0.0f32; 4];
    for (i, part) in parts.iter().enumerate() {
        match part.trim().parse::<f32>() {
            Ok(v) => coords[i] = v,
            Err(e) => {
                return Err(format!(
                    "bbox value {} ('{}') is not a valid number: {}",
                    i + 1,
                    part,
                    e
                ));
            }
        }
    }

    // Validate coordinates form a valid rectangle
    if coords[0] >= coords[2] {
        return Err(format!(
            "bbox x0 ({}) must be less than x1 ({})",
            coords[0], coords[2]
        ));
    }
    if coords[1] >= coords[3] {
        return Err(format!(
            "bbox y0 ({}) must be less than y1 ({})",
            coords[1], coords[3]
        ));
    }

    Ok(coords)
}

/// Validates that a path exists and is a PDF file.
///
/// # Errors
/// Returns an error if the file doesn't exist or isn't a PDF.
fn validate_pdf_path(path: &std::path::Path, name: &str) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} not found: {}", name, path.display()));
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase());

    if ext != Some("pdf".to_string()) {
        return Err(format!(
            "{} must be a PDF file, got: {:?}",
            name,
            path.extension().and_then(|s| s.to_str())
        ));
    }

    Ok(())
}

/// Blocking synchronous receiver helper
/// Drains progress beats and handles errors.
fn wait_for_terminal_result(job_rx: &Receiver<JobResult>) -> Result<JobResult, (String, String)> {
    loop {
        match job_rx.recv() {
            Ok(JobResult::Progress { label, fraction }) => {
                tracing::info!("[progress] {}: {:.0}%", label, fraction * 100.0);
            }
            // `LoadDocument` fires an async font-analysis task that emits
            // `FontAnalysisReady` independently of the document-load result -
            // and on a cache hit it can arrive *first*. It is not a terminal
            // result for any CLI flow, so skip it (otherwise `extract` and
            // friends mistake it for their answer and report "unexpected
            // result"). The font analysis is surfaced in the GUI separately.
            Ok(JobResult::FontAnalysisReady(_)) => {
                tracing::debug!("[cli] ignoring non-terminal FontAnalysisReady");
            }
            // Likewise, an incidental cascade report is informational only.
            Ok(JobResult::FontCascadeUsed(_)) => {
                tracing::debug!("[cli] ignoring non-terminal FontCascadeUsed");
            }
            // `ApplyChange` emits a `HistoryUpdated` side-effect *after* the
            // terminal `ChangeApplied`. For sequential CLI flows that apply an
            // edit then immediately issue another job (e.g. re-render in
            // `selftest`), this would otherwise be mistaken for the next job's
            // result. It is never a terminal result for a CLI command, so skip.
            Ok(JobResult::HistoryUpdated { .. }) => {
                tracing::debug!("[cli] ignoring non-terminal HistoryUpdated");
            }
            Ok(JobResult::ApiKeysVerified(_)) => {
                tracing::debug!("[cli] ignoring non-terminal ApiKeysVerified");
            }
            Ok(JobResult::JobCompleted { job_label, .. }) if job_label == "cleanup_temp_files" => {
                tracing::debug!("[cli] ignoring background cleanup_temp_files completion");
            }
            Ok(JobResult::Error { job_label, message }) => {
                return Err((job_label, message));
            }
            Ok(res) => return Ok(res),
            Err(e) => return Err(("runtime".into(), format!("Disconnected: {e}"))),
        }
    }
}

fn classify_transfer_result(result: JobResult) -> Option<Result<JobResult, (String, String)>> {
    match result {
        JobResult::Progress { label, fraction } => {
            tracing::info!("[progress] {}: {:.0}%", label, fraction * 100.0);
            None
        }
        result @ JobResult::TransferComplete(_) | result @ JobResult::TransferFailed { .. } => {
            Some(Ok(result))
        }
        JobResult::Error { job_label, message } => Some(Err((job_label, message))),
        JobResult::TimedOut { job_label, .. } => {
            Some(Err((job_label, "Transfer timed out".to_string())))
        }
        JobResult::Cancelled { .. } => Some(Err((
            "transfer_transactions".to_string(),
            "Transfer was cancelled".to_string(),
        ))),
        other => {
            tracing::debug!(
                result = ?other,
                "[cli] ignoring non-transfer result while transfer is active"
            );
            None
        }
    }
}

/// Wait on a result channel routed exclusively to the transfer job.
///
/// The shared runtime channel also carries cleanup watchdog events. A routed
/// ticket prevents those unrelated events from ending the CLI and cancelling
/// the asynchronous transfer pipeline.
fn wait_for_transfer_ticket(
    ticket: &crate::app::runtime::JobTicket,
) -> Result<JobResult, (String, String)> {
    loop {
        match ticket.recv_timeout(std::time::Duration::from_secs(780)) {
            Ok(result) => {
                if let Some(terminal) = classify_transfer_result(result) {
                    return terminal;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return Err((
                    "transfer_transactions".to_string(),
                    "Transfer result wait exceeded 780 seconds".to_string(),
                ));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err((
                    "runtime".to_string(),
                    "Transfer result channel disconnected".to_string(),
                ));
            }
        }
    }
}

struct OperationCompletion {
    disposition: OperationDisposition,
    artifact: Option<PathBuf>,
    message: String,
}

fn wait_for_operation_completion(
    job_rx: &Receiver<JobResult>,
    expected_job_label: &str,
) -> Result<OperationCompletion, (String, String)> {
    loop {
        match job_rx.recv() {
            Ok(JobResult::Progress { label, fraction }) => {
                tracing::info!("[progress] {}: {:.0}%", label, fraction * 100.0);
            }
            Ok(JobResult::JobCompleted {
                job_label,
                disposition,
                artifact,
                message,
            }) if job_label == expected_job_label => {
                return Ok(OperationCompletion {
                    disposition,
                    artifact,
                    message,
                });
            }
            Ok(JobResult::Error { job_label, message }) => {
                return Err((job_label, message));
            }
            Ok(JobResult::Cancelled { id }) => {
                return Err((expected_job_label.into(), format!("Job {id} was cancelled")));
            }
            Ok(JobResult::TimedOut { id, job_label }) => {
                return Err((job_label, format!("Job {id} exceeded its deadline")));
            }
            Ok(other) => tracing::debug!(
                expected_job_label,
                result = ?other,
                "[cli] ignoring intermediate result while awaiting operation completion"
            ),
            Err(error) => {
                return Err((
                    "runtime".into(),
                    format!("Disconnected while awaiting {expected_job_label}: {error}"),
                ));
            }
        }
    }
}

fn disposition_exit_code(disposition: OperationDisposition) -> i32 {
    match disposition {
        OperationDisposition::Succeeded => exit_code::SUCCESS,
        OperationDisposition::NoOp | OperationDisposition::Partial => exit_code::PARTIAL,
        OperationDisposition::Failed
        | OperationDisposition::Cancelled
        | OperationDisposition::TimedOut => exit_code::GENERAL,
    }
}

/// End-to-end self-test: render -> edit a real text span -> re-render, asserting
/// the edit changed the page (and only locally). Drives the same Job runtime
/// the GUI uses. Returns a process exit code (0 = PASS).
fn run_selftest(
    job_tx: &RuntimeClient,
    job_rx: &Receiver<JobResult>,
    input: Option<PathBuf>,
) -> anyhow::Result<i32> {
    use crate::app::runtime::{PythonJob, PythonJobResult};

    let input = input.unwrap_or_else(|| PathBuf::from("examples/sample.pdf"));
    if let Err(e) = validate_pdf_path(&input, "Self-test input") {
        anyhow::bail!("{e}");
    }
    println!("▶ Self-test on {}", input.display());

    // 1) Runtime liveness.
    let _ = job_tx.send_headless(Job::Ping);
    match wait_for_terminal_result(job_rx) {
        Ok(JobResult::Pong) => println!("  ✅ runtime ping"),
        _ => {
            eprintln!("  ❌ runtime did not respond to ping");
            return Ok(exit_code::GENERAL);
        }
    }

    // 2) Baseline render of page 0.
    let _ = job_tx.send_headless(Job::RenderPage {
        path: input.clone(),
        page: 0,
        dpi: 150.0,
        tag: "selftest_before".into(),
    });
    let before = match wait_for_terminal_result(job_rx) {
        Ok(JobResult::PageRendered { png_bytes, .. }) => {
            println!("  ✅ baseline render ({} bytes)", png_bytes.len());
            png_bytes
        }
        other => {
            eprintln!("  ❌ baseline render failed: {other:?}");
            return Ok(exit_code::GENERAL);
        }
    };

    // 3) Find a real text span on page 0 (so the edit has a target).
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = job_tx.send_headless(Job::Python(
        PythonJob::GetTextBlocks {
            pdf_path: input.to_string_lossy().to_string(),
            page_num: 0,
        },
        {
            // Bridge the oneshot reply onto a std channel via a helper thread.
            let (otx, orx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                if let Ok(r) = orx.blocking_recv() {
                    let _ = tx.send(r);
                }
            });
            otx
        },
    ));
    let blocks_json = match rx.recv() {
        Ok(PythonJobResult::Json(j)) => j,
        other => {
            eprintln!("  ❌ get_text_blocks failed: {other:?}");
            return Ok(exit_code::GENERAL);
        }
    };
    let blocks: serde_json::Value = serde_json::from_str(&blocks_json).unwrap_or_default();
    let first = blocks.as_array().and_then(|a| a.first());
    let (bbox, old_text) = match first {
        Some(b) => {
            let bb = b["bbox"].as_array().map(|a| {
                [
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                    a[3].as_f64().unwrap_or(0.0) as f32,
                ]
            });
            (bb, b["text"].as_str().unwrap_or("").to_string())
        }
        None => {
            eprintln!("  ❌ no text spans found on page 0; cannot self-test the edit path");
            return Ok(exit_code::GENERAL);
        }
    };
    let bbox = match bbox {
        Some(b) if b[0] < b[2] && b[1] < b[3] => b,
        _ => {
            eprintln!("  ❌ first span had an invalid bbox");
            return Ok(exit_code::GENERAL);
        }
    };
    println!("  ✅ found target span: {old_text:?} @ {bbox:?}");

    // 4) Apply an edit over that span.
    let out = std::path::PathBuf::from("output/selftest_edited.pdf");
    let _ = std::fs::create_dir_all("output");
    let _ = job_tx.send_headless(Job::ApplyChange {
        input: input.clone(),
        output: out.clone(),
        page: 0,
        bbox,
        new_text: "SELFTEST 12345".into(),
        old_text,
        description: "selftest edit".into(),
        deep_font_replication: false,
    });
    match wait_for_terminal_result(job_rx) {
        Ok(JobResult::ChangeApplied { .. }) => println!("  ✅ edit applied -> {}", out.display()),
        other => {
            eprintln!("  ❌ edit failed: {other:?}");
            return Ok(exit_code::GENERAL);
        }
    }

    // 5) Re-render the edited PDF and assert it differs from the baseline.
    let _ = job_tx.send_headless(Job::RenderPage {
        path: out.clone(),
        page: 0,
        dpi: 150.0,
        tag: "selftest_after".into(),
    });
    let after = match wait_for_terminal_result(job_rx) {
        Ok(JobResult::PageRendered { png_bytes, .. }) => png_bytes,
        other => {
            eprintln!("  ❌ re-render failed: {other:?}");
            return Ok(exit_code::GENERAL);
        }
    };

    if after == before {
        eprintln!("  ❌ edited render is identical to baseline - the edit did not land");
        return Ok(exit_code::GENERAL);
    }
    println!(
        "  ✅ edited render differs from baseline ({} vs {} bytes)",
        after.len(),
        before.len()
    );
    println!("✅ SELF-TEST PASSED - render, text-edit, and re-render all work end-to-end.");
    Ok(exit_code::SUCCESS)
}

/// Status of a single diagnostic check.
enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

fn print_status(status: &CheckStatus, name: &str, detail: &str) {
    let icon = match status {
        CheckStatus::Ok => "✅",
        CheckStatus::Warn => "⚠️ ",
        CheckStatus::Fail => "❌",
    };
    println!("  {icon}  {name:34}  {detail}");
}

/// Runs the `doctor` diagnostics command.
///
/// Reports configuration health grouped by requirement level, with explicit
/// setup guidance for anything missing. Returns a process exit code:
/// `SUCCESS` when ready, `CONFIG` when a required item is missing, or
/// `PARTIAL` when only optional/recommended items are absent.
fn run_doctor(
    config: &crate::app::config::AppConfig,
    job_tx: &RuntimeClient,
    job_rx: &Receiver<JobResult>,
) -> anyhow::Result<i32> {
    println!("══════════════════════════════════════════════════════════");
    println!("  Bank Statement Fidelity Editor - Doctor");
    println!("══════════════════════════════════════════════════════════");

    let mut missing_required: Vec<&'static str> = Vec::new();
    let mut missing_recommended: Vec<&'static str> = Vec::new();

    // ---- Environment variables, grouped by requirement -------------------
    println!("\n Environment variables");
    for spec in env_spec::ENV_VARS {
        let present = is_env_present(spec.name, config);
        let status = match (present, spec.requirement) {
            (true, _) => CheckStatus::Ok,
            (false, Requirement::Required) => CheckStatus::Fail,
            (false, Requirement::Recommended) => CheckStatus::Warn,
            (false, Requirement::Optional) => CheckStatus::Warn,
        };

        let detail = if present {
            spec.enables.to_string()
        } else {
            format!("[{}] {}", spec.requirement.label(), spec.enables)
        };
        print_status(&status, spec.name, &detail);

        if !present {
            match spec.requirement {
                Requirement::Required => missing_required.push(spec.name),
                Requirement::Recommended => missing_recommended.push(spec.name),
                Requirement::Optional => {}
            }
        }
    }

    // ---- Document AI auth method (only meaningful when configured) -------
    if let Some(da) = &config.document_ai {
        let auth = if !da.api_key.is_empty() {
            "API key (v1beta3) - primary"
        } else if !da.adc_path.is_empty() {
            "Application Default Credentials (gcloud)"
        } else if !da.service_account_path.is_empty() {
            "service-account JSON (v1)"
        } else {
            "no credential"
        };
        let status = if da.has_auth() {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        };
        print_status(&status, "Document AI auth", auth);
    }

    // ---- Filesystem checks ----------------------------------------------
    println!("\n Filesystem");
    let mut fs_ok = true;
    for (label, dir) in [
        ("logs/ writable", config.log_dir.as_path()),
        ("audit/ writable", std::path::Path::new("audit")),
        ("output/ writable", std::path::Path::new("output")),
    ] {
        let ok = std::fs::create_dir_all(dir).is_ok();
        fs_ok &= ok;
        let status = if ok {
            CheckStatus::Ok
        } else {
            CheckStatus::Fail
        };
        print_status(&status, label, &dir.display().to_string());
    }

    let template_dir = crate::app::paths::resolve_asset_path("bank_templates");
    let templates = std::fs::read_dir(&template_dir)
        .map(|d| d.filter_map(|e| e.ok()).count())
        .unwrap_or(0);
    print_status(
        if templates > 0 {
            &CheckStatus::Ok
        } else {
            &CheckStatus::Warn
        },
        "Bank templates",
        &format!("{templates} template(s) found"),
    );

    // ---- Runtime check ---------------------------------------------------
    println!("\n Runtime");
    let _ = job_tx.send_headless(Job::Ping);
    let runtime_ok = matches!(wait_for_terminal_result(job_rx), Ok(JobResult::Pong));
    print_status(
        if runtime_ok {
            &CheckStatus::Ok
        } else {
            &CheckStatus::Fail
        },
        "Worker responding",
        "Tokio + Python actor",
    );

    // ---- Summary & actionable guidance ----------------------------------
    println!("\n══════════════════════════════════════════════════════════");

    if !missing_required.is_empty() || !runtime_ok || !fs_ok {
        println!(" Doctor: ❌ Not ready - required items are missing.\n");
        for name in &missing_required {
            println!("{}\n", indent_block(&env_spec::guidance_for(name)));
        }
        if !runtime_ok {
            println!(
                "  • Runtime worker did not respond. Check logs in {}.",
                config.log_dir.display()
            );
        }
        if !fs_ok {
            println!("  • One or more required directories are not writable.");
        }
        return Ok(exit_code::CONFIG);
    }

    if !missing_recommended.is_empty() {
        println!(" Doctor: ⚠️  Usable, but some recommended features are off.\n");
        for name in &missing_recommended {
            if let Some(spec) = env_spec::lookup(name) {
                println!("  • {} -> enables: {}", spec.name, spec.enables);
            }
        }
        println!("\n Run with these set to unlock the full feature set.");
        return Ok(exit_code::PARTIAL);
    }

    println!(" Doctor: ✅ Ready for use. All systems go.");
    Ok(exit_code::SUCCESS)
}

/// Returns whether a given environment variable is effectively present,
/// preferring the parsed `AppConfig` where available (so we reflect the
/// values the app actually loaded rather than just raw env state).
fn is_env_present(name: &str, config: &crate::app::config::AppConfig) -> bool {
    match name {
        "DUAL_CORE_PASSPHRASE" => !config.passphrase.is_empty(),
        "PYMUPDF_PRO_KEY" => config.pymupdf_pro_key.is_some(),
        "GEMINI_API_KEY" => config.gemini_api_key.is_some(),
        "PDFREST_API_KEY" => config.pdfrest_api_key.is_some(),
        "OTEL_EXPORTER_OTLP_ENDPOINT" => config.otel_endpoint.is_some(),
        "DOCUMENT_AI_PROJECT_ID" | "DOCUMENT_AI_LOCATION" | "DOCUMENT_AI_PROCESSOR_ID" => {
            config.document_ai.is_some()
        }
        // For everything else, fall back to the raw environment.
        other => std::env::var(other).map(|v| !v.is_empty()).unwrap_or(false),
    }
}

/// Indents every line of a multi-line block by two spaces for display.
fn indent_block(text: &str) -> String {
    text.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, serde::Serialize)]
struct BatchExtractionFileResult {
    input: String,
    output: Option<String>,
    status: String,
    attempts: usize,
    row_count: usize,
    error: Option<String>,
}

fn collect_pdf_files(root: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
    fn visit(directory: &std::path::Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        let mut entries = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
            {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn run_extract_batch(
    input_dir: PathBuf,
    output_dir: PathBuf,
    max_concurrency: usize,
    retries: usize,
    config: std::sync::Arc<crate::app::config::AppConfig>,
) -> anyhow::Result<i32> {
    if !input_dir.is_dir() {
        anyhow::bail!("batch input is not a directory: {}", input_dir.display());
    }
    if !(1..=32).contains(&max_concurrency) {
        anyhow::bail!("max-concurrency must be between 1 and 32");
    }
    if retries > 5 {
        anyhow::bail!("retries must be between 0 and 5");
    }
    let files = collect_pdf_files(&input_dir)?;
    if files.is_empty() {
        anyhow::bail!("batch input contains no PDF files: {}", input_dir.display());
    }
    std::fs::create_dir_all(&output_dir)?;

    let worker_count = max_concurrency.min(files.len()).max(1);
    let queue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
        files,
    )));
    let results = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    std::thread::scope(|scope| {
        for worker_id in 0..worker_count {
            let queue = std::sync::Arc::clone(&queue);
            let results = std::sync::Arc::clone(&results);
            let config = std::sync::Arc::clone(&config);
            let input_root = input_dir.clone();
            let output_root = output_dir.clone();
            scope.spawn(move || {
                let audit_dir = output_root
                    .join(".batch-audit")
                    .join(format!("worker-{worker_id}"));
                let audit_log = match crate::app::audit::AuditLog::open(&audit_dir) {
                    Ok(log) => log,
                    Err(error) => {
                        tracing::error!("batch worker {worker_id} audit init failed: {error}");
                        return;
                    }
                };
                let (_runtime, job_tx, job_rx) =
                    crate::app::runtime::Runtime::start(audit_log, config);

                loop {
                    let input = queue.lock().ok().and_then(|mut queue| queue.pop_front());
                    let Some(input) = input else {
                        break;
                    };
                    let relative = input.strip_prefix(&input_root).unwrap_or(input.as_path());
                    let output = output_root.join(relative).with_extension("json");
                    let mut final_result = None;

                    for attempt in 1..=(retries + 1) {
                        let submitted = job_tx.send_headless(Job::ExtractTransactions {
                            path: input.clone(),
                            parser_mode: crate::app::config::DocumentParserMode::OfflineHeuristic,
                        });
                        if submitted.is_err() {
                            final_result = Some(BatchExtractionFileResult {
                                input: input.display().to_string(),
                                output: None,
                                status: "failed".into(),
                                attempts: attempt,
                                row_count: 0,
                                error: Some("runtime intake closed".into()),
                            });
                            break;
                        }

                        match wait_for_terminal_result(&job_rx) {
                            Ok(JobResult::TransactionsExtracted(transactions)) => {
                                let write_result = (|| -> anyhow::Result<()> {
                                    if let Some(parent) = output.parent() {
                                        std::fs::create_dir_all(parent)?;
                                    }
                                    let json = serde_json::to_vec_pretty(&transactions)?;
                                    std::fs::write(&output, json)?;
                                    Ok(())
                                })();
                                final_result = Some(match write_result {
                                    Ok(()) => BatchExtractionFileResult {
                                        input: input.display().to_string(),
                                        output: Some(output.display().to_string()),
                                        status: "success".into(),
                                        attempts: attempt,
                                        row_count: transactions.len(),
                                        error: None,
                                    },
                                    Err(error) => BatchExtractionFileResult {
                                        input: input.display().to_string(),
                                        output: None,
                                        status: "failed".into(),
                                        attempts: attempt,
                                        row_count: 0,
                                        error: Some(format!("output write failed: {error}")),
                                    },
                                });
                                break;
                            }
                            Ok(other) => {
                                final_result = Some(BatchExtractionFileResult {
                                    input: input.display().to_string(),
                                    output: None,
                                    status: "failed".into(),
                                    attempts: attempt,
                                    row_count: 0,
                                    error: Some(format!("unexpected terminal result: {other:?}")),
                                });
                            }
                            Err((label, error)) => {
                                final_result = Some(BatchExtractionFileResult {
                                    input: input.display().to_string(),
                                    output: None,
                                    status: "failed".into(),
                                    attempts: attempt,
                                    row_count: 0,
                                    error: Some(format!("{label}: {error}")),
                                });
                            }
                        }
                    }

                    if let (Some(result), Ok(mut all_results)) = (final_result, results.lock()) {
                        all_results.push(result);
                    }
                }
            });
        }
    });

    let mut results = results
        .lock()
        .map_err(|_| anyhow::anyhow!("batch result lock poisoned"))?
        .clone();
    results.sort_by(|left, right| left.input.cmp(&right.input));
    let manifest_path = output_dir.join("batch_manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&results)?)?;

    let succeeded = results
        .iter()
        .filter(|result| result.status == "success")
        .count();
    let failed = results.len().saturating_sub(succeeded);
    println!(
        "Batch extraction complete: {succeeded} succeeded, {failed} failed. Manifest: {}",
        manifest_path.display()
    );
    Ok(if failed == 0 {
        exit_code::SUCCESS
    } else {
        exit_code::PARTIAL
    })
}

pub fn run(
    cli: Cli,
    job_tx: RuntimeClient,
    job_rx: Receiver<JobResult>,
    config: std::sync::Arc<crate::app::config::AppConfig>,
) -> i32 {
    match run_inner(cli, job_tx, job_rx, config) {
        Ok(code) => code,
        Err(e) => {
            tracing::error!("CLI Error: {e}");
            1
        }
    }
}

pub fn run_inner(
    cli: Cli,
    job_tx: RuntimeClient,
    job_rx: Receiver<JobResult>,
    config: std::sync::Arc<crate::app::config::AppConfig>,
) -> anyhow::Result<i32> {
    // Pre-flight: input file existence checks for subcommands that take an input.
    let preflight = match &cli.command {
        Commands::Text { input, .. }
        | Commands::Balance { input, .. }
        | Commands::Extract { input, .. }
        | Commands::Render { input, .. }
        | Commands::FontComplete { input, .. }
        | Commands::AnalyzeFonts { input, .. }
        | Commands::AutoBalance { input, .. }
        | Commands::AiFixVisual { input, .. }
        | Commands::AdjustDates { input, .. } => Some(input.clone()),
        Commands::TypstReconstruct { input, .. } => Some(input.clone()),
        Commands::Verify {
            original, edited, ..
        } => {
            if !original.exists() {
                eprintln!("❌ Original PDF not found: {}", original.display());
                return Ok(exit_code::NOT_FOUND);
            }
            if !edited.exists() {
                eprintln!("❌ Edited PDF not found: {}", edited.display());
                return Ok(exit_code::NOT_FOUND);
            }
            None
        }
        Commands::ExportHistory { from_log, .. } => {
            if !from_log.exists() {
                eprintln!("❌ Audit log not found: {}", from_log.display());
                return Ok(exit_code::NOT_FOUND);
            }
            None
        }
        Commands::TransferTransactions {
            source_pdf,
            target_pdf,
            ..
        } => {
            if !source_pdf.exists() {
                eprintln!("❌ Source PDF not found: {}", source_pdf.display());
                return Ok(exit_code::NOT_FOUND);
            }
            if !target_pdf.exists() {
                eprintln!("❌ Target PDF not found: {}", target_pdf.display());
                return Ok(exit_code::NOT_FOUND);
            }
            None
        }
        Commands::RunTransferTests { statements, .. } => {
            for stmt in statements {
                if !stmt.exists() {
                    eprintln!("❌ Statement PDF not found: {}", stmt.display());
                    return Ok(exit_code::NOT_FOUND);
                }
            }
            None
        }
        _ => None,
    };
    if let Some(path) = preflight {
        if !path.exists() {
            eprintln!("❌ Input file not found: {}", path.display());
            return Ok(exit_code::NOT_FOUND);
        }
        if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            != Some("pdf".into())
        {
            eprintln!("❌ Input must be a .pdf file: {}", path.display());
            return Ok(exit_code::VALIDATION);
        }
    }

    match cli.command {
        Commands::TypstReconstruct { input, output } => {
            // Fail-closed path: runtime rejects edit-in-place Typst rebuilds.
            // Surface that clearly for CLI/MCP callers (UFO included).
            if !input.exists() {
                eprintln!("❌ Input PDF not found: {}", input.display());
                return Ok(exit_code::NOT_FOUND);
            }
            tracing::info!(
                "Typst reconstruct requested (input={}, output={})",
                input.display(),
                output.display()
            );
            let _ = job_tx.send_headless(Job::TypstReconstruct {
                input: input.clone(),
                output: output.clone(),
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::ReconstructComplete { output_path }) => {
                    tracing::info!("Reconstruction successful! Saved to {:?}", output_path);
                    Ok(exit_code::SUCCESS)
                }
                Ok(JobResult::Error { job_label, message }) => {
                    eprintln!("❌ Typst reconstruct blocked ({job_label}): {message}");
                    Ok(exit_code::GENERAL)
                }
                Ok(other) => {
                    tracing::error!("Unexpected terminal result: {:?}", other);
                    Ok(exit_code::GENERAL)
                }
                Err((label, err)) => {
                    eprintln!("❌ Job '{label}' failed: {err}");
                    Ok(exit_code::GENERAL)
                }
            }
        }
        Commands::McpRenderPage { input, page } => {
            let _ = job_tx.send_headless(Job::McpRenderPage { input, page });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::McpRenderComplete { base64_png }) => {
                    println!("{}", base64_png);
                    Ok(exit_code::SUCCESS)
                }
                Ok(other) => {
                    tracing::error!("Unexpected terminal result: {:?}", other);
                    Ok(exit_code::GENERAL)
                }
                Err((label, err)) => {
                    tracing::error!("Job '{}' failed: {}", label, err);
                    Ok(exit_code::GENERAL)
                }
            }
        }
        Commands::Mcp => {
            crate::ai::mcp::McpServer::start();
            Ok(exit_code::SUCCESS)
        }
        Commands::Ufo { request } => {
            let mut attempts = 0;
            loop {
                match crate::ai::ufo::UfoClient::dispatch_task(&request, None::<fn(String)>) {
                    Ok(result) => {
                        println!(
                            "UFO Task Result:\n{}",
                            serde_json::to_string_pretty(&result).unwrap_or_default()
                        );
                        if result.status == "error" {
                            let msg = result.error_message.as_deref().unwrap_or("unknown");
                            if attempts == 0 {
                                tracing::warn!("UFO Error: {}. Retrying once...", msg);
                                attempts += 1;
                                continue;
                            } else {
                                tracing::error!("UFO returned error status: {}", msg);
                                return Ok(exit_code::GENERAL);
                            }
                        } else {
                            if let Some(out) = result.output.as_ref() {
                                if out.contains(".pdf") {
                                    tracing::info!("UFO successfully acquired a PDF! Handoff ready: {}", out);
                                    let re = regex::Regex::new(r"(?i)[a-z]:[\\/][^<>\x22\|\?\*]+\.pdf").unwrap();
                                    if let Some(caps) = re.captures(out) {
                                        let pdf_path = std::path::PathBuf::from(caps.get(0).unwrap().as_str());
                                        tracing::info!("Auto-dispatching Reducto Parse Job for: {:?}", pdf_path);
                                        let _ = job_tx.send_headless(crate::app::runtime::Job::ExtractTransactions {
                                            path: pdf_path,
                                            parser_mode: crate::app::config::DocumentParserMode::LlamaParse,
                                        });
                                        match wait_for_terminal_result(&job_rx) {
                                            Ok(crate::app::runtime::JobResult::TransactionsExtracted(transactions)) => {
                                                println!("--- E2E SUCCESS: PARSED TRANSACTIONS ---");
                                                for t in transactions {
                                                    println!("{:?}", t);
                                                }
                                                return Ok(exit_code::SUCCESS);
                                            }
                                            Ok(other) => {
                                                tracing::error!("Unexpected terminal result: {:?}", other);
                                                return Ok(exit_code::GENERAL);
                                            }
                                            Err((label, err)) => {
                                                tracing::error!("Job '{}' failed: {}", label, err);
                                                return Ok(exit_code::GENERAL);
                                            }
                                        }
                                    }
                                }
                            }
                            return Ok(exit_code::SUCCESS);
                        }
                    }
                    Err(e) => {
                        if attempts == 0 {
                            tracing::warn!("UFO dispatch failed: {}. Retrying once...", e);
                            attempts += 1;
                            continue;
                        } else {
                            tracing::error!("UFO dispatch failed: {}", e);
                            eprintln!("UFO dispatch failed: {e}");
                            return Ok(exit_code::GENERAL);
                        }
                    }
                }
            }
        }
        Commands::Chat {
            instruction,
            target_pdf,
        } => {
            println!("🤖 Parsing instruction: \"{}\"", instruction);
            let cmd = crate::app::nlp_router::parse(&instruction);
            println!("🧠 Parsed Intent: {}", cmd.describe());

            let path = target_pdf.unwrap_or_else(|| PathBuf::from("statement.pdf"));

            // Depending on the command, dispatch a job. For AiEdit, we dispatch AiCommand.
            let job = match cmd {
                crate::app::nlp_router::NlpCommand::AiEdit {
                    instruction,
                    provider: _,
                } => Job::AiCommand {
                    prompt: instruction,
                    path,
                },
                // For simplicity, we just route everything complex to AiCommand here, or handle directly.
                // But nlp_router directly returns a mapped NlpCommand.
                // However, the runtime currently handles AiEdit via NlpCommand natively inside AiCommand dispatcher.
                // Wait, runtime matches NlpCommand inside process_job_inner only if we send an `AiCommand` that parses it again?
                // Actually, `Job::AiCommand` in runtime calls `nlp_router::parse` itself!
                _ => Job::AiCommand {
                    prompt: instruction.clone(),
                    path,
                },
            };

            let _ = job_tx.send_headless(job);
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::NaturalLanguageEditReady(txs)) => {
                    println!(
                        "✅ Local LLM modified {} transactions successfully.",
                        txs.len()
                    );
                    Ok(exit_code::SUCCESS)
                }
                Ok(other) => {
                    println!("✅ Command completed with result: {:?}", other);
                    Ok(exit_code::SUCCESS)
                }
                Err((lbl, msg)) => {
                    tracing::error!("❌ [{}] {}", lbl, msg);
                    Ok(exit_code::GENERAL)
                }
            }
        }
        Commands::Gui => {
            // [Phase 0.1] Environment & Memory Assertions
            if let Err(e) = crate::app::preflight::verify_environment(&config) {
                tracing::warn!(
                    "Pre-flight verification warning/error: {}. Proceeding with caution.",
                    e
                );
                if matches!(
                    e,
                    crate::app::preflight::PreflightError::HeadlessEnvironment
                ) {
                    tracing::error!("Headless environment detected. Auto-healing: falling back to Headless Server.");
                    if let Err(serve_err) =
                        crate::app::server::run_server(job_tx, job_rx, config.clone())
                    {
                        tracing::error!("Fallback server also failed: {}", serve_err);
                        return Ok(exit_code::GENERAL);
                    }
                    return Ok(exit_code::SUCCESS);
                }
            }

            // [Phase 0.2] GUI Pre-Flight & Fallback
            if let Err(e) = crate::app::gui::run_gui(job_tx, job_rx, config.clone()) {
                tracing::error!("Failed to launch GUI (eframe error): {}.", e);
                tracing::error!("Auto-healing: falling back to Headless Server.");

                // We must restart the runtime because `job_rx` was consumed by the failed GUI
                tracing::info!("Restarting worker runtime for fallback server...");
                let audit_log = match crate::app::audit::AuditLog::open("audit") {
                    Ok(log) => log,
                    Err(e) => {
                        tracing::error!("[AUDIT] Failed to open audit log for fallback: {}", e);
                        return Ok(exit_code::IO);
                    }
                };
                let (_new_rt, new_tx, new_rx) =
                    crate::app::runtime::Runtime::start(audit_log, config.clone());

                if let Err(serve_err) =
                    crate::app::server::run_server(new_tx, new_rx, config.clone())
                {
                    tracing::error!("Fallback server also failed: {}", serve_err);
                    return Ok(exit_code::GENERAL);
                }
            }
            Ok(exit_code::SUCCESS)
        }
        Commands::Serve => {
            if let Err(e) = crate::app::server::run_server(job_tx, job_rx, config.clone()) {
                tracing::error!("Headless server exited with error: {}", e);
                return Ok(exit_code::GENERAL);
            }
            Ok(exit_code::SUCCESS)
        }
        Commands::Text {
            input,
            output,
            old,
            new,
            page,
            bbox,
        } => {
            // Validate input file first
            if let Err(e) = validate_pdf_path(&input, "Input PDF") {
                anyhow::bail!("{e}");
            }

            // Parse bbox with proper error handling
            let coords = match parse_bbox(&bbox) {
                Ok(c) => c,
                Err(e) => {
                    anyhow::bail!("[cli_text] Invalid bbox: {e}");
                }
            };

            let _ = job_tx.send_headless(Job::ApplyChange {
                input,
                output,
                page: page.unwrap_or(0),
                bbox: coords,
                new_text: new,
                old_text: old,
                description: "CLI manual edit".into(),
                deep_font_replication: false,
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::ChangeApplied { .. }) => {
                    println!("✅ Change applied successfully.");
                    Ok(0)
                }
                Err((lbl, msg)) => {
                    tracing::error!("❌ [{}] {}", lbl, msg);
                    Ok(1)
                }
                _ => {
                    tracing::error!("Unexpected result from runtime");
                    Ok(1)
                }
            }
        }
        Commands::Balance {
            input,
            output,
            auto_approve,
        } => {
            let _ = job_tx.send_headless(Job::BalanceStatement {
                path: input.clone(),
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::BalanceProposed { imbalance, changes }) => {
                    if changes.is_empty() {
                        println!(
                            "✅ Statement is already perfectly balanced (imbalance: ${imbalance})."
                        );
                        return Ok(0);
                    }

                    println!("Imbalance detected: ${imbalance}");
                    println!("Proposed Adjustments:");
                    for (i, change) in changes.iter().enumerate() {
                        println!(
                            "  {}) P{}: {} -> {} (Confidence: {:.0}%)",
                            i + 1,
                            change.page,
                            change.old_text,
                            change.new_text,
                            change.confidence * 100.0
                        );
                        println!("      Reason: {}", change.reason);
                    }

                    if auto_approve {
                        let expected_changes = changes.len();
                        println!(
                            "\n--auto-approve flag is set. Applying all {} changes...",
                            expected_changes
                        );
                        let _ = job_tx.send_headless(Job::ApplyProposedChanges {
                            input,
                            output: output.clone(),
                            changes,
                        });

                        match wait_for_terminal_result(&job_rx) {
                            Ok(JobResult::ProposedChangesApplied {
                                changes_applied,
                                failures,
                            }) => {
                                if !failures.is_empty() {
                                    eprintln!("❌ {} change(s) failed:", failures.len());
                                    for (i, failure) in failures.iter().enumerate() {
                                        eprintln!("   {}. {}", i + 1, failure);
                                    }
                                    return Ok(1);
                                }
                                if changes_applied != expected_changes {
                                    tracing::error!(
                                        "❌ Exact apply count mismatch: requested {}, applied {}",
                                        expected_changes,
                                        changes_applied
                                    );
                                    return Ok(1);
                                }
                                let output_is_durable = std::fs::metadata(&output)
                                    .map(|metadata| metadata.is_file() && metadata.len() > 0)
                                    .unwrap_or(false);
                                if !output_is_durable {
                                    tracing::error!(
                                        "❌ Runtime reported success but the requested output artifact is missing or empty: {:?}",
                                        output
                                    );
                                    return Ok(1);
                                }
                                println!("✅ Successfully applied {changes_applied} changes.");
                                println!("Output saved to: {output:?}");
                                Ok(0)
                            }
                            Err((lbl, msg)) => {
                                tracing::error!("❌ [{}] {}", lbl, msg);
                                Ok(1)
                            }
                            _ => {
                                tracing::error!("Unexpected result from runtime");
                                Ok(1)
                            }
                        }
                    } else {
                        println!("\nRun with --auto-approve to apply these changes.");
                        Ok(0)
                    }
                }
                Err((lbl, msg)) => {
                    tracing::error!("❌ [{}] {}", lbl, msg);
                    Ok(1)
                }
                _ => {
                    tracing::error!("Unexpected result from runtime");
                    Ok(1)
                }
            }
        }
        Commands::ExtractBatch {
            input_dir,
            output_dir,
            max_concurrency,
            retries,
        } => run_extract_batch(input_dir, output_dir, max_concurrency, retries, config),
        Commands::Extract { input, output } => {
            let _ = job_tx.send_headless(Job::LoadDocument {
                path: input.clone(),
                three_page_mode: false,
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::DocumentLoaded { .. }) => {
                    let _ = job_tx.send_headless(Job::ExtractTransactions {
                        path: input,
                        parser_mode: crate::app::config::DocumentParserMode::OfflineHeuristic,
                    });
                    match wait_for_terminal_result(&job_rx) {
                        Ok(JobResult::TransactionsExtracted(transactions)) => {
                            let json = match serde_json::to_string_pretty(&transactions) {
                                Ok(j) => j,
                                Err(e) => {
                                    tracing::error!("❌ Failed to serialize: {e}");
                                    return Ok(1);
                                }
                            };
                            if std::fs::write(&output, json).is_ok() {
                                println!("✅ Data extraction successful. Saved to: {output:?}");
                                Ok(0)
                            } else {
                                tracing::error!("❌ Failed to write output file");
                                Ok(1)
                            }
                        }
                        Err((lbl, msg)) => {
                            tracing::error!("❌ [{}] {}", lbl, msg);
                            Ok(1)
                        }
                        _ => {
                            tracing::error!("Unexpected result from runtime");
                            Ok(1)
                        }
                    }
                }
                Err((lbl, msg)) => {
                    tracing::error!("❌ [{}] {}", lbl, msg);
                    Ok(1)
                }
                _ => {
                    tracing::error!("Unexpected result from runtime");
                    Ok(1)
                }
            }
        }
        Commands::Verify {
            original,
            edited,
            output_dir,
            use_pdfrest,
        } => {
            // Seed exact old/new target identities from the durable change history.
            // Without history, verification remains a full-document unchanged control.
            let intended_edits: Vec<crate::engine::verification::VerificationIntent> =
                match ChangeHistory::load_from_file(std::path::Path::new("audit/history.json")) {
                    Ok(history) => history
                        .get_history()
                        .iter()
                        .map(|record| crate::engine::verification::VerificationIntent {
                            page: record.page,
                            bbox: record.bbox,
                            old_text: record.old_text.clone(),
                            new_text: record.new_text.clone(),
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                };
            if !intended_edits.is_empty() {
                println!(
                    "Seeded {} exact intended edit(s) from audit/history.json",
                    intended_edits.len()
                );
            }
            let _ = job_tx.send_headless(Job::Verify {
                original,
                edited,
                output_dir: output_dir.clone(),
                intended_edits,
                use_pdfrest,
                pdfrest_key: config.pdfrest_api_key.clone(),
                auto_match_dpi: config.auto_match_dpi,
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::VerificationReport(report)) => {
                    let json_path = output_dir.join("verification_report.json");
                    let evidence_path = output_dir.join("verification_evidence.json");
                    if !json_path.is_file() || !evidence_path.is_file() {
                        tracing::error!(
                            "❌ Verifier evidence persistence incomplete: report={}, evidence={}",
                            json_path.display(),
                            evidence_path.display()
                        );
                        return Ok(exit_code::IO);
                    }
                    println!("{}", report.message);
                    println!("Report saved to: {json_path:?}");
                    println!("Evidence saved to: {evidence_path:?}");
                    if report.mandatory_local_pass() {
                        Ok(exit_code::SUCCESS)
                    } else {
                        tracing::error!("❌ Verification failed one or more mandatory local gates");
                        Ok(exit_code::VALIDATION)
                    }
                }
                Err((lbl, msg)) => {
                    tracing::error!("❌ [{}] {}", lbl, msg);
                    Ok(1)
                }
                _ => {
                    tracing::error!("Unexpected result from runtime");
                    Ok(1)
                }
            }
        }
        Commands::Render {
            input,
            output_dir,
            page,
            dpi,
        } => {
            // Capture the source stem before `input` is moved into the job, so
            // the output filename can include it (Improvement #5).
            let stem = input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("page")
                .to_string();
            let _ = job_tx.send_headless(Job::RenderPage {
                path: input,
                page,
                dpi,
                tag: "cli".into(),
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::PageRendered { png_bytes, .. }) => {
                    // Improvement #5: include the source PDF stem so batch
                    // renders of different files don't overwrite one another
                    // (previously every render produced `page_N_DPIdpi.png`).
                    let filename = format!("{}_page_{}_{}dpi.png", stem, page + 1, dpi as u32);
                    let path = output_dir.join(filename);
                    let _ = std::fs::create_dir_all(&output_dir);
                    if std::fs::write(&path, png_bytes).is_ok() {
                        println!("✅ Rendered to: {path:?}");
                        Ok(0)
                    } else {
                        tracing::error!("❌ Failed to write output file");
                        Ok(1)
                    }
                }
                Err((lbl, msg)) => {
                    tracing::error!("❌ [{}] {}", lbl, msg);
                    Ok(1)
                }
                _ => {
                    tracing::error!("Unexpected result from runtime");
                    Ok(1)
                }
            }
        }
        Commands::FontComplete { input, font } => {
            let _ = job_tx.send_headless(Job::CompleteFont {
                path: input,
                font_name: font,
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::FontCompleted(json)) => {
                    println!("{json}");
                    Ok(0)
                }
                Err((lbl, msg)) => {
                    tracing::error!("❌ [{}] {}", lbl, msg);
                    Ok(1)
                }
                _ => {
                    tracing::error!("Unexpected result from runtime");
                    Ok(1)
                }
            }
        }
        Commands::ExportHistory { from_log, output } => {
            match AuditLogParser::parse_file(&from_log) {
                Ok(records) => {
                    let mut history = ChangeHistory::new();
                    for rec in records {
                        history.push_record(rec);
                    }
                    if std::fs::write(&output, history.to_json_pretty_string()).is_ok() {
                        println!("✅ Reconstructed history exported to: {output:?}");
                        Ok(0)
                    } else {
                        tracing::error!("❌ Failed to write output file");
                        Ok(1)
                    }
                }
                Err(e) => {
                    tracing::error!("❌ Failed to parse audit log: {}", e);
                    Ok(1)
                }
            }
        }
        Commands::Ping => {
            let _ = job_tx.send_headless(Job::Ping);
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::Pong) => {
                    println!("pong");
                    Ok(0)
                }
                _ => Ok(1),
            }
        }
        Commands::Selftest { input } => run_selftest(&job_tx, &job_rx, input),
        Commands::Doctor => run_doctor(&config, &job_tx, &job_rx),
        Commands::VerifyApiKeys { json } => {
            // Run verification on a fresh tokio runtime
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("❌ failed to start tokio runtime: {e}");
                    return Ok(1);
                }
            };
            let cfg = config.clone();
            Ok(rt.block_on(async move {
                let report = crate::app::api_verification::verify_all_api_keys(&cfg, json).await;
                report.exit_code()
            }))
        }
        Commands::DocaiTrain {
            display_name,
            min_labelled,
            set_default,
            report_only,
        } => {
            // The training calls are async, so run them on a fresh single-thread
            // tokio runtime here (we deliberately don't reuse the worker
            // runtime to keep the CLI flow self-contained).
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("❌ failed to start tokio runtime: {e}");
                    return Ok(1);
                }
            };
            let cfg = config.clone();
            rt.block_on(async move {
                let client = match crate::ai::document_ai::DocumentAiClient::from_app_config(&cfg) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("❌ Document AI not configured: {e}");
                        return Ok(1);
                    }
                };
                println!("Polling dataset...");
                let (labeled, total) = match client.count_labeled_documents().await {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("❌ failed to list dataset: {e}");
                        return Ok(1);
                    }
                };
                println!("  Dataset: {labeled} / {total} labelled");
                if report_only {
                    return Ok(0);
                }
                if labeled < min_labelled {
                    eprintln!(
                        "⚠️ only {labeled} labelled doc(s); need ≥{min_labelled}. Label more in the Console."
                    );
                    return Ok(1);
                }
                let name = display_name.unwrap_or_else(|| {
                    format!("au-bank-{}", chrono::Utc::now().format("%Y%m%d-%H%M"))
                });
                println!("Starting training: {name}");
                let op = match client.start_training(&name).await {
                    Ok(o) => o,
                    Err(e) => {
                        eprintln!("❌ training kickoff failed: {e}");
                        return Ok(1);
                    }
                };
                println!("Operation: {op}");
                println!("Polling (this typically takes 1-6 hours)...");
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    match client.poll_operation(&op).await {
                        Ok((true, None)) => {
                            println!("✅ Training succeeded");
                            break;
                        }
                        Ok((true, Some(err))) => {
                            eprintln!("❌ Training failed: {err}");
                            return Ok(1);
                        }
                        Ok((false, _)) => {
                            print!(".");
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                        }
                        Err(e) => {
                            eprintln!("⚠️ poll error (will retry): {e}");
                        }
                    }
                }
                if set_default {
                    // The version ID is the last path segment of the operation
                    // metadata; we don't have it without another GET, so we ask
                    // the user to set it themselves. Surface a clear message.
                    println!("ℹ️ --set-default requested. Inspect the operation response for the new version ID, then set it in the Console (Manage versions -> Set default).");
                }
                Ok(0)
            })
        }
        Commands::FontcacheInit { force, dir } => {
            let cache_dir = dir.unwrap_or_else(crate::app::fontcache::default_cache_dir);
            println!("─────────────────────────────────────────");
            println!(" Font cache bootstrap (Stage 12 / Item #1)");
            println!("─────────────────────────────────────────");
            println!("Cache dir: {}", cache_dir.display());
            if force {
                println!("Mode: --force (re-downloading all fonts)");
            }
            match crate::app::fontcache::bootstrap(&cache_dir, force) {
                Ok(report) => {
                    report.print();
                    if report.failed.is_empty() {
                        println!();
                        println!(
                            "✅ Font cache ready. Stage 11 cascade Tier 2/3 will use these donors."
                        );
                        Ok(0)
                    } else {
                        println!();
                        println!("⚠️ Some downloads failed. The cache is usable but coverage is partial.");
                        Ok(2)
                    }
                }
                Err(e) => {
                    eprintln!("❌ bootstrap failed: {e}");
                    Ok(1)
                }
            }
        }
        Commands::AnalyzeFonts { input } => {
            let _ = job_tx.send_headless(Job::AnalyzeFonts { path: input });
            loop {
                match job_rx.recv() {
                    Ok(JobResult::FontAnalysisReady(report)) => {
                        println!("✅ Font Analysis Ready:\n{}", report.one_line_summary());
                        return Ok(0);
                    }
                    Ok(JobResult::Error { job_label, message }) => {
                        eprintln!("❌ [{job_label}] {message}");
                        return Ok(1);
                    }
                    Err(_) => return Ok(1),
                    _ => {}
                }
            }
        }
        Commands::AutoBalance { input, output } => {
            let _ = job_tx.send_headless(Job::BalanceAndApplyAll {
                input,
                output: output.clone(),
                auto_apply: true,
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::ProposedChangesApplied {
                    changes_applied,
                    failures,
                }) => {
                    println!("✅ Applied {changes_applied} changes to {output:?}");
                    if !failures.is_empty() {
                        eprintln!("⚠️ {} failure(s)", failures.len());
                        return Ok(1);
                    }
                    Ok(0)
                }
                Err((lbl, msg)) => {
                    eprintln!("❌ [{lbl}] {msg}");
                    Ok(1)
                }
                _ => Ok(1),
            }
        }
        Commands::AiFixVisual { input: _, page: _ } => {
            eprintln!("AI visual layout repair is not available in v1; no document was changed. Use the deterministic edit and verification workflow.");
            Ok(2)
        }
        Commands::TransferTransactions {
            source_pdf,
            target_pdf,
            output,
        } => {
            let ticket = match job_tx.submit_headless(Job::TransferTransactions {
                source_pdf,
                target_pdf,
                output_pdf: output,
            }) {
                Ok(ticket) => ticket,
                Err(error) => {
                    eprintln!("❌ Could not submit transfer: {error}");
                    return Ok(1);
                }
            };
            match wait_for_transfer_ticket(&ticket) {
                Ok(JobResult::TransferComplete(result)) => {
                    println!(
                        "✅ Transfer complete. Target has {} transactions.",
                        result.target_tx_count
                    );
                    Ok(0)
                }
                Ok(JobResult::TransferFailed { stage, message }) => {
                    eprintln!("❌ Transfer failed at {stage}: {message}");
                    Ok(1)
                }
                Err((lbl, msg)) => {
                    eprintln!("❌ [{lbl}] {msg}");
                    Ok(1)
                }
                _ => Ok(1),
            }
        }
        Commands::AdjustDates {
            input,
            output,
            mode,
        } => {
            let parsed_mode = if mode == "remap" {
                crate::engine::date_adjust::DateAdjustMode::RemapPeriod {
                    from_start: chrono::NaiveDate::from_ymd_opt(2025, 1, 1)
                        .unwrap_or(chrono::NaiveDate::MIN),
                    to_start: chrono::NaiveDate::from_ymd_opt(2025, 2, 1)
                        .unwrap_or(chrono::NaiveDate::MIN),
                }
            } else {
                crate::engine::date_adjust::DateAdjustMode::ShiftDays(30)
            };
            if let Err(error) = job_tx.send_headless(Job::AdjustDatePeriods {
                input,
                output: output.clone(),
                mode: parsed_mode,
            }) {
                eprintln!("Could not submit date adjustment: {error}");
                return Ok(exit_code::GENERAL);
            }
            match wait_for_operation_completion(&job_rx, "adjust_dates") {
                Ok(completion) => {
                    let exit = disposition_exit_code(completion.disposition);
                    if completion.disposition == OperationDisposition::Succeeded {
                        let artifact_is_exact = completion.artifact.as_ref() == Some(&output);
                        let artifact_is_durable = std::fs::metadata(&output)
                            .map(|metadata| metadata.is_file() && metadata.len() > 0)
                            .unwrap_or(false);
                        if !artifact_is_exact || !artifact_is_durable {
                            eprintln!(
                                "Date adjustment reported success without the exact durable requested artifact: {}",
                                output.display()
                            );
                            return Ok(exit_code::GENERAL);
                        }
                        println!("{}", completion.message);
                    } else {
                        eprintln!(
                            "Date adjustment ended as {:?}: {}",
                            completion.disposition, completion.message
                        );
                    }
                    Ok(exit)
                }
                Err((label, message)) => {
                    eprintln!("[{label}] {message}");
                    Ok(exit_code::GENERAL)
                }
            }
        }
        Commands::RunTransferTests {
            statements,
            max_iterations,
        } => {
            let _ = job_tx.send_headless(Job::RunTransferTests {
                statements,
                max_iterations,
            });
            match wait_for_terminal_result(&job_rx) {
                Ok(JobResult::TransferTestsComplete(report)) => {
                    println!("✅ Transfer Tests Complete:\n{report:?}");
                    Ok(0)
                }
                Err((lbl, msg)) => {
                    eprintln!("❌ [{lbl}] {msg}");
                    Ok(1)
                }
                _ => Ok(1),
            }
        }
    }
}

#[cfg(test)]
mod batch_extraction_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn transfer_wait_ignores_premature_generic_job_completion() {
        let premature = JobResult::JobCompleted {
            job_label: "transfer_transactions".to_string(),
            disposition: OperationDisposition::Succeeded,
            artifact: None,
            message: "runtime intake returned before asynchronous transfer".to_string(),
        };
        assert!(classify_transfer_result(premature).is_none());

        let result = classify_transfer_result(JobResult::TransferFailed {
            stage: "AnalyzeSource".to_string(),
            message: "bounded test result".to_string(),
        })
        .expect("terminal classification")
        .expect("transfer result");
        assert!(matches!(
            result,
            JobResult::TransferFailed { ref stage, .. } if stage == "AnalyzeSource"
        ));
    }

    #[test]
    fn recursive_batch_discovery_is_complete_and_deterministic() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let nested = root.path().join("nested");
        std::fs::create_dir_all(&nested)?;
        std::fs::write(root.path().join("b.pdf"), b"pdf-b")?;
        std::fs::write(root.path().join("a.PDF"), b"pdf-a")?;
        std::fs::write(nested.join("c.pdf"), b"pdf-c")?;
        std::fs::write(nested.join("ignore.txt"), b"not a pdf")?;

        let discovered = collect_pdf_files(root.path())?;
        assert_eq!(discovered.len(), 3);
        assert!(discovered.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(discovered.iter().all(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        }));
        Ok(())
    }
}
