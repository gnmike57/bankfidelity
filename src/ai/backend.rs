use crate::ai::gemini_client::{GeminiClient, GeminiCompletenessReport, GeminiError};
use crate::ai::openai_client::{OpenAiClient, OpenAiError};
use crate::app::config::{AiProviderMode, AppConfig};
use crate::engine::model::Transaction;

pub struct AiBackend {
    pub primary: AiProviderMode,
    pub gemini: Option<GeminiClient>,
    pub openrouter: Option<OpenAiClient>,
    pub groq: Option<OpenAiClient>,
    pub mistral: Option<OpenAiClient>,
}

#[derive(thiserror::Error, Debug)]
pub enum AiBackendError {
    #[error("Gemini Error: {0}")]
    Gemini(#[from] GeminiError),
    #[error("OpenAI Error: {0}")]
    OpenAi(#[from] OpenAiError),
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

        let mut gemini = None;
        let mut openrouter = None;
        let mut groq = None;
        let mut mistral = None;

        if let Ok(c) = GeminiClient::from_app_config(cfg) {
            gemini = Some(c);
        }

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

        Ok(Self {
            primary: cfg.ai_provider,
            gemini,
            openrouter,
            groq,
            mistral,
        })
    }

    pub fn new_mock() -> Self {
        Self {
            primary: AiProviderMode::GeminiApiKey,
            gemini: None,
            openrouter: None,
            groq: None,
            mistral: None,
        }
    }

    pub async fn from_app_config_async(cfg: &AppConfig) -> Result<Self, AiBackendError> {
        if cfg.ai_provider == AiProviderMode::ManualOnly {
            return Err(AiBackendError::AllFailed(
                "AI disabled (ManualOnly mode)".into(),
            ));
        }

        let mut gemini = None;
        let mut openrouter = None;
        let mut groq = None;
        let mut mistral = None;

        if let Ok(c) = GeminiClient::from_app_config_async(cfg).await {
            gemini = Some(c);
        }

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

        Ok(Self {
            primary: cfg.ai_provider,
            gemini,
            openrouter,
            groq,
            mistral,
        })
    }

    pub async fn ping(&self) -> Result<(), AiBackendError> {
        // Just ping whatever we have
        if let Some(c) = &self.gemini {
            let _ = c.ping().await;
        }
        Ok(())
    }
}

macro_rules! cascade {
    ($self:ident, $method:ident, $($args:expr),*) => {{
        let mut last_err = String::new();

        // 1. Try primary
        match $self.primary {
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
            AiProviderMode::MistralApiKey => {
                if let Some(c) = &$self.mistral {
                    match c.$method($($args),*).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_err = e.to_string(),
                    }
                }
            }
            AiProviderMode::GeminiApiKey | AiProviderMode::GeminiVertex => {
                if let Some(c) = &$self.gemini {
                    match c.$method($($args),*).await {
                        Ok(r) => return Ok(r),
                        Err(e) => last_err = e.to_string(),
                    }
                }
            }
            _ => {}
        }

        // 2. Cascade: Mistral -> OpenRouter -> Gemini -> Groq
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
        if let Some(c) = &$self.gemini {
            if !matches!($self.primary, AiProviderMode::GeminiApiKey | AiProviderMode::GeminiVertex) {
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
    ) -> Result<crate::ai::gemini_client::GeminiBalancePlan, AiBackendError> {
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
    ) -> Result<GeminiCompletenessReport, AiBackendError> {
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
    ) -> Result<crate::ai::gemini_client::GeminiVisionReport, AiBackendError> {
        if let Some(c) = &self.gemini {
            c.validate_render_visually(_doc, _bboxes)
                .await
                .map_err(Into::into)
        } else {
            Ok(crate::ai::gemini_client::GeminiVisionReport {
                anomaly_score: 0.0,
                hotspots: vec![],
                notes: "Vision check bypassed (Gemini not configured)".into(),
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
}
