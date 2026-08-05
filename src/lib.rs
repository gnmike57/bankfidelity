//! Bank Statement Fidelity Editor v1.0.0
//! Public API

#![allow(missing_docs)]

pub mod ai;
pub mod app;
pub mod engine;
pub mod error; // Unified error types
pub mod extractors;
pub mod pdf;
pub mod security;

pub use crate::error::{
    AIError, AppError, AuditError, BalanceError, CacheError, ConfigError, DocumentAIError,
    ExtractionError, PdfRestError, TextEditError, VerificationError as AppVerificationError,
};

pub use engine::balance::process_and_reconcile;
pub use engine::font_metrics::ExactFontMetrics;
pub use engine::verification::{verify_edit, VerificationReport};
pub use engine::verification_v2::{EnhancedVerificationReport, VisualFidelityEngine};

pub use engine::workflow::{
    WorkflowEvent, WorkflowFailure, WorkflowStage, WorkflowStateKind, WorkflowTransitionError,
};
