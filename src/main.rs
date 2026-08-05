//! Bank Statement Fidelity Editor v1.0.0
//! High-fidelity text & number editing with automatic balance reconciliation + smart targeted selection

use clap::Parser;
use dual_core_pdf_pipeline::error::exit_code;
use dual_core_pdf_pipeline::{app, security};
use std::sync::Arc;

fn main() {
    // Clap handles `--help` and `--version` by printing and exiting here. Keep
    // this before environment, telemetry, configuration, audit, and runtime
    // initialization so informational startup contracts are side-effect free.
    let cli = app::cli::Cli::parse();

    // If running in a Mac app bundle, resolve relative to Resources
    if let Ok(exe_path) = std::env::current_exe() {
        if exe_path.to_string_lossy().contains("Contents/MacOS") {
            if let Some(resources_dir) = exe_path
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("Resources"))
            {
                let _ = dotenvy::from_path_override(resources_dir.join(".env"));
            }
        }
    }

    // Fallback for terminal/dev usage — override_() ensures .env values
    // always take precedence over stale system/user environment variables.
    let _ = dotenvy::dotenv_override();

    // Phase 3 - Stage 10: Sentry Integration for Telemetry
    let _sentry = sentry::init((
        std::env::var("SENTRY_DSN").unwrap_or_default(),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            traces_sample_rate: 0.1, // Can be increased for full tracing
            ..Default::default()
        },
    ));

    let config = Arc::new(app::config::AppConfig::from_env().unwrap_or_else(|e| {
        eprintln!("\n❌ Configuration Error\n");
        eprintln!("{e}");
        eprintln!("\n💡 Tip: run `dual-core-pdf-pipeline doctor` to check your full setup,");
        eprintln!("   or copy .env.example to .env and fill in the required values.\n");
        std::process::exit(exit_code::CONFIG);
    }));

    let _telemetry_guard = app::telemetry::init(&config);

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║   Bank Statement Fidelity Editor v1.0.0                   ║");
    println!("║   100% Visual Fidelity • Smart Targeted Editing           ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    // Software root of trust
    if let Err(e) = security::software_root::require_software_attestation() {
        tracing::error!("[SECURITY] {}", e);
        std::process::exit(exit_code::GENERAL);
    }

    let app_paths = match app::paths::AppPaths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            tracing::error!("[PATHS] Failed to initialize platform application root: {error}");
            std::process::exit(exit_code::IO);
        }
    };

    // Open Audit Log
    let audit_log =
        match app::audit::AuditLog::open(app_paths.audit_dir().to_string_lossy().as_ref()) {
            Ok(log) => log,
            Err(e) => {
                tracing::error!("[AUDIT] Failed to open audit log: {}", e);
                std::process::exit(exit_code::IO);
            }
        };

    // Start Runtime (Unified Worker)
    let (mut runtime, job_tx, job_rx) = app::runtime::Runtime::start(audit_log, config.clone());

    // Dispatch to CLI module
    let code = app::cli::run(cli, job_tx, job_rx, config.clone());
    let shutdown_clean = runtime.shutdown(std::time::Duration::from_secs(5));
    drop(runtime);
    drop(_telemetry_guard);
    drop(_sentry);
    let final_code = if shutdown_clean || code != 0 {
        code
    } else {
        exit_code::GENERAL
    };
    std::process::exit(final_code);
}
