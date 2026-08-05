//! Unified Error Types for the Application
//!
//! This module provides a single `AppError` enum that wraps all possible
//! errors in the application, enabling consistent error handling and
//! propagation throughout the codebase.

use thiserror::Error;

/// Process exit codes used across the CLI for consistent, scriptable results.
///
/// These follow a stable convention so callers and shell scripts can branch
/// on the specific failure category rather than a generic non-zero code.
pub mod exit_code {
    /// Successful completion.
    pub const SUCCESS: i32 = 0;
    /// Generic / runtime failure.
    pub const GENERAL: i32 = 1;
    /// Configuration problem (missing or invalid env vars).
    pub const CONFIG: i32 = 2;
    /// Invalid user input or arguments.
    pub const VALIDATION: i32 = 3;
    /// A required file or resource was not found.
    pub const NOT_FOUND: i32 = 4;
    /// An I/O operation failed.
    pub const IO: i32 = 5;
    /// Completed, but with partial success (e.g. some optional steps failed).
    pub const PARTIAL: i32 = 6;
}

/// Unified application error type.
///
/// This enum wraps all possible errors that can occur in the application,
/// providing a single error type for consistent handling in CLI, GUI, and
/// library contexts.
#[derive(Error, Debug)]
pub enum AppError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// PDF engine errors
    #[error("PDF engine error: {0}")]
    PdfEngine(#[from] PdfEngineError),

    /// Extraction errors
    #[error("Extraction error: {0}")]
    Extraction(#[from] ExtractionError),

    /// Balance calculation errors
    #[error("Balance error: {0}")]
    Balance(#[from] BalanceError),

    /// Verification errors
    #[error("Verification error: {0}")]
    Verification(#[from] VerificationError),

    /// Text editing errors
    #[error("Text edit error: {0}")]
    TextEdit(#[from] TextEditError),

    /// Gemini AI client errors
    #[error("AI error: {0}")]
    AI(#[from] AIError),

    /// Document AI errors
    #[error("Document AI error: {0}")]
    DocumentAI(#[from] DocumentAIError),

    /// PDF REST API errors
    #[error("PDF REST API error: {0}")]
    PdfRest(#[from] PdfRestError),

    /// Cache errors
    #[error("Cache error: {0}")]
    Cache(#[from] CacheError),

    /// Audit log errors
    #[error("Audit error: {0}")]
    Audit(#[from] AuditError),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Runtime errors (e.g., channel disconnects)
    #[error("Runtime error: {0}")]
    Runtime(String),

    /// Validation errors (user input, arguments, etc.)
    #[error("Validation error: {0}")]
    Validation(String),

    /// Not found errors
    #[error("Not found: {0}")]
    NotFound(String),
}

impl AppError {
    /// Creates a new runtime error with the given message.
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }

    /// Creates a new validation error with the given message.
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    /// Creates a new not found error with the given context.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// Returns the exit code associated with this error type.
    /// Useful for CLI applications.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Config(_) => exit_code::CONFIG,
            Self::Validation(_) => exit_code::VALIDATION,
            Self::NotFound(_) => exit_code::NOT_FOUND,
            Self::Io(_) => exit_code::IO,
            Self::Runtime(_) => exit_code::GENERAL,
            // Other errors are application-level failures
            _ => exit_code::GENERAL,
        }
    }

    /// Returns a user-friendly message for display in CLI/GUI.
    pub fn user_message(&self) -> String {
        match self {
            Self::Config(e) => format!("Configuration problem: {e}"),
            Self::Validation(e) => format!("Invalid input: {e}"),
            Self::NotFound(e) => format!("Required item not found: {e}"),
            Self::Io(e) => format!("File operation failed: {e}"),
            Self::Runtime(e) => format!("Unexpected error: {e}"),
            Self::Json(e) => format!("Data format error: {e}"),
            // For wrapped errors, use the Display implementation
            _ => self.to_string(),
        }
    }

    /// Returns whether this error should be logged as a warning vs error.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Config(_) | Self::Validation(_) | Self::NotFound(_)
        )
    }
}

/// Configuration-specific errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error(
        "Missing required environment variable: {0}\n\n{}",
        crate::app::env_spec::guidance_for(.0)
    )]
    MissingRequired(String),

    #[error("Invalid value for {variable}: {message}")]
    InvalidValue { variable: String, message: String },

    #[error("Failed to parse {variable}: {source}")]
    ParseError {
        variable: String,
        source: std::num::ParseIntError,
    },

    #[error("Path does not exist: {0}")]
    PathNotFound(String),

    #[error("Path is not a directory: {0}")]
    NotADirectory(String),
}

impl ConfigError {
    /// Creates a new invalid value error.
    pub fn invalid_value(variable: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidValue {
            variable: variable.into(),
            message: message.into(),
        }
    }

    /// Creates a new parse error.
    pub fn parse_error(variable: impl Into<String>, source: std::num::ParseIntError) -> Self {
        Self::ParseError {
            variable: variable.into(),
            source,
        }
    }

    /// Creates a new path not found error.
    pub fn path_not_found(path: impl Into<String>) -> Self {
        Self::PathNotFound(path.into())
    }

    /// Creates a new not a directory error.
    pub fn not_a_directory(path: impl Into<String>) -> Self {
        Self::NotADirectory(path.into())
    }
}

/// PDF Engine-specific errors
#[derive(Error, Debug)]
pub enum PdfEngineError {
    #[error("Unsupported operation for this engine")]
    Unsupported,

    #[error("Failed to render page: {0}")]
    RenderFailed(String),

    #[error("Failed to load PDF: {0}")]
    LoadFailed(String),

    #[error("Failed to save PDF: {0}")]
    SaveFailed(String),

    #[error("Page {page} not found (document has {total} pages)")]
    PageNotFound { page: usize, total: usize },

    #[error("Font not found: {0}")]
    FontNotFound(String),
}

/// Extraction-specific errors
#[derive(Error, Debug)]
pub enum ExtractionError {
    #[error("Failed to extract geometry: {0}")]
    ExtractionFailed(String),

    #[error("Failed to parse transaction: {0}")]
    ParseFailed(String),

    #[error("No transactions found in document")]
    NoTransactions,
}

/// Balance calculation errors
#[derive(Error, Debug)]
pub enum BalanceError {
    #[error("Balance mismatch on line {line}: expected {expected}, got {actual} (diff: {diff})")]
    Mismatch {
        line: usize,
        expected: String,
        actual: String,
        diff: String,
    },

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),
}

/// Verification errors
#[derive(Error, Debug)]
pub enum VerificationError {
    #[error("Failed to load PDF: {0}")]
    PdfiumLoad(String),

    #[error("Failed to render PDF: {0}")]
    PdfiumRender(String),

    #[error("Failed to compute visual diff: {0}")]
    DiffFailed(String),
}

/// Text editing errors
#[derive(Error, Debug)]
pub enum TextEditError {
    #[error("Text replacement failed: {0}")]
    ReplacementFailed(String),

    #[error("Bounding box overflow: text does not fit in specified area")]
    BboxOverflow,

    #[error("Empty text provided")]
    EmptyText,
}

/// AI-related errors (Gemini)
#[derive(Error, Debug)]
pub enum AIError {
    #[error("Missing API key")]
    MissingKey,

    #[error("API request failed: {0}")]
    RequestFailed(String),

    #[error("API response invalid: {0}")]
    InvalidResponse(String),

    #[error("Rate limit exceeded")]
    RateLimited,
}

/// Document AI errors
#[derive(Error, Debug)]
pub enum DocumentAIError {
    #[error("Missing configuration: {0}")]
    MissingConfig(&'static str),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Processing failed: {0}")]
    ProcessingFailed(String),
}

/// PDF REST API errors
#[derive(Error, Debug)]
pub enum PdfRestError {
    #[error("Failed to upload PDF: {0}")]
    Upload(String),

    #[error("Failed to render PDF: {0}")]
    Render(String),

    #[error("API key not configured")]
    MissingApiKey,
}

/// Cache errors
#[derive(Error, Debug)]
pub enum CacheError {
    #[error("Cache I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Cache corruption: {0}")]
    Corruption(String),

    #[error("Cache not found: {0}")]
    NotFound(String),
}

/// Audit log errors
#[derive(Error, Debug)]
pub enum AuditError {
    /// Failed to open or create the audit directory/log file.
    #[error("failed to open audit log at {path}: {source}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Failed to write a record or line to the audit log.
    #[error("failed to write to audit log: {0}")]
    Write(#[source] std::io::Error),

    /// Failed to read or open an audit log file for parsing.
    #[error("failed to read audit log at {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Failed to create or write a snapshot for a change.
    #[error("failed to snapshot {what}: {source}")]
    Snapshot {
        what: String,
        #[source]
        source: std::io::Error,
    },
}

impl AuditError {
    /// Construct an `Open` error with a path context.
    pub fn open(path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Open {
            path: path.into(),
            source,
        }
    }

    /// Construct a `Read` error with a path context.
    pub fn read(path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }

    /// Construct a `Snapshot` error with a descriptive context.
    pub fn snapshot(what: impl Into<String>, source: std::io::Error) -> Self {
        Self::Snapshot {
            what: what.into(),
            source,
        }
    }
}

// ============================================================================
// Convenience type aliases for Result types
// ============================================================================

/// Result type for configuration operations
pub type ConfigResult<T> = std::result::Result<T, ConfigError>;

/// Result type for PDF engine operations
pub type PdfEngineResult<T> = std::result::Result<T, PdfEngineError>;

/// Result type for extraction operations
pub type ExtractionResult<T> = std::result::Result<T, ExtractionError>;

/// Result type for balance operations
pub type BalanceResult<T> = std::result::Result<T, BalanceError>;

/// Result type for verification operations
pub type VerificationResult<T> = std::result::Result<T, VerificationError>;

/// Result type for text editing operations
pub type TextEditResult<T> = std::result::Result<T, TextEditError>;

/// Result type for AI operations
pub type AIResult<T> = std::result::Result<T, AIError>;

/// Result type for Document AI operations
pub type DocumentAIResult<T> = std::result::Result<T, DocumentAIError>;

/// Result type for PDF REST operations
pub type PdfRestResult<T> = std::result::Result<T, PdfRestError>;

/// Result type for cache operations
pub type CacheResult<T> = std::result::Result<T, CacheError>;

/// Result type for audit operations
pub type AuditResult<T> = std::result::Result<T, AuditError>;
