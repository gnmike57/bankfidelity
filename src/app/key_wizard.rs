//! API Key Setup Wizard
//!
//! A guided first-run modal that walks the user through configuring all
//! required and optional AI provider keys. Triggered automatically on first
//! launch when no `.env` file is found, and accessible at any time via
//! Settings → Configure API Keys (or the `setup-keys` CLI command).
//!
//! ## Wizard Flow
//!
//! 1. **Welcome screen** — explains what keys are needed and why
//! 2. **Required keys** — DUAL_CORE_PASSPHRASE (mandatory for encryption)
//! 3. **Document parser** — choose: Offline (PyMuPDF), LlamaParse, or pdfRest
//! 4. **AI provider** — choose: Gemini, Mistral, OpenRouter, or Local LLM
//! 5. **Optional keys** — PyMuPDF Pro (for Type-3 font support)
//! 6. **Validation** — live API test for each configured key
//! 7. **Save** — writes `.env` and hot-reloads the engine
//!
//! ## Key Reference
//!
//! | Key | Required | Purpose | Get it at |
//! |-----|----------|---------|-----------|
//! | `DUAL_CORE_PASSPHRASE` | YES | Encrypts the change history database | (generate locally) |
//! | `GEMINI_API_KEY` | One of these | Vision AI + balance verification | aistudio.google.com |
//! | `MISTRAL_API_KEY` | One of these | Alternative AI provider | console.mistral.ai |
//! | `OPENROUTER_API_KEY` | One of these | Multi-model router | openrouter.ai |
//! | `LLAMAPARSE_API_KEY` | Optional | Premium PDF parsing | cloud.llamaindex.ai |
//! | `PDFREST_API_KEY` | Optional | Cloud PDF processing | pdfrest.com |
//! | `PYMUPDF_PRO_KEY` | Optional | Type-3 font support (NAB) | pymupdf.io |
//!
//! ## CLI Usage
//!
//! ```bash
//! # Interactive guided setup
//! ./dual-core-pdf-pipeline setup-keys
//!
//! # Set a single key non-interactively
//! ./dual-core-pdf-pipeline setup-keys --provider gemini --key AIza...
//!
//! # Verify all configured keys
//! ./dual-core-pdf-pipeline verify-api-keys
//!
//! # Print current configuration (no key values)
//! ./dual-core-pdf-pipeline doctor
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

/// All configurable API keys with metadata.
#[derive(Debug, Clone)]
pub struct KeySpec {
    pub env_var: &'static str,
    pub provider: &'static str,
    pub required: bool,
    pub description: &'static str,
    pub get_url: &'static str,
    pub placeholder: &'static str,
}

pub const ALL_KEYS: &[KeySpec] = &[
    KeySpec {
        env_var: "DUAL_CORE_PASSPHRASE",
        provider: "Security",
        required: true,
        description: "Encrypts the change history database. Must be at least 16 characters.",
        get_url: "",
        placeholder: "my-secure-passphrase-here",
    },
    KeySpec {
        env_var: "GEMINI_API_KEY",
        provider: "Google Gemini",
        required: false,
        description: "Vision AI for balance verification and natural language editing.",
        get_url: "https://aistudio.google.com/app/apikey",
        placeholder: "AIza...",
    },
    KeySpec {
        env_var: "MISTRAL_API_KEY",
        provider: "Mistral AI",
        required: false,
        description: "Alternative AI provider for document understanding.",
        get_url: "https://console.mistral.ai/api-keys",
        placeholder: "...",
    },
    KeySpec {
        env_var: "OPENROUTER_API_KEY",
        provider: "OpenRouter",
        required: false,
        description: "Multi-model router supporting GPT-4o, Claude, Llama, and more.",
        get_url: "https://openrouter.ai/keys",
        placeholder: "sk-or-...",
    },
    KeySpec {
        env_var: "LLAMAPARSE_API_KEY",
        provider: "LlamaParse",
        required: false,
        description: "Premium PDF parsing with table and layout preservation.",
        get_url: "https://cloud.llamaindex.ai",
        placeholder: "llx-...",
    },
    KeySpec {
        env_var: "PDFREST_API_KEY",
        provider: "pdfRest",
        required: false,
        description: "Cloud PDF processing for complex layout operations.",
        get_url: "https://pdfrest.com/dashboard",
        placeholder: "...",
    },
    KeySpec {
        env_var: "PYMUPDF_PRO_KEY",
        provider: "PyMuPDF Pro",
        required: false,
        description: "Enables Type-3 font support for NAB statements.",
        get_url: "https://pymupdf.io/pro",
        placeholder: "...",
    },
];

/// Wizard state for the egui modal.
#[derive(Debug, Default, Clone)]
pub struct KeyWizardState {
    pub step: usize,
    pub values: HashMap<String, String>,
    pub validation_results: HashMap<String, ValidationResult>,
    pub is_validating: bool,
    pub show_values: HashMap<String, bool>,
    pub completed: bool,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub status: ValidationStatus,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationStatus {
    Pending,
    Testing,
    Ok,
    Error,
    NotConfigured,
}

impl KeyWizardState {
    pub fn new() -> Self {
        let mut state = Self::default();
        // Pre-populate from environment
        for key in ALL_KEYS {
            if let Ok(val) = std::env::var(key.env_var) {
                if !val.is_empty() {
                    state.values.insert(key.env_var.to_string(), val);
                }
            }
        }
        state
    }

    pub fn total_steps() -> usize {
        4
    }

    pub fn step_title(&self) -> &'static str {
        match self.step {
            0 => "Welcome",
            1 => "Security & Passphrase",
            2 => "AI Provider",
            3 => "Optional Keys",
            _ => "Validate & Save",
        }
    }

    pub fn is_complete(&self) -> bool {
        // Minimum: passphrase set + at least one AI provider key
        let has_passphrase = self
            .values
            .get("DUAL_CORE_PASSPHRASE")
            .map(|v| v.len() >= 16)
            .unwrap_or(false);
        let has_ai = ["GEMINI_API_KEY", "MISTRAL_API_KEY", "OPENROUTER_API_KEY"]
            .iter()
            .any(|k| self.values.get(*k).map(|v| !v.is_empty()).unwrap_or(false));
        has_passphrase && has_ai
    }

    /// Write all configured keys to the .env file.
    pub fn save_to_env(&self, env_path: &PathBuf) -> std::io::Result<()> {
        let existing = if env_path.exists() {
            std::fs::read_to_string(env_path)?
        } else {
            String::new()
        };

        let mut lines: Vec<String> = existing
            .lines()
            .filter(|l| {
                let key = l.split('=').next().unwrap_or("").trim();
                !self.values.contains_key(key)
            })
            .map(String::from)
            .collect();

        for (k, v) in &self.values {
            if !v.is_empty() {
                lines.push(format!("{}={}", k, v));
            }
        }

        // Add GEMINI_AUTH_MODE if Gemini key is set
        if self.values.contains_key("GEMINI_API_KEY")
            && !lines.iter().any(|l| l.starts_with("GEMINI_AUTH_MODE="))
        {
            lines.push("GEMINI_AUTH_MODE=api_key".to_string());
        }

        std::fs::write(env_path, lines.join("\n") + "\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wizard_completeness_check() {
        let mut state = KeyWizardState::new();
        assert!(!state.is_complete(), "Should not be complete with no keys");

        state.values.insert(
            "DUAL_CORE_PASSPHRASE".into(),
            "my-secure-passphrase-here".into(),
        );
        assert!(
            !state.is_complete(),
            "Should not be complete with only passphrase"
        );

        state
            .values
            .insert("GEMINI_API_KEY".into(), "AIzaTestKey".into());
        assert!(
            state.is_complete(),
            "Should be complete with passphrase + Gemini key"
        );
    }

    #[test]
    fn test_all_keys_have_env_vars() {
        for key in ALL_KEYS {
            assert!(!key.env_var.is_empty());
            assert!(!key.provider.is_empty());
        }
    }
}
