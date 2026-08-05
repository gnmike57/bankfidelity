use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use uuid::Uuid;

pub const PYTHON_PROTOCOL_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonOperation {
    Ping,
    GetTextBlocks,
    ReplaceTextInRect,
    FindTextBlockAtClick,
    GetAllTransactions,
    AnalyzeDocumentLayout,
    CompleteFontWithAdaption,
    DeepFontReplication,
    ApplyManyEdits,
    ChunkPdfForDocai,
    AnalyzeFonts,
    ReplicateFontForMissingChars,
    ClonePages,
    RemovePages,
    RenderPageToPng,
}

impl PythonOperation {
    pub const ALL: [Self; 15] = [
        Self::Ping,
        Self::GetTextBlocks,
        Self::ReplaceTextInRect,
        Self::FindTextBlockAtClick,
        Self::GetAllTransactions,
        Self::AnalyzeDocumentLayout,
        Self::CompleteFontWithAdaption,
        Self::DeepFontReplication,
        Self::ApplyManyEdits,
        Self::ChunkPdfForDocai,
        Self::AnalyzeFonts,
        Self::ReplicateFontForMissingChars,
        Self::ClonePages,
        Self::RemovePages,
        Self::RenderPageToPng,
    ];

    pub fn mutates_document(self) -> bool {
        matches!(
            self,
            Self::ReplaceTextInRect | Self::ApplyManyEdits | Self::ClonePages | Self::RemovePages
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonRequestEnvelope {
    pub protocol_version: String,
    pub operation_id: Uuid,
    pub operation: PythonOperation,
    pub submitted_at_unix_ms: u64,
    pub deadline_unix_ms: u64,
    pub input_sha256: Option<String>,
    pub payload: Value,
}

impl PythonRequestEnvelope {
    pub fn new(
        operation: PythonOperation,
        operation_id: Uuid,
        submitted_at_unix_ms: u64,
        deadline_unix_ms: u64,
        input_sha256: Option<String>,
        payload: Value,
    ) -> Result<Self, PythonProtocolError> {
        let request = Self {
            protocol_version: PYTHON_PROTOCOL_VERSION.to_string(),
            operation_id,
            operation,
            submitted_at_unix_ms,
            deadline_unix_ms,
            input_sha256,
            payload,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn from_json_exact(json: &str) -> Result<Self, PythonProtocolError> {
        let request: Self = serde_json::from_str(json)
            .map_err(|error| PythonProtocolError::Malformed(error.to_string()))?;
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), PythonProtocolError> {
        validate_protocol_version(&self.protocol_version)?;
        if self.operation_id.is_nil() {
            return Err(PythonProtocolError::InvalidOperationId);
        }
        if self.deadline_unix_ms < self.submitted_at_unix_ms {
            return Err(PythonProtocolError::InvalidDeadline);
        }
        validate_optional_sha256(self.input_sha256.as_deref(), "input_sha256")?;
        if !self.payload.is_object() {
            return Err(PythonProtocolError::InvalidPayload);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonDisposition {
    Succeeded,
    NoOp,
    Partial,
    Failed,
    Cancelled,
    TimedOut,
}

impl PythonDisposition {
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::TimedOut)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonCapabilityTier {
    Core,
    Pro,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonFailure {
    pub code: String,
    pub class: String,
    pub message: String,
    pub retryable: bool,
    #[serde(default)]
    pub context: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonMetrics {
    pub duration_ms: u64,
    pub rss_before_bytes: Option<u64>,
    pub rss_after_bytes: Option<u64>,
    pub open_handles_before: Option<u64>,
    pub open_handles_after: Option<u64>,
    pub gc_collections: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonResponseEnvelope {
    pub protocol_version: String,
    pub operation_id: Uuid,
    pub operation: PythonOperation,
    pub disposition: PythonDisposition,
    pub input_sha256: Option<String>,
    pub output_sha256: Option<String>,
    pub requested_count: Option<usize>,
    pub applied_count: Option<usize>,
    pub capability_tier: PythonCapabilityTier,
    #[serde(default)]
    pub warnings: Vec<PythonWarning>,
    pub metrics: PythonMetrics,
    pub payload: Value,
    pub failure: Option<PythonFailure>,
}

impl PythonResponseEnvelope {
    pub fn from_json_exact(json: &str) -> Result<Self, PythonProtocolError> {
        let response: Self = serde_json::from_str(json)
            .map_err(|error| PythonProtocolError::Malformed(error.to_string()))?;
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), PythonProtocolError> {
        validate_protocol_version(&self.protocol_version)?;
        if self.operation_id.is_nil() {
            return Err(PythonProtocolError::InvalidOperationId);
        }
        validate_optional_sha256(self.input_sha256.as_deref(), "input_sha256")?;
        validate_optional_sha256(self.output_sha256.as_deref(), "output_sha256")?;
        if !self.payload.is_object() {
            return Err(PythonProtocolError::InvalidPayload);
        }
        match (self.disposition.is_failure(), self.failure.is_some()) {
            (true, false) => return Err(PythonProtocolError::MissingFailure),
            (false, true) => return Err(PythonProtocolError::UnexpectedFailure),
            _ => {}
        }
        match (self.requested_count, self.applied_count) {
            (Some(requested), Some(applied)) if applied > requested => {
                return Err(PythonProtocolError::InvalidAppliedCount { requested, applied });
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(PythonProtocolError::IncompleteCountEvidence);
            }
            _ => {}
        }
        if self.disposition == PythonDisposition::Succeeded
            && self.operation.mutates_document()
            && self.output_sha256.is_none()
        {
            return Err(PythonProtocolError::MissingOutputHash);
        }
        if self.disposition == PythonDisposition::Succeeded {
            if let (Some(requested), Some(applied)) = (self.requested_count, self.applied_count) {
                if requested != applied {
                    return Err(PythonProtocolError::SuccessCountMismatch { requested, applied });
                }
            }
        }
        Ok(())
    }

    pub fn validate_for(&self, request: &PythonRequestEnvelope) -> Result<(), PythonProtocolError> {
        self.validate()?;
        if self.operation_id != request.operation_id {
            return Err(PythonProtocolError::OperationIdMismatch);
        }
        if self.operation != request.operation {
            return Err(PythonProtocolError::OperationMismatch);
        }
        if self.input_sha256 != request.input_sha256 {
            return Err(PythonProtocolError::InputHashMismatch);
        }
        Ok(())
    }
}

fn validate_protocol_version(version: &str) -> Result<(), PythonProtocolError> {
    if version != PYTHON_PROTOCOL_VERSION {
        return Err(PythonProtocolError::UnsupportedVersion(version.to_string()));
    }
    Ok(())
}

fn validate_optional_sha256(
    value: Option<&str>,
    field: &'static str,
) -> Result<(), PythonProtocolError> {
    if let Some(value) = value {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PythonProtocolError::InvalidSha256 { field });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PythonProtocolError {
    Malformed(String),
    UnsupportedVersion(String),
    InvalidOperationId,
    InvalidDeadline,
    InvalidSha256 { field: &'static str },
    InvalidPayload,
    MissingFailure,
    UnexpectedFailure,
    IncompleteCountEvidence,
    InvalidAppliedCount { requested: usize, applied: usize },
    MissingOutputHash,
    SuccessCountMismatch { requested: usize, applied: usize },
    OperationIdMismatch,
    OperationMismatch,
    InputHashMismatch,
}

impl fmt::Display for PythonProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(error) => {
                write!(formatter, "malformed Python protocol payload: {error}")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported Python protocol version: {version}")
            }
            Self::InvalidOperationId => write!(formatter, "operation_id must not be nil"),
            Self::InvalidDeadline => write!(formatter, "deadline precedes submission time"),
            Self::InvalidSha256 { field } => {
                write!(
                    formatter,
                    "{field} must be a 64-character hexadecimal SHA-256"
                )
            }
            Self::InvalidPayload => write!(formatter, "payload must be a JSON object"),
            Self::MissingFailure => {
                write!(formatter, "failure disposition requires failure detail")
            }
            Self::UnexpectedFailure => {
                write!(
                    formatter,
                    "non-failure disposition cannot contain failure detail"
                )
            }
            Self::IncompleteCountEvidence => {
                write!(
                    formatter,
                    "requested_count and applied_count must appear together"
                )
            }
            Self::InvalidAppliedCount { requested, applied } => write!(
                formatter,
                "applied_count {applied} exceeds requested_count {requested}"
            ),
            Self::MissingOutputHash => {
                write!(formatter, "successful mutation requires output_sha256")
            }
            Self::SuccessCountMismatch { requested, applied } => write!(
                formatter,
                "successful operation applied {applied} of {requested} requested changes"
            ),
            Self::OperationIdMismatch => write!(formatter, "response operation_id mismatch"),
            Self::OperationMismatch => write!(formatter, "response operation mismatch"),
            Self::InputHashMismatch => write!(formatter, "response input_sha256 mismatch"),
        }
    }
}

impl std::error::Error for PythonProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> PythonRequestEnvelope {
        PythonRequestEnvelope::new(
            PythonOperation::ApplyManyEdits,
            Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
            1_000,
            2_000,
            Some("11".repeat(32)),
            json!({"pdf_path": "fixture.pdf", "edits": []}),
        )
        .unwrap()
    }

    fn response() -> PythonResponseEnvelope {
        PythonResponseEnvelope {
            protocol_version: PYTHON_PROTOCOL_VERSION.to_string(),
            operation_id: request().operation_id,
            operation: PythonOperation::ApplyManyEdits,
            disposition: PythonDisposition::Succeeded,
            input_sha256: request().input_sha256,
            output_sha256: Some("22".repeat(32)),
            requested_count: Some(2),
            applied_count: Some(2),
            capability_tier: PythonCapabilityTier::Pro,
            warnings: Vec::new(),
            metrics: PythonMetrics {
                duration_ms: 25,
                rss_before_bytes: Some(100),
                rss_after_bytes: Some(100),
                open_handles_before: Some(4),
                open_handles_after: Some(4),
                gc_collections: 1,
            },
            payload: json!({"evidence": []}),
            failure: None,
        }
    }

    #[test]
    fn operation_catalog_is_complete_and_unique() {
        let names = PythonOperation::ALL
            .into_iter()
            .map(|operation| serde_json::to_string(&operation).unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), PythonOperation::ALL.len());
    }

    #[derive(Deserialize)]
    struct GoldenFixture {
        protocol_version: String,
        cases: Vec<GoldenCase>,
    }

    #[derive(Deserialize)]
    struct GoldenCase {
        request: Value,
        response: Value,
    }

    #[test]
    fn rust_and_python_accept_every_golden_operation() {
        let fixture: GoldenFixture = serde_json::from_str(include_str!(
            "../../python/contract_fixtures/v1/golden_operations.json"
        ))
        .unwrap();
        assert_eq!(fixture.protocol_version, PYTHON_PROTOCOL_VERSION);
        assert_eq!(fixture.cases.len(), PythonOperation::ALL.len());

        for (expected_operation, case) in PythonOperation::ALL.into_iter().zip(fixture.cases) {
            let request =
                PythonRequestEnvelope::from_json_exact(&case.request.to_string()).unwrap();
            let response =
                PythonResponseEnvelope::from_json_exact(&case.response.to_string()).unwrap();
            assert_eq!(request.operation, expected_operation);
            assert_eq!(response.operation, expected_operation);
            response.validate_for(&request).unwrap();
        }
    }

    #[test]
    fn exact_parser_rejects_unknown_request_fields() {
        let json = serde_json::to_string(&request()).unwrap();
        let mut value: Value = serde_json::from_str(&json).unwrap();
        value["unknown"] = json!(true);
        let error = PythonRequestEnvelope::from_json_exact(&value.to_string()).unwrap_err();
        assert!(matches!(error, PythonProtocolError::Malformed(_)));
    }

    #[test]
    fn response_requires_exact_success_evidence() {
        let mut response = response();
        response.applied_count = Some(1);
        assert!(matches!(
            response.validate(),
            Err(PythonProtocolError::SuccessCountMismatch {
                requested: 2,
                applied: 1
            })
        ));
        response.applied_count = Some(2);
        response.output_sha256 = None;
        assert_eq!(
            response.validate(),
            Err(PythonProtocolError::MissingOutputHash)
        );
    }

    #[test]
    fn response_must_match_request_identity_operation_and_input() {
        let request = request();
        let mut response = response();
        response.validate_for(&request).unwrap();
        response.operation_id = Uuid::new_v4();
        assert_eq!(
            response.validate_for(&request),
            Err(PythonProtocolError::OperationIdMismatch)
        );
    }

    #[test]
    fn failures_require_typed_failure_detail() {
        let mut response = response();
        response.disposition = PythonDisposition::Failed;
        response.output_sha256 = None;
        assert_eq!(
            response.validate(),
            Err(PythonProtocolError::MissingFailure)
        );
        response.failure = Some(PythonFailure {
            code: "PYTHON_OPERATION_FAILED".to_string(),
            class: "RuntimeError".to_string(),
            message: "fixture failure".to_string(),
            retryable: false,
            context: json!({}),
        });
        response.validate().unwrap();
    }
}
