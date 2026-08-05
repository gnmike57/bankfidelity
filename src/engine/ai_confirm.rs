//! AI Confirmation System.
//!
//! When the AI is uncertain about parsing, format detection, or any operation
//! (confidence < threshold), it pauses and asks the user a question via a modal
//! dialog. All user responses are logged as learning data.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A question the AI needs the user to answer before proceeding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfirmation {
    /// Unique identifier for this confirmation request.
    pub id: Uuid,
    /// Which stage/operation triggered the question.
    pub stage: String,
    /// The question text to show the user.
    pub question: String,
    /// Available options (e.g., ["Yes, this looks correct", "No, try again", "Skip"]).
    pub options: Vec<String>,
    /// Context about what the AI was trying to do.
    pub context: String,
    /// How confident the AI is (0.0 – 1.0). Lower = more reason to ask.
    pub confidence: f32,
    /// Index of the default/recommended option.
    pub default_answer: Option<usize>,
}

/// Response from the user to an AI confirmation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfirmationResponse {
    /// Which confirmation this responds to.
    pub id: Uuid,
    /// Index into the original `options` list.
    pub selected_option: usize,
    /// Optional free-text note from the user.
    pub user_note: Option<String>,
}

/// Confidence threshold below which the AI should ask the user.
pub const CONFIDENCE_THRESHOLD: f32 = 0.70;

/// Whether a given confidence score warrants asking the user.
pub fn should_ask_user(confidence: f32) -> bool {
    confidence < CONFIDENCE_THRESHOLD
}

/// Build a confirmation request for uncertain parsing results.
pub fn parsing_uncertain(
    stage: &str,
    question: &str,
    context: &str,
    confidence: f32,
) -> AiConfirmation {
    AiConfirmation {
        id: Uuid::new_v4(),
        stage: stage.to_string(),
        question: question.to_string(),
        options: vec![
            "Yes, this looks correct".to_string(),
            "No, try a different approach".to_string(),
            "Skip this step".to_string(),
        ],
        context: context.to_string(),
        confidence,
        default_answer: Some(0),
    }
}

/// Build a confirmation request for format detection.
pub fn format_uncertain(detected_format: &str, confidence: f32) -> AiConfirmation {
    AiConfirmation {
        id: Uuid::new_v4(),
        stage: "FormatDetection".to_string(),
        question: format!(
            "The AI detected the statement format as \"{}\" with {:.0}% confidence. Is this correct?",
            detected_format,
            confidence * 100.0,
        ),
        options: vec![
            format!("Yes, it's \"{}\"", detected_format),
            "No, let me specify the format".to_string(),
            "Skip format detection".to_string(),
        ],
        context: format!("Detected format: {detected_format}"),
        confidence,
        default_answer: Some(0),
    }
}

/// Log a user's confirmation response as learning data.
/// Appends to `audit/ai_learning/responses.jsonl`.
pub fn log_learning_response(
    confirmation: &AiConfirmation,
    response: &AiConfirmationResponse,
) -> std::io::Result<()> {
    use std::io::Write;

    let dir = std::path::PathBuf::from("audit/ai_learning");
    std::fs::create_dir_all(&dir)?;

    let entry = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "confirmation_id": confirmation.id.to_string(),
        "stage": confirmation.stage,
        "question": confirmation.question,
        "ai_confidence": confirmation.confidence,
        "user_selected_option": response.selected_option,
        "user_selected_text": confirmation.options.get(response.selected_option),
        "user_note": response.user_note,
        "context": confirmation.context,
    });

    let path = dir.join("responses.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    writeln!(file, "{}", serde_json::to_string(&entry)?)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_ask_below_threshold() {
        assert!(should_ask_user(0.5));
        assert!(should_ask_user(0.69));
        assert!(!should_ask_user(0.70));
        assert!(!should_ask_user(0.95));
    }

    #[test]
    fn parsing_uncertain_builds_valid_confirmation() {
        let c = parsing_uncertain("Parse", "Is this correct?", "some context", 0.5);
        assert_eq!(c.stage, "Parse");
        assert_eq!(c.options.len(), 3);
        assert_eq!(c.default_answer, Some(0));
        assert!(c.confidence < CONFIDENCE_THRESHOLD);
    }

    #[test]
    fn format_uncertain_includes_format_name() {
        let c = format_uncertain("Fidelity Bank v2", 0.45);
        assert!(c.question.contains("Fidelity Bank v2"));
        assert!(c.question.contains("45%"));
    }
}
