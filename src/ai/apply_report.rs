use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const APPLY_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApplyEditEvidence {
    pub index: usize,
    pub page: usize,
    pub rect: [f64; 4],
    pub matched: bool,
    pub placed: bool,
    pub method: String,
    #[serde(default)]
    pub font_profile_sha256: Option<String>,
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApplyReport {
    pub schema_version: u32,
    pub success: bool,
    pub requested: usize,
    pub matched: usize,
    pub placed: usize,
    pub failed: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub method_per_edit: Vec<String>,
    #[serde(default)]
    pub review_flags: Vec<usize>,
    pub source_sha256: String,
    pub output_sha256: Option<String>,
    pub output_published: bool,
    pub edits: Vec<ApplyEditEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReportError(String);

impl ApplyReportError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ApplyReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ApplyReportError {}

impl ApplyReport {
    pub fn from_json_exact(
        json: &str,
        expected_requested: usize,
    ) -> Result<Self, ApplyReportError> {
        let report: Self = serde_json::from_str(json)
            .map_err(|error| ApplyReportError::new(format!("invalid ApplyReport JSON: {error}")))?;
        report.validate_exact(expected_requested)?;
        Ok(report)
    }

    pub fn validate_exact(&self, expected_requested: usize) -> Result<(), ApplyReportError> {
        if self.schema_version != APPLY_REPORT_SCHEMA_VERSION {
            return Err(ApplyReportError::new(format!(
                "unsupported ApplyReport schema version {}; expected {}",
                self.schema_version, APPLY_REPORT_SCHEMA_VERSION
            )));
        }
        if self.requested != expected_requested {
            return Err(ApplyReportError::new(format!(
                "ApplyReport requested {} edits but caller submitted {}",
                self.requested, expected_requested
            )));
        }
        if self.edits.len() != self.requested {
            return Err(ApplyReportError::new(format!(
                "ApplyReport contains {} per-edit records for {} requested edits",
                self.edits.len(),
                self.requested
            )));
        }
        if self.method_per_edit.len() != self.requested {
            return Err(ApplyReportError::new(format!(
                "ApplyReport contains {} methods for {} requested edits",
                self.method_per_edit.len(),
                self.requested
            )));
        }

        for (expected_index, evidence) in self.edits.iter().enumerate() {
            if evidence.index != expected_index {
                return Err(ApplyReportError::new(format!(
                    "ApplyReport edit index {} appears at position {}",
                    evidence.index, expected_index
                )));
            }
            if self.method_per_edit[expected_index] != evidence.method {
                return Err(ApplyReportError::new(format!(
                    "ApplyReport method mismatch at edit {}",
                    expected_index
                )));
            }
            if evidence.placed && !evidence.matched {
                return Err(ApplyReportError::new(format!(
                    "ApplyReport edit {} was placed without a matched target",
                    expected_index
                )));
            }
            if let Some(profile_hash) = evidence.font_profile_sha256.as_deref() {
                validate_sha256("font_profile_sha256", profile_hash)?;
            }
        }

        let matched = self.edits.iter().filter(|edit| edit.matched).count();
        let placed = self.edits.iter().filter(|edit| edit.placed).count();
        let failed = self.edits.iter().filter(|edit| !edit.placed).count();
        if (self.matched, self.placed, self.failed) != (matched, placed, failed) {
            return Err(ApplyReportError::new(format!(
                "ApplyReport count mismatch: reported matched/placed/failed={}/{}/{}, derived={}/{}/{}",
                self.matched, self.placed, self.failed, matched, placed, failed
            )));
        }

        let exact_success = self.requested > 0
            && self.matched == self.requested
            && self.placed == self.requested
            && self.failed == 0
            && self.output_published
            && self.output_sha256.is_some();
        if self.success != exact_success {
            return Err(ApplyReportError::new(format!(
                "ApplyReport success={} but exact success evaluates to {}",
                self.success, exact_success
            )));
        }
        if !self.success && self.output_published {
            return Err(ApplyReportError::new(
                "failed ApplyReport must not publish an output",
            ));
        }
        validate_sha256("source_sha256", &self.source_sha256)?;
        if let Some(output_hash) = &self.output_sha256 {
            validate_sha256("output_sha256", output_hash)?;
        }
        Ok(())
    }

    pub fn verify_files(
        &self,
        source_path: &Path,
        output_path: &Path,
    ) -> Result<(), ApplyReportError> {
        let actual_source = sha256_file(source_path)?;
        if actual_source != self.source_sha256 {
            return Err(ApplyReportError::new(format!(
                "ApplyReport source hash mismatch: report={}, actual={actual_source}",
                self.source_sha256
            )));
        }

        if self.success {
            let expected_output = self.output_sha256.as_deref().ok_or_else(|| {
                ApplyReportError::new("successful ApplyReport is missing output_sha256")
            })?;
            let actual_output = sha256_file(output_path)?;
            if actual_output != expected_output {
                return Err(ApplyReportError::new(format!(
                    "ApplyReport output hash mismatch: report={expected_output}, actual={actual_output}"
                )));
            }
        } else if self.output_published || self.output_sha256.is_some() {
            return Err(ApplyReportError::new(
                "failed ApplyReport claims a published output",
            ));
        }
        Ok(())
    }
}

fn validate_sha256(field: &str, value: &str) -> Result<(), ApplyReportError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApplyReportError::new(format!(
            "ApplyReport {field} is not a lowercase/uppercase 64-character SHA-256 hex value"
        )));
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, ApplyReportError> {
    let mut file = File::open(path).map_err(|error| {
        ApplyReportError::new(format!(
            "failed to open {} for hashing: {error}",
            path.display()
        ))
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            ApplyReportError::new(format!(
                "failed to read {} for hashing: {error}",
                path.display()
            ))
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success_json() -> String {
        serde_json::json!({
            "schema_version": 1,
            "success": true,
            "requested": 1,
            "matched": 1,
            "placed": 1,
            "failed": 0,
            "warnings": [],
            "method_per_edit": ["embedded"],
            "review_flags": [],
            "source_sha256": "a".repeat(64),
            "output_sha256": "b".repeat(64),
            "output_published": true,
            "edits": [{
                "index": 0,
                "page": 0,
                "rect": [1.0, 2.0, 3.0, 4.0],
                "matched": true,
                "placed": true,
                "method": "embedded",
                "font_profile_sha256": null,
                "warning": null
            }]
        })
        .to_string()
    }

    #[test]
    fn parses_exact_success_report() {
        let report = ApplyReport::from_json_exact(&success_json(), 1).unwrap();
        assert!(report.success);
        assert_eq!(report.placed, 1);
    }

    #[test]
    fn rejects_unknown_fields() {
        let mut value: serde_json::Value = serde_json::from_str(&success_json()).unwrap();
        value["cached"] = serde_json::Value::Bool(true);
        let error = ApplyReport::from_json_exact(&value.to_string(), 1).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_partial_or_mismatched_counts() {
        let mut value: serde_json::Value = serde_json::from_str(&success_json()).unwrap();
        value["placed"] = serde_json::json!(0);
        let error = ApplyReport::from_json_exact(&value.to_string(), 1).unwrap_err();
        assert!(error.to_string().contains("count mismatch"));
    }

    #[test]
    fn rejects_success_without_published_output() {
        let mut value: serde_json::Value = serde_json::from_str(&success_json()).unwrap();
        value["output_published"] = serde_json::Value::Bool(false);
        value["output_sha256"] = serde_json::Value::Null;
        let error = ApplyReport::from_json_exact(&value.to_string(), 1).unwrap_err();
        assert!(error.to_string().contains("exact success"));
    }

    #[test]
    fn validates_optional_font_profile_hash() {
        let mut value: serde_json::Value = serde_json::from_str(&success_json()).unwrap();
        value["edits"][0]["font_profile_sha256"] = serde_json::json!("c".repeat(64));
        let report = ApplyReport::from_json_exact(&value.to_string(), 1).unwrap();
        assert_eq!(
            report.edits[0].font_profile_sha256.as_deref(),
            Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")
        );

        value["edits"][0]["font_profile_sha256"] = serde_json::json!("not-a-hash");
        let error = ApplyReport::from_json_exact(&value.to_string(), 1).unwrap_err();
        assert!(error.to_string().contains("font_profile_sha256"));
    }
}
