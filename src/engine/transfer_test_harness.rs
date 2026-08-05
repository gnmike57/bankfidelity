//! Cross-Statement Transfer Test Harness.
//!
//! Automated testing system that verifies the app can transfer transactions
//! between a set of bank statement PDFs in every possible direction, checks
//! fidelity, and loops until perfection or a retry ceiling is reached.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Result of a single directional transfer test (source -> target).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTestResult {
    pub source: PathBuf,
    pub target: PathBuf,
    pub output: PathBuf,
    /// How many correction iterations were needed.
    pub iterations: u32,
    pub final_math_ok: bool,
    pub final_visual_score: f64,
    /// Descriptions of corrections applied across iterations.
    pub corrections: Vec<String>,
    pub duration_secs: f64,
    /// Whether this pair converged to "perfect" within the retry limit.
    pub converged: bool,
}

/// Summary of a full N×(N−1) cross-test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestHarnessReport {
    pub timestamp: String,
    pub statement_count: usize,
    pub total_pairs: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<TransferTestResult>,
    pub total_duration_secs: f64,
}

impl TestHarnessReport {
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    pub fn summary(&self) -> String {
        format!(
            "{}/{} transfer pairs passed ({} failed) in {:.1}s",
            self.passed, self.total_pairs, self.failed, self.total_duration_secs,
        )
    }
}

/// Generate all N×(N−1) ordered pairs for cross-testing.
pub fn generate_test_pairs(statements: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
    let mut pairs = Vec::new();
    for (i, source) in statements.iter().enumerate() {
        for (j, target) in statements.iter().enumerate() {
            if i != j {
                pairs.push((source.clone(), target.clone()));
            }
        }
    }
    pairs
}

/// Generate the output path for a test transfer.
pub fn test_output_path(source: &std::path::Path, target: &std::path::Path) -> PathBuf {
    let dir = PathBuf::from("audit/transfer_tests/outputs");
    let _ = std::fs::create_dir_all(&dir);
    let source_stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let target_stem = target.file_stem().unwrap_or_default().to_string_lossy();
    dir.join(format!("{source_stem}__to__{target_stem}.pdf"))
}

/// Write the harness report to disk.
pub fn write_harness_report(report: &TestHarnessReport) -> std::io::Result<PathBuf> {
    let dir = PathBuf::from("audit/transfer_tests");
    std::fs::create_dir_all(&dir)?;
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let path = dir.join(format!("harness_{timestamp}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(report)?)?;
    Ok(path)
}

/// Build a `TestHarnessReport` from individual results.
pub fn build_report(
    results: Vec<TransferTestResult>,
    total_duration_secs: f64,
) -> TestHarnessReport {
    let passed = results
        .iter()
        .filter(|r| r.converged && r.final_math_ok)
        .count();
    let failed = results.len() - passed;
    TestHarnessReport {
        timestamp: chrono::Utc::now().to_rfc3339(),
        statement_count: {
            let mut all: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
            for r in &results {
                all.insert(r.source.clone());
                all.insert(r.target.clone());
            }
            all.len()
        },
        total_pairs: results.len(),
        passed,
        failed,
        results,
        total_duration_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_pairs_2_statements() {
        let stmts = vec![PathBuf::from("a.pdf"), PathBuf::from("b.pdf")];
        let pairs = generate_test_pairs(&stmts);
        assert_eq!(pairs.len(), 2); // a->b, b->a
        assert_eq!(pairs[0], (PathBuf::from("a.pdf"), PathBuf::from("b.pdf")));
        assert_eq!(pairs[1], (PathBuf::from("b.pdf"), PathBuf::from("a.pdf")));
    }

    #[test]
    fn generate_pairs_3_statements() {
        let stmts = vec![
            PathBuf::from("a.pdf"),
            PathBuf::from("b.pdf"),
            PathBuf::from("c.pdf"),
        ];
        let pairs = generate_test_pairs(&stmts);
        assert_eq!(pairs.len(), 6); // 3×2
    }

    #[test]
    fn build_report_counts() {
        let results = vec![
            TransferTestResult {
                source: PathBuf::from("a.pdf"),
                target: PathBuf::from("b.pdf"),
                output: PathBuf::from("out.pdf"),
                iterations: 1,
                final_math_ok: true,
                final_visual_score: 0.001,
                corrections: vec![],
                duration_secs: 5.0,
                converged: true,
            },
            TransferTestResult {
                source: PathBuf::from("b.pdf"),
                target: PathBuf::from("a.pdf"),
                output: PathBuf::from("out2.pdf"),
                iterations: 3,
                final_math_ok: false,
                final_visual_score: 0.05,
                corrections: vec!["balance fix".into()],
                duration_secs: 15.0,
                converged: false,
            },
        ];
        let report = build_report(results, 20.0);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert!(!report.all_passed());
        assert!(report.summary().contains("1/2"));
    }

    #[test]
    fn test_output_path_format() -> anyhow::Result<()> {
        let p = test_output_path(
            std::path::Path::new("statements/fidelity_jan.pdf"),
            std::path::Path::new("statements/chase_feb.pdf"),
        );
        let name = p
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("No file name"))?
            .to_string_lossy();
        assert!(name.contains("fidelity_jan"));
        assert!(name.contains("chase_feb"));
        assert!(name.ends_with(".pdf"));
        Ok(())
    }
}
