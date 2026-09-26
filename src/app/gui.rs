//! Bank Statement Fidelity Editor - production GUI
//!
//! Layout (5-region):
//!   [ menu / status / actions / theme toggle ]
//!   [ left: nav + thumbnails ] [ central: canvas ] [ right: tools ]
//!   [ bottom: toasts / progress / status bar ]
//!
//! All long-running work runs through `Job`s on the runtime; the UI only
//! reads `JobResult`s and never blocks.

use eframe::egui;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::app::runtime::{
    Job, JobId, JobResult, PythonJobResult, RuntimeClient, RuntimeSubmitError,
};
use crate::engine::history::ChangeHistory;
use egui_plot::PlotPoints;

pub mod results;
pub mod state;
pub mod view_canvas;
pub mod view_navigation;
pub mod view_panels;
pub mod view_workflows;

pub use state::*;

impl MyApp {
    pub fn new<T: Into<RuntimeClient>>(
        job_tx: T,
        job_rx: std::sync::mpsc::Receiver<JobResult>,
        config: std::sync::Arc<crate::app::config::AppConfig>,
    ) -> Self {
        let job_tx = job_tx.into();
        let settings: AppSettings =
            confy::load("bank-statement-modifier", None).unwrap_or_default();
        let input_path = settings
            .recent_files
            .first()
            .cloned()
            .unwrap_or_else(|| "examples/sample.pdf".to_string());
        let app_paths = crate::app::paths::AppPaths::discover().unwrap_or_else(|error| {
            tracing::error!("[gui] platform application root unavailable: {error}");
            let fallback = crate::app::paths::AppPaths::with_root(
                std::env::temp_dir().join("BankStatementFidelityEditor"),
            );
            let _ = fallback.ensure();
            fallback
        });
        let capability_registry =
            crate::app::capabilities::CapabilityRegistry::probe(&config, &app_paths);
        let run_workspace = app_paths
            .create_run_workspace(std::path::Path::new(&input_path))
            .ok();
        let output_path = run_workspace
            .as_ref()
            .map(|workspace| workspace.output.join("edited.pdf"))
            .unwrap_or_else(|| app_paths.root().join("edited.pdf"));
        let export_path = run_workspace
            .as_ref()
            .map(|workspace| workspace.audit.join("history.json"))
            .unwrap_or_else(|| app_paths.audit_dir().join("history.json"));

        let app = Self {
            input_path: input_path.clone(),
            output_path: output_path.to_string_lossy().to_string(),
            current_pdf_path: PathBuf::new(),
            previous_pdf_path: None,
            export_path: export_path.to_string_lossy().to_string(),
            current_page: 0,
            total_pages: 0,
            history_state: ChangeHistory::new(),
            batch_folder_path: None,
            batch_files: Vec::new(),
            current_view: AppView::SingleDocument,
            active_workflow: ActiveWorkflow::EditStatement,
            sidebar_expanded: true,
            zoom_factor: 1.0,
            pan_offset: egui::Vec2::ZERO,
            show_curtain: false,
            curtain_ratio: 0.5,
            fit_to_view: true,
            selected_block: None,
            last_click_pos: None,
            new_text: String::new(),
            natural_language_prompt: String::new(),
            current_page_texture: None,
            before_texture: None,
            after_texture: None,
            transfer_source_texture: None,
            transfer_target_texture: None,
            current_page_dpi: settings.default_dpi,
            current_page_size_pts: None,
            status: "Ready".to_string(),
            progress: None,
            last_warning: None,
            last_verification: None,
            proposed_changes: Vec::new(),
            last_imbalance: None,
            in_flight: 0,
            active_workflow_job_id: None,
            ai_explanation: None,
            toasts: VecDeque::new(),
            job_tx,
            job_rx,
            ufo_logs: Vec::new(),
            is_ufo_running: false,
            ufo_user_cancelled: false,
            pending_python: None,
            app_paths,
            run_workspace,
            last_render_request: None,
            command_query: String::new(),
            agent_autonomous_mode: false,
            workflow_stage: crate::engine::workflow::WorkflowStage::Idle,
            workflow_transactions: Vec::new(),
            workflow_validation: None,
            workflow_df: None,
            workflow_edits: Vec::new(),
            workflow_preview: None,
            workflow_visual: None,
            workflow_outcome: None,
            last_runtime_activity: std::time::Instant::now(),
            stuck_detection: None,
            native_engine: None,
            font_analysis: None,
            font_cascade_reports: Vec::new(),
            active_modal: ActiveModal::None,
            transfer_source_path: String::new(),
            feedback_text: String::new(),
            feedback_include_logs: true,
            feedback_include_audit: true,
            date_adjust_shift_days: "0".to_string(),
            date_adjust_mode_shift: true,
            date_adjust_from: String::new(),
            date_adjust_to: String::new(),
            pending_ai_confirmations: Vec::new(),
            pending_interactive_fallback: None,
            transfer_test_paths: Vec::new(),
            transfer_test_report: None,
            telemetry_cpu: 0.0,
            telemetry_ram_mb: 0,
            workflow_dirty: false,
            workflow_last_save: None,
            workflow_input_hash: None,
            workflow_cell_buffers: std::collections::HashMap::new(),
            api_availability: config.detect_availability(),
            capability_registry,
            config: config.clone(),
            api_health: None,
            settings,
            // Seed API-key editor buffers from the current environment so the
            // Settings panel shows what's active. Values are masked in the UI.
            edit_gemini_api_key: config.gemini_api_key.clone().unwrap_or_default(),
            edit_docai_project_id: config
                .document_ai
                .as_ref()
                .map(|d| d.project_id.clone())
                .unwrap_or_default(),
            edit_docai_location: config
                .document_ai
                .as_ref()
                .map(|d| d.location.clone())
                .unwrap_or_else(|| "us".to_string()),
            edit_docai_processor_id: config
                .document_ai
                .as_ref()
                .map(|d| d.processor_id.clone())
                .unwrap_or_default(),
            edit_docai_service_account: config
                .document_ai
                .as_ref()
                .map(|d| d.service_account_path.clone())
                .unwrap_or_default(),
            edit_docai_api_key: config
                .document_ai
                .as_ref()
                .map(|d| d.api_key.clone())
                .unwrap_or_default(),
            edit_pymupdf_pro_key: config.pymupdf_pro_key.clone().unwrap_or_default(),
            edit_llamaparse_api_key: config.llamaparse_api_key.clone().unwrap_or_default(),
            edit_pdfrest_api_key: config.pdfrest_api_key.clone().unwrap_or_default(),
            edit_vision_api_key: config.vision_api_key.clone().unwrap_or_default(),
            edit_groq_api_key: config.groq_api_key.clone().unwrap_or_default(),
            edit_openrouter_api_key: config.openrouter_api_key.clone().unwrap_or_default(),
            edit_openrouter_model: config.openrouter_model.clone(),
            edit_mistral_api_key: config.mistral_api_key.clone().unwrap_or_default(),
            edit_mistral_model: config.mistral_model.clone(),
            edit_lipi_api_key: config.lipi_api_key.clone().unwrap_or_default(),
            edit_mindee_api_key: config.mindee_api_key.clone().unwrap_or_default(),
            edit_gemini_use_vertex: matches!(
                config.gemini_auth_mode,
                crate::app::config::GeminiAuthMode::Vertex
            ),
            edit_engine_mode: config.engine_mode,
            config_status: None,
            credential_validation_status: None,
            api_keys_seeded: true,
            pending_autofix: None,
            selected_parser_version: crate::app::config::DEFAULT_DOCAI_PROCESSOR_VERSION
                .to_string(),
            docai_versions: Vec::new(),
            docai_versions_loading: false,
            docai_training_status: None,
            docai_active_operation: None,
        };
        // Log which API backends were detected at boot for diagnostics.
        app.api_availability.log_summary();

        // Seed USE_VISION_AI environment variable from the loaded AppSettings
        // so that the verification engine (running on the tokio runtime) respects
        // the GUI toggle on startup.
        std::env::set_var(
            "USE_VISION_AI",
            if app.settings.use_vision_ai { "1" } else { "0" },
        );

        // Seed AI_PROVIDER from persisted settings so the runtime AppConfig
        // snapshot picks it up on the initial ReloadConfig below.
        std::env::set_var("AI_PROVIDER", app.settings.ai_provider.env_key());
        std::env::set_var(
            "INTERACTIVE_FALLBACKS",
            if app.settings.interactive_fallbacks {
                "true"
            } else {
                "false"
            },
        );

        // Dispatch a one-time ReloadConfig so the runtime's config_holder
        // picks up the persisted provider + any env vars seeded above.
        // USE_VISION_AI is read live by verification, but ai_provider lives
        // in the config snapshot, so a reload is required.
        if let Err(e) = app.job_tx.send(crate::app::runtime::Job::ReloadConfig) {
            tracing::warn!("[gui] boot ReloadConfig failed (runtime may not be ready): {e}");
        }

        app
    }

    // -- helpers --------------------------------------------------------------

    /// Persist the in-app credential buffers to `.env`, apply them to the
    /// current process environment, and tell the runtime to hot-reload its
    /// `AppConfig` so subsequent Document AI / Gemini / Pro jobs use the new
    /// values without an application restart.
    ///
    /// Keys are upserted into `.env` (existing lines replaced in place,
    /// missing ones appended). Empty buffers remove the override from the live
    /// environment so a cleared field truly disables that credential.
    pub(crate) fn sync_credential_editors_from_config(
        &mut self,
        config: &crate::app::config::AppConfig,
    ) {
        self.edit_gemini_api_key = config.gemini_api_key.clone().unwrap_or_default();
        if let Some(document_ai) = &config.document_ai {
            self.edit_docai_project_id = document_ai.project_id.clone();
            self.edit_docai_location = document_ai.location.clone();
            self.edit_docai_processor_id = document_ai.processor_id.clone();
            self.edit_docai_service_account = document_ai.service_account_path.clone();
            self.edit_docai_api_key = document_ai.api_key.clone();
        } else {
            self.edit_docai_project_id.clear();
            self.edit_docai_location = "us".to_string();
            self.edit_docai_processor_id.clear();
            self.edit_docai_service_account.clear();
            self.edit_docai_api_key.clear();
        }
        self.edit_pymupdf_pro_key = config.pymupdf_pro_key.clone().unwrap_or_default();
        self.edit_gemini_use_vertex = matches!(
            config.gemini_auth_mode,
            crate::app::config::GeminiAuthMode::Vertex
        );
        self.edit_llamaparse_api_key = config.llamaparse_api_key.clone().unwrap_or_default();
        self.edit_pdfrest_api_key = config.pdfrest_api_key.clone().unwrap_or_default();
        self.edit_lipi_api_key = config.lipi_api_key.clone().unwrap_or_default();
        self.edit_vision_api_key = config.vision_api_key.clone().unwrap_or_default();
        self.edit_groq_api_key = config.groq_api_key.clone().unwrap_or_default();
        self.edit_openrouter_api_key = config.openrouter_api_key.clone().unwrap_or_default();
        self.edit_openrouter_model = config.openrouter_model.clone();
        self.edit_mistral_api_key = config.mistral_api_key.clone().unwrap_or_default();
        self.edit_mistral_model = config.mistral_model.clone();
        self.edit_mindee_api_key = config.mindee_api_key.clone().unwrap_or_default();
        self.edit_engine_mode = config.engine_mode;
        self.settings.ai_provider = config.ai_provider;
        self.settings.interactive_fallbacks = config.interactive_fallbacks;
    }

    pub fn save_credentials(&mut self) {
        // (env var name, value) pairs to upsert.
        let pairs: Vec<(&str, String)> = vec![
            (
                "GEMINI_API_KEY",
                self.edit_gemini_api_key.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_PROJECT_ID",
                self.edit_docai_project_id.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_LOCATION",
                self.edit_docai_location.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_PROCESSOR_ID",
                self.edit_docai_processor_id.trim().to_string(),
            ),
            (
                "GOOGLE_APPLICATION_CREDENTIALS",
                self.edit_docai_service_account.trim().to_string(),
            ),
            (
                "DOCUMENT_AI_API_KEY",
                self.edit_docai_api_key.trim().to_string(),
            ),
            (
                "PYMUPDF_PRO_KEY",
                self.edit_pymupdf_pro_key.trim().to_string(),
            ),
            (
                "LLAMAPARSE_API_KEY",
                self.edit_llamaparse_api_key.trim().to_string(),
            ),
            (
                "PDFREST_API_KEY",
                self.edit_pdfrest_api_key.trim().to_string(),
            ),
            (
                "VISION_API_KEY",
                self.edit_vision_api_key.trim().to_string(),
            ),
            ("GROQ_API_KEY", self.edit_groq_api_key.trim().to_string()),
            (
                "OPENROUTER_API_KEY",
                self.edit_openrouter_api_key.trim().to_string(),
            ),
            (
                "MISTRAL_API_KEY",
                self.edit_mistral_api_key.trim().to_string(),
            ),
            (
                "MINDEE_API_KEY",
                self.edit_mindee_api_key.trim().to_string(),
            ),
            (
                "GEMINI_AUTH_MODE",
                if self.edit_gemini_use_vertex {
                    "vertex".to_string()
                } else {
                    "api_key".to_string()
                },
            ),
            (
                "PDF_ENGINE_MODE",
                match self.edit_engine_mode {
                    crate::app::config::PdfEngineMode::DualConcurrent => "dual".to_string(),
                    crate::app::config::PdfEngineMode::PyMuPdfProPrimary => "auto".to_string(),
                    crate::app::config::PdfEngineMode::NativeOnly => "native".to_string(),
                    crate::app::config::PdfEngineMode::PyMuPdfOnly => "pymupdf".to_string(),
                    crate::app::config::PdfEngineMode::TypstReconstruct => "typst".to_string(),
                },
            ),
            (
                "USE_VISION_AI",
                if self.settings.use_vision_ai {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            ),
            (
                "AI_PROVIDER",
                self.settings.ai_provider.env_key().to_string(),
            ),
            (
                "INTERACTIVE_FALLBACKS",
                if self.settings.interactive_fallbacks {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            ),
        ];

        // 1) Apply to the live process environment so from_env() sees them.
        for (k, v) in &pairs {
            if v.is_empty() {
                std::env::remove_var(k);
            } else {
                std::env::set_var(k, v);
            }
        }

        // 2) Upsert into .env so the change survives a restart.
        if let Err(e) = upsert_env_file(std::path::Path::new(".env"), &pairs) {
            tracing::warn!("[gui] failed to write .env: {}", e);
            self.toast(ToastKind::Error, format!("Could not write .env: {e}"));
            // Still attempt the live reload below - the in-memory env is set.
        }

        // 3) Ask the runtime to hot-reload AppConfig from the environment.
        if let Err(e) = self.job_tx.send(Job::ReloadConfig) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
        self.toast(ToastKind::Info, "Saving credentials and reloading...");
    }

    pub fn toast(&mut self, kind: ToastKind, msg: impl Into<String>) {
        self.toasts.push_back(Toast {
            kind,
            text: msg.into(),
            expires_at: Instant::now() + Duration::from_secs(6),
            action_label: None,
            action_id: None,
        });
        while self.toasts.len() > 5 {
            self.toasts.pop_front();
        }
    }

    pub(crate) fn activate_run_workspace(&mut self, document: &std::path::Path) {
        match self.app_paths.create_run_workspace(document) {
            Ok(workspace) => {
                self.output_path = workspace
                    .output
                    .join("edited.pdf")
                    .to_string_lossy()
                    .to_string();
                self.export_path = workspace
                    .audit
                    .join("history.json")
                    .to_string_lossy()
                    .to_string();
                self.run_workspace = Some(workspace);
            }
            Err(error) => {
                tracing::error!("[gui] failed to create document workspace: {error}");
                self.toast(
                    ToastKind::Error,
                    "Could not create an isolated document workspace",
                );
            }
        }
    }

    pub(crate) fn active_workflow_draft_path(&self) -> PathBuf {
        self.run_workspace
            .as_ref()
            .map(|workspace| workspace.drafts.join("workflow.json"))
            .unwrap_or_else(Self::workflow_draft_path)
    }

    pub(crate) fn discard_active_workflow_draft_quiet(&self) {
        let path = self.active_workflow_draft_path();
        if path.exists() {
            if let Err(error) = std::fs::remove_file(&path) {
                tracing::warn!("[gui] removing workflow draft failed: {error}");
            }
        }
    }

    pub(crate) fn dispatch_workflow_job(&mut self, job: Job) -> Result<JobId, RuntimeSubmitError> {
        let job_id = self.job_tx.send(job)?;
        self.active_workflow_job_id = Some(job_id);
        Ok(job_id)
    }

    pub(crate) fn cancel_active_workflow(&mut self) {
        let Some(job_id) = self.active_workflow_job_id else {
            self.toast(ToastKind::Warn, "No active workflow to cancel");
            return;
        };
        match self.job_tx.send(Job::Cancel { id: job_id }) {
            Ok(_) => {
                self.status = format!("Cancelling workflow job #{job_id}...");
                self.toast(ToastKind::Info, format!("Cancelling job #{job_id}"));
            }
            Err(error) => {
                tracing::error!("Runtime disconnected while cancelling job #{job_id}: {error}");
                self.toast(ToastKind::Error, "Could not send cancellation request");
            }
        }
    }

    pub(crate) fn apply_workflow_event(
        &mut self,
        event: crate::engine::workflow::WorkflowEvent,
    ) -> bool {
        let event_name = event.name();
        match self.workflow_stage.apply_event(event) {
            Ok(()) => {
                self.status = format!("Workflow: {}", self.workflow_stage.label());
                true
            }
            Err(error) => {
                let message = format!("Rejected workflow event '{event_name}': {error}");
                tracing::error!(
                    event = event_name,
                    state = ?self.workflow_stage.kind(),
                    "{message}"
                );
                self.status = message.clone();
                self.toast(ToastKind::Error, message);
                false
            }
        }
    }

    /// Pair edited PDFs with their corresponding originals for batch verification.
    ///
    /// Convention: an edited file is named `<base>_edited.pdf`. Its original is
    /// looked up as `<base>_original.pdf` first, falling back to a bare
    /// `<base>.pdf` in the same set. Returns `(original, edited)` pairs.
    #[allow(dead_code)]
    pub fn pair_originals_and_edited(files: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
        use std::collections::HashMap;
        let by_stem: HashMap<String, PathBuf> = files
            .iter()
            .filter_map(|p| {
                p.file_stem()
                    .map(|s| (s.to_string_lossy().to_string(), p.clone()))
            })
            .collect();

        let mut pairs = Vec::new();
        for (stem, edited) in &by_stem {
            if let Some(base) = stem.strip_suffix("_edited") {
                if let Some(original) = by_stem
                    .get(&format!("{base}_original"))
                    .or_else(|| by_stem.get(base))
                {
                    pairs.push((original.clone(), edited.clone()));
                }
            }
        }
        pairs.sort();
        pairs
    }

    #[allow(dead_code)]
    pub(crate) fn toast_with_action(
        &mut self,
        kind: ToastKind,
        msg: impl Into<String>,
        label: impl Into<String>,
        id: impl Into<String>,
    ) {
        self.toasts.push_back(Toast {
            kind,
            text: msg.into(),
            expires_at: Instant::now() + Duration::from_secs(12),
            action_label: Some(label.into()),
            action_id: Some(id.into()),
        });
        while self.toasts.len() > 5 {
            self.toasts.pop_front();
        }
    }

    pub fn request_render(&mut self, tag: &str) {
        // Only render if the page actually changed since the last request for
        // this tag. This drops bursts when the user clicks rapidly through
        // pages or zooms - preventing render queue blow-up.
        let key = (
            tag.to_string(),
            self.current_page,
            self.current_page_dpi as u32,
        );
        if self.last_render_request.as_ref() == Some(&key) && tag == "current" {
            // already requested with same parameters
            return;
        }
        self.last_render_request = Some(key);

        let path = if tag == "before" {
            self.previous_pdf_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&self.input_path))
        } else {
            self.current_pdf_path.clone()
        };
        if !path.exists() {
            tracing::warn!("[gui] cannot render {:?} (does not exist)", path);
            return;
        }
        if let Err(e) = self.job_tx.send(Job::RenderPage {
            path,
            page: self.current_page,
            dpi: self.current_page_dpi,
            tag: tag.to_string(),
        }) {
            tracing::error!("Runtime disconnected: {}", e);
        }
        self.in_flight += 1;
    }

    pub(crate) fn update_recent_files(&mut self, path: String) {
        self.settings.recent_files.retain(|f| f != &path);
        self.settings.recent_files.insert(0, path);
        if self.settings.recent_files.len() > 10 {
            self.settings.recent_files.pop();
        }
        if let Err(e) = confy::store("bank-statement-modifier", None, &self.settings) {
            tracing::warn!("[gui] failed to persist settings: {}", e);
        }
    }

    pub(crate) fn load_texture_from_bytes(
        &self,
        ctx: &egui::Context,
        name: &str,
        bytes: &[u8],
    ) -> Option<egui::TextureHandle> {
        let image = match image::load_from_memory(bytes) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("[gui] failed to decode rendered PNG '{}': {}", name, e);
                return None;
            }
        };
        let image = image.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = image.as_flat_samples();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
        Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
    }
    /// G2: Build a real `.tar.gz` artifact bundle containing the input PDF,
    /// edited output PDF, audit log, and change history JSON.
    #[allow(dead_code)]
    pub fn build_artifact_bundle(
        input_path: &str,
        output_path: &std::path::Path,
        bundle_path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::fs::File;

        let gz_file = File::create(bundle_path)?;
        let enc = GzEncoder::new(gz_file, Compression::default());
        let mut ar = tar::Builder::new(enc);

        // Add input PDF if it exists
        let input = std::path::Path::new(input_path);
        if input.exists() {
            ar.append_path_with_name(
                input,
                format!(
                    "bundle/{}",
                    input.file_name().unwrap_or_default().to_string_lossy()
                ),
            )?;
        }

        // Add edited output PDF if it exists
        if output_path.exists() {
            ar.append_path_with_name(
                output_path,
                format!(
                    "bundle/{}",
                    output_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            )?;
        }

        // Add audit log if it exists
        let audit_dir = std::path::Path::new("audit");
        if audit_dir.exists() {
            for entry in std::fs::read_dir(audit_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    ar.append_path_with_name(
                        &path,
                        format!(
                            "bundle/audit/{}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                    )?;
                }
            }
        }

        // Add change history JSON if it exists
        let history_path = std::path::Path::new("audit/change_history.json");
        if history_path.exists() {
            ar.append_path_with_name(history_path, "bundle/change_history.json")?;
        }

        ar.into_inner()?.finish()?;
        Ok(())
    }

    pub fn export_to_excel(&mut self) {
        let result: Result<(), Box<dyn std::error::Error>> = (|| {
            let mut workbook = rust_xlsxwriter::Workbook::new();
            let worksheet = workbook.add_worksheet();
            worksheet.write_string(0, 0, "#")?;
            worksheet.write_string(0, 1, "Page")?;
            worksheet.write_string(0, 2, "Old Text")?;
            worksheet.write_string(0, 3, "New Text")?;
            worksheet.write_string(0, 4, "Reason")?;
            worksheet.write_string(0, 5, "Timestamp")?;
            for (i, rec) in self.history_state.get_history().iter().enumerate() {
                let row = (i + 1) as u32;
                worksheet.write_number(row, 0, (i + 1) as f64)?;
                worksheet.write_number(row, 1, (rec.page + 1) as f64)?;
                worksheet.write_string(row, 2, &rec.old_text)?;
                worksheet.write_string(row, 3, &rec.new_text)?;
                worksheet.write_string(row, 4, &rec.description)?;
                worksheet.write_string(row, 5, &rec.timestamp)?;
            }
            std::fs::create_dir_all("output")?;
            workbook.save("output/export.xlsx")?;
            Ok(())
        })();
        match result {
            Ok(_) => self.toast(ToastKind::Success, "Exported history to output/export.xlsx"),
            Err(e) => self.toast(ToastKind::Error, format!("Excel export failed: {e}")),
        }
    }

    pub fn fit_zoom_to_view(&mut self, available: egui::Vec2, tex_size: egui::Vec2) {
        if tex_size.x <= 0.0 || tex_size.y <= 0.0 {
            return;
        }
        let scale_x = available.x / tex_size.x;
        let scale_y = available.y / tex_size.y;
        self.zoom_factor = scale_x.min(scale_y).clamp(0.1, 5.0) * 0.95;
        self.pan_offset = egui::Vec2::ZERO;
    }

    pub fn balance_trend_points(&self) -> PlotPoints {
        // Real running-balance trend (no fake data).
        let pts: Vec<[f64; 2]> = self
            .history_state
            .get_history()
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.new_text
                    .replace(['$', ','], "")
                    .parse::<f64>()
                    .ok()
                    .map(|v| [i as f64, v])
            })
            .collect();
        if pts.is_empty() {
            PlotPoints::from(vec![[0.0, 0.0]])
        } else {
            PlotPoints::from(pts)
        }
    }
}

// ---------------------------------------------------------------------------
// eframe::App
// ---------------------------------------------------------------------------

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.headless_update(ctx);
    }
}

impl MyApp {
    pub fn headless_update(&mut self, ctx: &egui::Context) {
        // theme
        self.settings.theme.apply(ctx);

        // High-DPI auto-scaling (Stage 6)
        let target_dpi = self.settings.default_dpi * ctx.pixels_per_point();
        if (self.current_page_dpi - target_dpi).abs() > 1.0 {
            self.current_page_dpi = target_dpi;
            self.request_render("current");
        }

        if let Some(p) = &self.progress {
            let fade =
                ctx.animate_value_with_time(egui::Id::new("progress_overlay_fade"), 1.0, 0.3);

            egui::Area::new(egui::Id::new("progress_dialog"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(self.settings.theme.palette().surface.linear_multiply(0.95))
                        .inner_margin(egui::Margin::same(32.0))
                        .rounding(egui::Rounding::same(24.0))
                        .shadow(egui::epaint::Shadow {
                            offset: egui::vec2(0.0, 20.0),
                            blur: 40.0,
                            spread: 0.0,
                            color: egui::Color32::from_black_alpha((100.0 * fade) as u8),
                        })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            self.settings.theme.palette().text.linear_multiply(0.1),
                        ))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add(
                                    egui::Spinner::new()
                                        .size(40.0)
                                        .color(self.settings.theme.palette().accent),
                                );
                                ui.add_space(24.0);
                                ui.label(
                                    egui::RichText::new(&p.label)
                                        .size(18.0)
                                        .strong()
                                        .color(self.settings.theme.palette().text),
                                );

                                ui.add_space(16.0);

                                let pct = (p.fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
                                let mut text = format!("{pct}%");

                                if p.fraction > 0.0 {
                                    let elapsed = p.started_at.elapsed().as_secs_f32();
                                    let eta = (elapsed / p.fraction) * (1.0 - p.fraction);
                                    if eta > 0.0 && eta.is_finite() {
                                        text = format!("{pct}% (ETA: {eta:.0}s)");
                                    }
                                }

                                ui.add(
                                    egui::ProgressBar::new(p.fraction.clamp(0.0, 1.0))
                                        .desired_width(320.0)
                                        .text(egui::RichText::new(text).size(14.0))
                                        .fill(self.settings.theme.palette().accent),
                                );
                            });
                        });
                });
        }

        // Stage 13 / Item #6: workflow shortcuts.
        //   Ctrl+1 -> Parse + AI validate
        //   Ctrl+2 -> Balance Out Preview
        //   Ctrl+3 -> Confirm and Render
        let want_parse =
            ctx.input(|i| i.modifiers.command_only() && i.key_pressed(egui::Key::Num1));
        let want_preview =
            ctx.input(|i| i.modifiers.command_only() && i.key_pressed(egui::Key::Num2));
        let want_confirm =
            ctx.input(|i| i.modifiers.command_only() && i.key_pressed(egui::Key::Num3));
        if want_parse && !self.input_path.is_empty() {
            if let Err(e) = self.dispatch_workflow_job(Job::WorkflowParseAndValidate {
                input: PathBuf::from(&self.input_path),
                version: Some(self.selected_parser_version.clone()),
                parser_mode: self.settings.document_parser,
                ai_provider: self.settings.ai_provider,
                ignore_offline_fallback: false,
            }) {
                tracing::error!("Runtime disconnected: {}", e);
            }
            self.in_flight += 1;
            self.workflow_edits.clear();
            self.workflow_preview = None;
            self.workflow_visual = None;
            self.workflow_outcome = None;
            self.font_cascade_reports.clear();
            self.workflow_dirty = true;
            self.toast(ToastKind::Info, "Parse triggered (Ctrl+1)");
        }
        if want_preview {
            if let Some(v) = &self.workflow_validation {
                if let Err(e) = self.dispatch_workflow_job(Job::WorkflowPreview {
                    original_transactions: self.workflow_transactions.clone(),
                    edits: self.workflow_edits.clone(),
                    opening_balance: v.opening_balance,
                    expected_closing: if v.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                        Some(v.closing_balance)
                    } else {
                        None
                    },
                }) {
                    tracing::error!("Runtime disconnected: {}", e);
                }
                self.in_flight += 1;
                self.toast(ToastKind::Info, "Preview triggered (Ctrl+2)");
            }
        }
        if want_confirm {
            if let Some(preview) = self.workflow_preview.clone() {
                if !preview.balanced {
                    self.toast(
                        ToastKind::Error,
                        format!(
                            "Render blocked: deterministic ledger is out of balance by ${:.2}",
                            preview.final_imbalance.abs()
                        ),
                    );
                } else {
                    let (kept, _) = crate::engine::workflow::prune_redundant_edits(
                        &self.workflow_edits,
                        &preview,
                    );
                    match self.dispatch_workflow_job(Job::WorkflowConfirmAndRender {
                        input: PathBuf::from(&self.input_path),
                        output: PathBuf::from(&self.output_path),
                        edits: kept,
                        original_transactions: self.workflow_transactions.clone(),
                        opening_balance: self
                            .workflow_validation
                            .as_ref()
                            .map(|validation| validation.opening_balance)
                            .unwrap_or_default(),
                        expected_closing: self.workflow_validation.as_ref().and_then(
                            |validation| {
                                if validation.closing_balance.abs() > rust_decimal::Decimal::ZERO {
                                    Some(validation.closing_balance)
                                } else {
                                    None
                                }
                            },
                        ),
                        deep_font_replication: self.settings.deep_font_replication,
                        max_visual_attempts: self.settings.max_visual_attempts,
                        visual_threshold: self.settings.visual_diff_threshold,
                        ignore_font_coverage: false,
                        ignore_visual_fidelity: false,
                    }) {
                        Ok(_) => {
                            self.in_flight += 1;
                            self.toast(ToastKind::Info, "Confirm + Render triggered (Ctrl+3)");
                        }
                        Err(error) => tracing::error!("Runtime disconnected: {}", error),
                    }
                }
            }
        }

        // Stage 13 / Item #15: Ctrl+Shift+Z removes the last queued edit
        // (regular Ctrl+Z is reserved by egui::TextEdit for buffer undo).
        let want_undo_last_edit =
            ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Z));
        if want_undo_last_edit && !self.workflow_edits.is_empty() {
            let removed = self.workflow_edits.pop();
            if let Some(e) = removed {
                // Drop the matching cell-buffer entry so the table shows
                // the original value next frame.
                self.workflow_cell_buffers
                    .remove(&(e.page, e.line_on_page, e.field));
                self.workflow_dirty = true;
                self.toast(
                    ToastKind::Info,
                    format!(
                        "Undid last edit on P{} L{} ({} pending)",
                        e.page + 1,
                        e.line_on_page + 1,
                        self.workflow_edits.len()
                    ),
                );
            }
        }

        // Drag-and-drop support: open the first dropped PDF and tell the
        // user about additional drops. Stage 13 / Item #8.
        //
        // This global path is document-only: it filters to `.pdf` paths and
        // never participates in the font-upload flow. That flow lives in
        // `modals.rs` and fires only when a drop lands on the custom-font
        // target with a supported font extension, so opening a dropped PDF
        // here can never raise the "custom font embedding" error.
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let dropped: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            let pdfs: Vec<PathBuf> = dropped
                .into_iter()
                .filter(|p| {
                    p.extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_lowercase())
                        == Some("pdf".into())
                })
                .collect();
            if let Some(first) = pdfs.first().cloned() {
                self.current_view = AppView::SingleDocument;
                self.open_pdf(first);
                if pdfs.len() > 1 {
                    self.toast(
                        ToastKind::Warn,
                        format!(
                            "Opened the first PDF; ignored {} other(s). The app handles one statement at a time.",
                            pdfs.len() - 1
                        ),
                    );
                }
            }
        }
        // Visual hover-cue while dragging files
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            let p = self.settings.theme.palette();
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("dnd-overlay"),
            ));
            painter.rect_filled(
                screen,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 110),
            );
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "📥 Drop PDF to open",
                egui::FontId::proportional(28.0),
                p.accent,
            );
        }

        // ---- 1. Drain runtime results --------------------------------------
        // Single ownership of `in_flight` decrements for completed jobs:
        // use `ends_gui_tracked_job` (not only strict `is_terminal`) so success
        // payloads like PageRendered / TransactionsExtracted free the wait
        // slot exactly once. Handlers must not double-decrement.
        loop {
            match self.job_rx.try_recv() {
                Ok(res) => {
                    self.last_runtime_activity = std::time::Instant::now();
                    if res.ends_gui_tracked_job() && self.in_flight > 0 {
                        self.in_flight -= 1;
                    }
                    self.handle_job_result(ctx, res);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.status = "❌ Runtime worker disconnected".into();
                    self.in_flight = 0; // Bulletproof fix: reset on disconnect
                    break;
                }
            }
        }

        // ---- 1.1 Watchdog Stuck Detection ----------------------------------
        if self.in_flight > 0 {
            if self.last_runtime_activity.elapsed() > std::time::Duration::from_secs(30) {
                if self.stuck_detection.is_none() {
                    tracing::warn!(
                        "Watchdog: No activity from runtime for 30s. Triggering stuck detection."
                    );
                    self.stuck_detection = Some(std::time::Instant::now());
                }
            } else {
                self.stuck_detection = None;
            }
        } else {
            self.stuck_detection = None;
        }

        // ---- 1.5 Handle drag & drop ----------------------------------------
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                if let Some(file) = i.raw.dropped_files.first() {
                    if let Some(path) = &file.path {
                        if path.is_dir() {
                            self.toast(
                                ToastKind::Warn,
                                "Folder batch UI is not included in v1. Use the `extract-batch` CLI command for bounded batch processing.",
                            );
                        } else if is_supported_font_path(path) {
                            // Font files belong exclusively to the
                            // custom-font drop target in `modals.rs`, which
                            // raises the font-upload error only for drops
                            // that land on that target. Outside it they are
                            // ignored here, so this global document-open path
                            // neither opens them as documents nor triggers
                            // the font-upload error flow.
                            tracing::debug!(
                                "[gui] ignoring font drop outside the custom-font target: {:?}",
                                path
                            );
                        }
                    }
                }
            }
        });

        // Stage 5 / Item #9: autosave the workflow draft if anything has
        // changed since the last save (debounced to 1.5s inside the helper).
        self.autosave_workflow_draft();

        // ---- 2. Check pending Python click reply ---------------------------
        if let Some(rx) = self.pending_python.as_mut() {
            match rx.try_recv() {
                Ok(PythonJobResult::Json(json)) => {
                    if json.trim() == "null" {
                        self.toast(ToastKind::Info, "No text under that click.");
                    } else {
                        match serde_json::from_str::<TextBlock>(&json) {
                            Ok(b) => {
                                self.new_text = b.text.clone();
                                self.selected_block = Some(b);
                            }
                            Err(e) => {
                                self.toast(ToastKind::Warn, format!("Click parse failed: {e}"))
                            }
                        }
                    }
                    self.pending_python = None;
                }
                Ok(other) => {
                    tracing::debug!("[gui] click reply: {:?}", other);
                    self.pending_python = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                Err(_) => self.pending_python = None,
            }
        }

        // ---- 3. Collapsible Sidebar ----------------------------------------
        self.draw_sidebar(ctx);

        // ---- 4. Bottom status bar -----------------------------------------
        self.draw_status_bar(ctx);

        // ---- 5. Main Workspace Routing ------------------------------------
        match self.active_workflow {
            ActiveWorkflow::EditStatement => {
                self.draw_edit_statement_workflow(ctx);
            }
            ActiveWorkflow::TransferTransactions => {
                self.draw_transfer_workflow(ctx);
            }
            ActiveWorkflow::AgentCommand => {
                self.draw_agent_command_workflow(ctx);
            }
            ActiveWorkflow::AuditForensics => {
                self.draw_audit_explorer_view(ctx);
            }
            ActiveWorkflow::ChaosSandbox => {
                self.draw_chaos_sandbox_workflow(ctx);
            }
            ActiveWorkflow::Settings => {
                self.draw_settings_workflow(ctx);
            }
            ActiveWorkflow::ApiKeys => {
                self.draw_api_keys_workflow(ctx);
            }
        }

        // ---- 6. Toasts ----------------------------------------------------
        if let Some(action_id) = self.draw_toasts(ctx) {
            if action_id == "open_audit_explorer" {
                self.current_view = AppView::AuditExplorer;
            }
        }

        // ---- 6b. Modal confirmations -------------------------------------
        self.draw_modals(ctx);

        // ---- 7. Keyboard shortcuts ---------------------------------------
        self.handle_shortcuts(ctx);

        // Repaint while jobs are running so progress updates animate
        if self.in_flight > 0 || !self.toasts.is_empty() || self.pending_python.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
}

/// Upsert `pairs` (env var name -> value) into a dotenv file at `path`.
///
/// Existing `KEY=...` lines are replaced in place (preserving order and
/// unrelated lines/comments); keys not present are appended. A key whose
/// value is empty is written as `KEY=` so the file documents that it was
/// intentionally cleared (the live process env already had it removed by the
/// caller). Creates the file if it does not exist.
fn upsert_env_file(path: &std::path::Path, pairs: &[(&str, String)]) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();

    // Track which keys we've already written so leftovers get appended.
    let mut remaining: std::collections::HashMap<&str, &String> =
        pairs.iter().map(|(k, v)| (*k, v)).collect();

    let mut out_lines: Vec<String> = Vec::new();
    for line in existing.lines() {
        let trimmed = line.trim_start();
        // Leave comments and blank lines untouched.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out_lines.push(line.to_string());
            continue;
        }
        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            if let Some(val) = remaining.remove(key) {
                out_lines.push(format!("{key}={val}"));
                continue;
            }
        }
        out_lines.push(line.to_string());
    }

    // Append any keys that weren't already present.
    if !remaining.is_empty() {
        // Deterministic order for the appended block.
        let mut appended: Vec<(&str, &String)> = pairs
            .iter()
            .filter(|(k, _)| remaining.contains_key(*k))
            .map(|(k, v)| (*k, v))
            .collect();
        appended.dedup_by(|a, b| a.0 == b.0);
        for (k, v) in appended {
            out_lines.push(format!("{k}={v}"));
        }
    }

    let mut contents = out_lines.join("\n");
    contents.push('\n');
    std::fs::write(path, contents)
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "Inter-Regular".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "Inter-Bold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/Inter-Bold.ttf"
        ))),
    );

    if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        prop.insert(0, "Inter-Regular".to_owned());
    }

    if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        mono.push("Inter-Regular".to_owned());
    }

    fonts.families.insert(
        egui::FontFamily::Name("Bold".into()),
        vec!["Inter-Bold".to_owned(), "Inter-Regular".to_owned()],
    );

    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn load_icon() -> egui::IconData {
    #[allow(clippy::expect_used)] // Embedded compile-time asset — failure is a build error
    let image = image::load_from_memory(include_bytes!("../../assets/icon.png"))
        .expect("Failed to open icon path")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

pub fn run_gui(
    job_tx: RuntimeClient,
    job_rx: std::sync::mpsc::Receiver<JobResult>,
    config: std::sync::Arc<crate::app::config::AppConfig>,
) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 640.0])
            .with_title(format!(
                "Bank Statement Fidelity Editor v{}",
                env!("CARGO_PKG_VERSION")
            ))
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Bank Statement Fidelity Editor",
        options,
        Box::new(move |cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(MyApp::new(job_tx, job_rx, config.clone())))
        }),
    )
}
