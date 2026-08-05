use crate::app::config::AppConfig;
use crate::app::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    PythonPipeline,
    PyMuPdfPro,
    Pdfium,
    LocalOcr,
    WritableStorage,
    Gemini,
    GeminiVertex,
    DocumentAi,
    LlamaParse,
    Groq,
    OpenRouter,
    Mistral,
    PdfRest,
    VisionAi,
    LocalLlm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Ready,
    Configured,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityStatus {
    pub state: CapabilityState,
    pub reason: String,
}

impl CapabilityStatus {
    pub fn ready(reason: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Ready,
            reason: reason.into(),
        }
    }

    pub fn configured(reason: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Configured,
            reason: reason.into(),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            state: CapabilityState::Unavailable,
            reason: reason.into(),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.state == CapabilityState::Ready
    }

    pub fn is_selectable(&self) -> bool {
        self.state != CapabilityState::Unavailable
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityRegistry {
    statuses: BTreeMap<Capability, CapabilityStatus>,
}

impl CapabilityRegistry {
    pub fn probe(config: &AppConfig, app_paths: &AppPaths) -> Self {
        let mut registry = Self::default();

        let mut worker_pro_package = false;
        match crate::ai::python_worker::PythonWorkerSupervisor::start(
            crate::ai::python_worker::PythonWorkerConfig::default(),
        ) {
            Ok(mut supervisor) => {
                let handshake = supervisor.handshake().cloned();
                supervisor.shutdown();
                match handshake {
                    Some(handshake) if handshake.ready => {
                        worker_pro_package = handshake.pro_package_available;
                        registry.set(
                            Capability::PythonPipeline,
                            CapabilityStatus::ready(format!(
                                "Supervised Python {} worker is ready with PyMuPDF {}",
                                handshake.python_version,
                                handshake.pymupdf_version.as_deref().unwrap_or("unknown")
                            )),
                        );
                    }
                    Some(handshake) => registry.set(
                        Capability::PythonPipeline,
                        CapabilityStatus::unavailable(format!(
                            "Python worker started but bridge import failed ({})",
                            handshake.bridge_error_class.as_deref().unwrap_or("unknown")
                        )),
                    ),
                    None => registry.set(
                        Capability::PythonPipeline,
                        CapabilityStatus::unavailable(
                            "Python worker did not provide a capability handshake",
                        ),
                    ),
                }
            }
            Err(error) => registry.set(
                Capability::PythonPipeline,
                CapabilityStatus::unavailable(format!(
                    "Supervised Python worker initialization failed: {error}"
                )),
            ),
        }

        let python_ready = registry.is_ready(Capability::PythonPipeline);
        if config.pro_editing_available() && python_ready && worker_pro_package {
            registry.set(
                Capability::PyMuPdfPro,
                CapabilityStatus::configured(
                    "PyMuPDF Pro package and key are configured; entitlement is verified on first unlock",
                ),
            );
        } else {
            let reason = if !python_ready {
                "PyMuPDF Pro requires the supervised Python pipeline".to_string()
            } else if !worker_pro_package {
                "PyMuPDF Pro package is not installed in the worker runtime".to_string()
            } else {
                config.pro_editing_status_reason().to_string()
            };
            registry.set(
                Capability::PyMuPdfPro,
                CapabilityStatus::unavailable(reason),
            );
        }

        match crate::pdf::native_engine::pdfium_resolver::probe_local() {
            Ok(path) if path.as_os_str().is_empty() => registry.set(
                Capability::Pdfium,
                CapabilityStatus::ready("Pdfium is available as a system library"),
            ),
            Ok(path) => registry.set(
                Capability::Pdfium,
                CapabilityStatus::ready(format!("Pdfium found at {}", path.display())),
            ),
            Err(error) => registry.set(Capability::Pdfium, CapabilityStatus::unavailable(error)),
        }

        let detection_model = crate::app::paths::resolve_asset_path("models/text-detection.rten");
        let recognition_model =
            crate::app::paths::resolve_asset_path("models/text-recognition.rten");
        if cfg!(feature = "ocr") && detection_model.is_file() && recognition_model.is_file() {
            registry.set(
                Capability::LocalOcr,
                CapabilityStatus::ready("OCR feature and both local models are present"),
            );
        } else {
            registry.set(
                Capability::LocalOcr,
                CapabilityStatus::unavailable(
                    "Local OCR requires the ocr build feature and both packaged RTen models",
                ),
            );
        }

        registry.set(Capability::WritableStorage, probe_storage(app_paths));

        let availability = config.detect_availability();
        registry.set_configured(
            Capability::Gemini,
            availability.gemini_api_key,
            "Gemini API key is configured; remote acceptance is verified on use",
            "GEMINI_API_KEY is not configured",
        );
        registry.set_configured(
            Capability::GeminiVertex,
            availability.gemini_vertex,
            "Vertex credentials are configured; remote acceptance is verified on use",
            "Vertex service-account or ADC credentials are not configured",
        );
        registry.set_configured(
            Capability::DocumentAi,
            availability.document_ai,
            "Document AI processor and auth are configured; readiness is verified on use",
            "Document AI processor or authentication configuration is incomplete",
        );
        registry.set_configured(
            Capability::LlamaParse,
            availability.llamaparse,
            "LlamaParse key is configured; remote acceptance is verified on use",
            "LLAMAPARSE_API_KEY is not configured",
        );
        registry.set_configured(
            Capability::Groq,
            availability.groq_api_key,
            "Groq key is configured; remote acceptance is verified on use",
            "GROQ_API_KEY is not configured",
        );
        registry.set_configured(
            Capability::OpenRouter,
            availability.openrouter_api_key,
            "OpenRouter key is configured; remote acceptance is verified on use",
            "OPENROUTER_API_KEY is not configured",
        );
        registry.set_configured(
            Capability::Mistral,
            availability.mistral_api_key,
            "Mistral key is configured; remote acceptance is verified on use",
            "MISTRAL_API_KEY is not configured",
        );
        registry.set_configured(
            Capability::PdfRest,
            availability.pdfrest,
            "pdfRest key is configured; remote acceptance is verified on use",
            "PDFREST_API_KEY is not configured",
        );
        registry.set_configured(
            Capability::VisionAi,
            availability.vision_ai,
            "Vision API key is configured; remote acceptance is verified on use",
            "VISION_API_KEY is not configured",
        );
        registry.set(
            Capability::LocalLlm,
            CapabilityStatus::unavailable(
                "No local LLM runtime has passed the Phase 08 benchmark and packaging gate",
            ),
        );

        registry
    }

    pub fn status(&self, capability: Capability) -> Option<&CapabilityStatus> {
        self.statuses.get(&capability)
    }

    pub fn is_ready(&self, capability: Capability) -> bool {
        self.status(capability)
            .is_some_and(CapabilityStatus::is_ready)
    }

    pub fn is_selectable(&self, capability: Capability) -> bool {
        self.status(capability)
            .is_some_and(CapabilityStatus::is_selectable)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Capability, &CapabilityStatus)> {
        self.statuses.iter()
    }

    fn set(&mut self, capability: Capability, status: CapabilityStatus) {
        self.statuses.insert(capability, status);
    }

    fn set_configured(
        &mut self,
        capability: Capability,
        configured: bool,
        configured_reason: &'static str,
        unavailable_reason: &'static str,
    ) {
        let status = if configured {
            CapabilityStatus::configured(configured_reason)
        } else {
            CapabilityStatus::unavailable(unavailable_reason)
        };
        self.set(capability, status);
    }
}

fn probe_storage(app_paths: &AppPaths) -> CapabilityStatus {
    if let Err(error) = app_paths.ensure() {
        return CapabilityStatus::unavailable(format!(
            "Application root could not be created: {error}"
        ));
    }
    let probe = app_paths
        .root()
        .join(format!(".write-probe-{}", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&probe)?;
        file.write_all(b"capability-probe")?;
        file.sync_all()?;
        std::fs::remove_file(&probe)?;
        Ok(())
    })();
    match result {
        Ok(()) => CapabilityStatus::ready(format!(
            "Application root is writable at {}",
            app_paths.root().display()
        )),
        Err(error) => {
            let _ = std::fs::remove_file(&probe);
            CapabilityStatus::unavailable(format!(
                "Application root is not writable at {}: {error}",
                app_paths.root().display()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_capability_is_never_selectable_or_ready() {
        let status = CapabilityStatus::unavailable("missing dependency");
        assert!(!status.is_ready());
        assert!(!status.is_selectable());
    }

    #[test]
    fn configured_cloud_capability_is_selectable_but_not_ready() {
        let status = CapabilityStatus::configured("credential configured");
        assert!(!status.is_ready());
        assert!(status.is_selectable());
    }

    #[test]
    fn storage_probe_uses_the_platform_root() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_root(temp.path().join("app-root"));
        let status = probe_storage(&paths);
        assert!(status.is_ready(), "{}", status.reason);
        assert!(paths.root().is_dir());
        assert_eq!(
            std::fs::read_dir(paths.root()).unwrap().count(),
            5,
            "only the five standard root directories should remain after probing"
        );
    }
}
