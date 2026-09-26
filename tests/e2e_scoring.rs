#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use dual_core_pdf_pipeline::engine::verification::{verify_edit_pages, MathInputs};
use rust_decimal_macros::dec;
use std::path::PathBuf;

#[tokio::test]
async fn test_e2e_pipeline_scoring() {
    let input = PathBuf::from("examples/sample.pdf");
    assert!(input.exists(), "examples/sample.pdf fixture must exist");

    let dir = tempfile::tempdir().unwrap();
    let math = MathInputs {
        transactions: vec![],
        expected_transactions: None,
        opening_balance: dec!(0.0),
        expected_final_balance: None,
        required: false,
    };

    let report = verify_edit_pages(&input, &input, dir.path(), &[], math, None, false, None)
        .await
        .expect("verify_edit_pages on real sample.pdf fixture must succeed");

    assert!(report.mandatory_local_pass());
    assert!(report.only_intended_changes);
    assert!(report.visual_diff_score < 0.01);
    assert!(report.min_ssim > 0.99);
}
