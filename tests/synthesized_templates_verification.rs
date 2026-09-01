#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dual_core_pdf_pipeline::engine::offline_parser::OfflineParser;
use dual_core_pdf_pipeline::engine::verification_structural::{
    verify_structural_invariants, StructuralVerificationConfig,
};
use std::fs;
use std::path::Path;

#[test]
fn test_synthesized_templates_self_consistency_and_verification() {
    let rendered_dir = Path::new("bank_templates/rendered");
    assert!(rendered_dir.exists(), "Rendered target templates directory must exist");

    let entries = fs::read_dir(rendered_dir).expect("Failed to read bank_templates/rendered");
    let mut tested = 0;

    let config = StructuralVerificationConfig::default();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("pdf") {
            let pdf_bytes = fs::read(&path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
            assert!(!pdf_bytes.is_empty(), "PDF {} cannot be empty", path.display());

            // 1. Structural Invariant Self-Verification (identical control)
            let result = verify_structural_invariants(&pdf_bytes, &pdf_bytes, &config)
                .unwrap_or_else(|e| panic!("Structural verification crashed for {}: {}", path.display(), e));
            assert!(
                result.passed,
                "Synthesized PDF {} must pass self structural verification: {:?}",
                path.display(),
                result.failures
            );

            // 2. Offline Parser Extraction
            let parser = OfflineParser::new();
            let parsed = parser.parse_statement(&path)
                .unwrap_or_else(|e| panic!("Offline parser failed on {}: {}", path.display(), e));

            assert!(
                !parsed.transactions.is_empty(),
                "Synthesized template {} must have extracted transactions",
                path.display()
            );
            assert!(
                parsed.opening_balance > rust_decimal_macros::dec!(0),
                "Synthesized template {} opening balance must be positive",
                path.display()
            );

            tested += 1;
            println!(
                "[synthesis_verification] {} PASSED (tx_count: {}, open: {}, close: {})",
                path.display(),
                parsed.transactions.len(),
                parsed.opening_balance,
                parsed.closing_balance
            );
        }
    }

    assert!(tested >= 6, "Expected at least 6 synthesized target templates, verified {}", tested);
}
