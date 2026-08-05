//! Interactive Fallback System
//!
//! Provides a mechanism for background tasks to pause execution when encountering
//! "semi-failures" (e.g., parsing errors, validation anomalies) and ask the user
//! via a GUI modal how they would like to proceed.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A request sent from a background task to the GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveFallbackRequest {
    /// Unique identifier for this request. Used to route the response back.
    pub id: Uuid,
    /// The stage or subsystem where the failure occurred (e.g., "Document Parsing").
    pub stage: String,
    /// Detailed error message or reason for the fallback prompt.
    pub error_details: String,
    /// List of alternative actions the user can choose from.
    pub alternatives: Vec<FallbackAlternative>,
}

/// A specific alternative the user can select.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackAlternative {
    /// Unique internal ID for this choice (e.g., "mindee", "offline_parser", "cancel").
    pub id: String,
    /// User-facing button label (e.g., "Try Mindee API").
    pub label: String,
    /// Optional longer description explaining the trade-offs of this choice.
    pub description: Option<String>,
}

/// The response routed back to the background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveFallbackResponse {
    /// The ID of the original request.
    pub id: Uuid,
    /// The ID of the selected alternative.
    pub selected_alternative_id: String,
}

impl InteractiveFallbackRequest {
    pub fn new(stage: impl Into<String>, error_details: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            stage: stage.into(),
            error_details: error_details.into(),
            alternatives: Vec::new(),
        }
    }

    pub fn add_alternative(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        description: Option<String>,
    ) -> Self {
        self.alternatives.push(FallbackAlternative {
            id: id.into(),
            label: label.into(),
            description,
        });
        self
    }
}
