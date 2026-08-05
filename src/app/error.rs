use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum AppError {
    #[error("API configuration is missing: {0}")]
    ApiConfigMissing(String),

    #[error("Required font '{0}' is missing from the system")]
    FontMissing(String),

    #[error("Failed to load PDF document: {0}")]
    PdfLoadError(String),

    #[error("I/O error occurred: {0}")]
    IoError(std::sync::Arc<std::io::Error>),

    #[error("API failure or rate limit: {0}")]
    ApiFailure(String),

    #[error("Visual verify failed: {0}")]
    VisualDrift(String),

    #[error("Parse failure or malformed PDF: {0}")]
    ParseFailure(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(std::sync::Arc::new(err))
    }
}

impl AppError {
    /// Parses string errors back into AppError for autofix interception
    pub fn parse_msg(msg: &str) -> Option<Self> {
        let lower = msg.to_lowercase();
        if lower.contains("api_key_invalid")
            || lower.contains("api key")
            || lower.contains("credentials")
        {
            Some(Self::ApiConfigMissing(msg.to_string()))
        } else if lower.contains("429")
            || lower.contains("quota")
            || lower.contains("rate limit")
            || lower.contains("api error")
        {
            Some(Self::ApiFailure(msg.to_string()))
        } else if lower.contains("font missing")
            || lower.contains("coverage missing chars")
            || lower.contains("fontconfig")
        {
            Some(Self::FontMissing(msg.to_string()))
        } else if lower.contains("visual verify failed")
            || lower.contains("drifted")
            || lower.contains("didn't converge")
        {
            Some(Self::VisualDrift(msg.to_string()))
        } else if lower.contains("parse")
            || lower.contains("corrupt")
            || lower.contains("invalid pdf")
            || lower.contains("0 transactions")
        {
            Some(Self::ParseFailure(msg.to_string()))
        } else {
            None
        }
    }

    /// Suggests an actionable UI fix for the specific error
    pub fn suggested_action(&self) -> Option<&'static str> {
        match self {
            Self::ApiConfigMissing(_) => Some("Open Settings to configure API Keys"),
            Self::ApiFailure(_) => Some("Retry with a different AI Provider (e.g. Gemini, OpenRouter) or wait for quota reset"),
            Self::FontMissing(_) => Some(
                "Return to edit review and supply a verified font with complete glyph coverage",
            ),
            Self::VisualDrift(_) => Some(
                "Review the verification evidence; adjust the edit or cancel publication",
            ),
            Self::ParseFailure(_) => Some(
                "Review parser selection or retry with the qualified offline parser",
            ),
            Self::Unknown(_) => Some("Retry the last action or check logs"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_msg() {
        assert!(matches!(
            AppError::parse_msg("Invalid API key"),
            Some(AppError::ApiConfigMissing(_))
        ));
        assert!(matches!(
            AppError::parse_msg("HTTP 429 rate limit exceeded"),
            Some(AppError::ApiFailure(_))
        ));
        assert!(matches!(
            AppError::parse_msg("font missing from system"),
            Some(AppError::FontMissing(_))
        ));
        assert!(matches!(
            AppError::parse_msg("visual verify failed, drifted"),
            Some(AppError::VisualDrift(_))
        ));
        assert!(matches!(
            AppError::parse_msg("parse error, 0 transactions"),
            Some(AppError::ParseFailure(_))
        ));
        assert!(AppError::parse_msg("totally unrelated error").is_none());
    }

    #[test]
    fn test_suggested_action() {
        let err1 = AppError::ApiConfigMissing("".into());
        assert_eq!(
            err1.suggested_action(),
            Some("Open Settings to configure API Keys")
        );

        let err2 = AppError::ApiFailure("".into());
        assert_eq!(err2.suggested_action(), Some("Retry with a different AI Provider (e.g. Gemini, OpenRouter) or wait for quota reset"));

        let err3 = AppError::FontMissing("".into());
        assert_eq!(
            err3.suggested_action(),
            Some("Return to edit review and supply a verified font with complete glyph coverage")
        );

        let err4 = AppError::VisualDrift("".into());
        assert_eq!(
            err4.suggested_action(),
            Some("Review the verification evidence; adjust the edit or cancel publication")
        );

        let err5 = AppError::ParseFailure("".into());
        assert_eq!(
            err5.suggested_action(),
            Some("Review parser selection or retry with the qualified offline parser")
        );

        let err6 = AppError::Unknown("".into());
        assert_eq!(
            err6.suggested_action(),
            Some("Retry the last action or check logs")
        );

        let io_err = AppError::IoError(std::sync::Arc::new(std::io::Error::other("io")));
        assert_eq!(io_err.suggested_action(), None);
    }

    #[test]
    fn test_from_io_error() {
        let err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let app_err: AppError = err.into();
        assert!(matches!(app_err, AppError::IoError(_)));
    }
}
