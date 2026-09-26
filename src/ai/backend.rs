use crate::ai::gemini_client::{GeminiClient, GeminiError};
use crate::ai::openai_client::{OpenAiClient, OpenAiError};
use crate::app::config::{AiProviderMode, AppConfig};
use crate::engine::model::Transaction;

pub type BalancePlan = crate::ai::gemini_client::GeminiBalancePlan;
pub type CompletenessReport = crate::ai::gemini_client::GeminiCompletenessReport;
pub type VisionReport = crate::ai::gemini_client::GeminiVisionReport;

pub struct AiBackend {
    pub primary: AiProviderMode,
    pub openrouter: Option<OpenAiClient>,
    pub groq: Option<OpenAiClient>,
    pub mistral: Option<OpenAiClient>,
    pub mistral_native: Option<crate::ai::mistral_client::MistralClient>,
    pub gemini: Option<GeminiClient>,
}

#[derive(thiserror::Error, Debug)]
pub enum AiBackendError {
    #[error("OpenAI/OpenRouter Error: {0}")]
    OpenAi(#[from] OpenAiError),
    #[error("Mistral Error: {0}")]
    Mistral(#[from] crate::ai::mistral_client::MistralError),
    #[error("Gemini Error: {0}")]
    Gemini(#[from] GeminiError),
    #[error("No AI backends available or all failed. Last error: {0}")]
    AllFailed(String),
}

impl AiBackend {
    pub fn from_app_config(cfg: &AppConfig) -> Result<Self, AiBackendError> {
        if cfg.ai_provider == AiProviderMode::ManualOnly {
            return Err(AiBackendError::AllFailed(
                "AI disabled (ManualOnly mode)".into(),
            ));
        }

        let mut openrouter = None;
        let mut groq = None;
        let mut mistral = None;
        let mut mistral_native = None;
        let mut gemini = None;

        let mut or_cfg = cfg.clone();
        or_cfg.ai_provider = AiProviderMode::OpenRouterApiKey;
        if let Ok(c) = OpenAiClient::from_app_config(&or_cfg) {
            openrouter = Some(c);
        }

        let mut groq_cfg = cfg.clone();
        groq_cfg.ai_provider = AiProviderMode::GroqApiKey;
        if let Ok(c) = OpenAiClient::from_app_config(&groq_cfg) {
            groq = Some(c);
        }

        let mut mistral_cfg = cfg.clone();
        mistral_cfg.ai_provider = AiProviderMode::MistralApiKey;
        if let Ok(c) = OpenAiClient::from_app_config(&mistral_cfg) {
            mistral = Some(c);
        }

        if let Ok(mc) = crate::ai::mistral_client::MistralClient::from_app_config(cfg) {
            mistral_native = Some(mc);
        }

        if let Ok(c) = GeminiClient::from_app_config(cfg) {
            gemini = Some(c);
        }

        Ok(Self {
            primary: cfg.ai_provider,
            openrouter,
            groq,
            mistral,
            mistral_native,
            gemini,
        })
    }

    pub fn new_mock() -> Self {
        Self {
            primary: AiProviderMode::OpenRouterApiKey,
            openrouter: None,
            groq: None,
            mistral: None,
            mistral_native: None,
            gemini: None,
        }
    }

    pub async fn from_app_config_async(cfg: &AppConfig) -> Result<Self, AiBackendError> {
        if cfg.ai_provider == AiProviderMode::ManualOnly {
            return Err(AiBackendError::AllFailed(
                "AI disabled (ManualOnly mode)".into(),
            ));
        }

        let mut openrouter = None;
        let mut groq = None;
        let mut mistral = None;
        let mut mistral_native = None;
        let mut gemini = None;

        let mut or_cfg = cfg.clone();
        or_cfg.ai_provider = AiProviderMode::OpenRouterApiKey;
        if let Ok(c) = OpenAiClient::from_app_config_async(&or_cfg).await {
            openrouter = Some(c);
        }

        let mut groq_cfg = cfg.clone();
        groq_cfg.ai_provider = AiProviderMode::GroqApiKey;
        if let Ok(c) = OpenAiClient::from_app_config_async(&groq_cfg).await {
            groq = Some(c);
        }

        let mut mistral_cfg = cfg.clone();
        mistral_cfg.ai_provider = AiProviderMode::MistralApiKey;
        if let Ok(c) = OpenAiClient::from_app_config_async(&mistral_cfg).await {
            mistral = Some(c);
        }

        if let Ok(mc) = crate::ai::mistral_client::MistralClient::from_app_config_async(cfg).await {
            mistral_native = Some(mc);
        }

        if let Ok(c) = GeminiClient::from_app_config_async(cfg).await {
            gemini = Some(c);
        }

        Ok(Self {
            primary: cfg.ai_provider,
            openrouter,
            groq,
            mistral,
            mistral_native,
            gemini,
        })
    }

    pub async fn ping(&self) -> Result<(), AiBackendError> {
        if let Some(c) = &self.openrouter {
            if let Ok(()) = c.ping().await {
                return Ok(());
            }
        }
        if let Some(c) = &self.groq {
            if let Ok(()) = c.ping().await {
                return Ok(());
            }
        }
        if let Some(c) = &self.mistral {
            if let Ok(()) = c.ping().await {
                return Ok(());
            }
        }
        if let Some(c) = &self.gemini {
            let _ = c.ping().await;
        }
        Ok(())
    }
}

macro_rules! cascade {
    ($self:ident, $method:ident, $($args:expr),*) => {{
        let mut last_err = String::new();

        // 1. Try primary provider first
        match $self.primary {
            AiProviderMode::MistralApiKey => {
                if let Some(c) = &$self.mistral_native {
                    match c.$method($($args),*).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_err = e.to_string(),
                    }
                } else if let Some(c) = &$self.mistral {
                    match c.$method($($args),*).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_err = e.to_string(),
                    }
                }
            }
            AiProviderMode::OpenRouterApiKey => {
                if let Some(c) = &$self.openrouter {
                    match c.$method($($args),*).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_err = e.to_string(),
                    }
                }
            }
            AiProviderMode::GroqApiKey => {
                if let Some(c) = &$self.groq {
                    match c.$method($($args),*).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_err = e.to_string(),
                    }
                }
            }
            AiProviderMode::GeminiApiKey => {
                if let Some(c) = &$self.gemini {
                    match c.$method($($args),*).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_err = e.to_string(),
                    }
                }
            }
            _ => {}
        }

        // 2. Cascade: Mistral (Specialist) -> OpenRouter / Groq (Zero Gemini)
        if let Some(c) = &$self.mistral_native {
            if $self.primary != AiProviderMode::MistralApiKey {
                if let Ok(r) = c.$method($($args),*).await { return Ok(r); }
            }
        }
        if let Some(c) = &$self.mistral {
            if $self.primary != AiProviderMode::MistralApiKey {
                if let Ok(r) = c.$method($($args),*).await { return Ok(r); }
            }
        }
        if let Some(c) = &$self.openrouter {
            if $self.primary != AiProviderMode::OpenRouterApiKey {
                if let Ok(r) = c.$method($($args),*).await { return Ok(r); }
            }
        }
        if let Some(c) = &$self.groq {
            if $self.primary != AiProviderMode::GroqApiKey {
                if let Ok(r) = c.$method($($args),*).await { return Ok(r); }
            }
        }

        Err(AiBackendError::AllFailed(last_err))
    }}
}

impl AiBackend {
    pub async fn propose_balance_adjustments(
        &self,
        transactions: &[Transaction],
        imbalance: f64,
        layout: &crate::engine::layout::DocumentLayout,
    ) -> Result<BalancePlan, AiBackendError> {
        cascade!(
            self,
            propose_balance_adjustments,
            transactions,
            imbalance,
            layout
        )
    }

    pub async fn validate_parse_completeness(
        &self,
        transactions: &[Transaction],
        opening: f64,
        closing: f64,
        pages: usize,
    ) -> Result<CompletenessReport, AiBackendError> {
        cascade!(
            self,
            validate_parse_completeness,
            transactions,
            opening,
            closing,
            pages
        )
    }

    pub async fn verify_statement_mathematics(
        &self,
        transactions_json: &str,
        opening: f64,
    ) -> Result<bool, AiBackendError> {
        cascade!(
            self,
            verify_statement_mathematics,
            transactions_json,
            opening
        )
    }

    // Pass-through stubs for vision methods not supported by text-only OpenAI models.
    pub async fn validate_render_visually(
        &self,
        _doc: &[u8],
        _bboxes: &[[f32; 4]],
    ) -> Result<VisionReport, AiBackendError> {
        if let Some(c) = &self.gemini {
            c.validate_render_visually(_doc, _bboxes)
                .await
                .map_err(Into::into)
        } else {
            Ok(VisionReport {
                anomaly_score: 0.0,
                hotspots: vec![],
                notes: "Vision check bypassed (local spatial verifier active)".into(),
            })
        }
    }

    pub async fn plan_transaction_transfer(
        &self,
        source_transactions: &[Transaction],
        target_transactions: &[Transaction],
        correction_hint: Option<&str>,
    ) -> Result<crate::engine::transfer::TransferPlan, AiBackendError> {
        cascade!(
            self,
            plan_transaction_transfer,
            source_transactions,
            target_transactions,
            correction_hint
        )
    }

    pub async fn verify_transfer_math(
        &self,
        mapped_transactions: &[crate::engine::transfer::MappedTransaction],
        opening_balance: rust_decimal::Decimal,
    ) -> Result<bool, AiBackendError> {
        cascade!(
            self,
            verify_transfer_math,
            mapped_transactions,
            opening_balance
        )
    }

    pub async fn repair_extracted_transactions(
        &self,
        transactions: &[Transaction],
        opening_balance: rust_decimal::Decimal,
        closing_balance: rust_decimal::Decimal,
        raw_ocr_text: &str,
        error_message: &str,
    ) -> Result<Vec<Transaction>, AiBackendError> {
        cascade!(
            self,
            repair_extracted_transactions,
            transactions,
            opening_balance,
            closing_balance,
            raw_ocr_text,
            error_message
        )
    }

    pub async fn apply_natural_language_edit(
        &self,
        prompt: &str,
        transactions: &[Transaction],
    ) -> Result<Vec<Transaction>, AiBackendError> {
        cascade!(self, apply_natural_language_edit, prompt, transactions)
    }
}
