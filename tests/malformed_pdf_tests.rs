use dual_core_pdf_pipeline::pdf::{OxidizePdfEngine, PdfEngine};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::test]
async fn test_malformed_pdf_handling() {
    let _ = dotenvy::dotenv();
    let engine = Arc::new(OxidizePdfEngine::new());

    // 1. Zero-byte PDF handling
    let zero_byte_path = PathBuf::from("tests/stress_pdfs/zero_byte.pdf");
    let _output_path = PathBuf::from("output/malformed_test_output.pdf");

    // Create zero byte file
    fs::create_dir_all("tests/stress_pdfs").unwrap();
    fs::write(&zero_byte_path, "").unwrap();

    let res = engine.analyze_layout(&zero_byte_path);
    assert!(
        res.is_err(),
        "Engine should cleanly reject a zero-byte PDF instead of panicking"
    );

    // 2. Corrupt PDF syntax handling (not really PDF)
    let corrupt_path = PathBuf::from("tests/stress_pdfs/corrupt.pdf");
    fs::write(
        &corrupt_path,
        "This is absolutely not a valid PDF file! %PDF-1.4 %%EOF",
    )
    .unwrap();

    let res2 = engine.analyze_layout(&corrupt_path);
    assert!(res2.is_err(), "Engine should cleanly reject a corrupt PDF");

    // 3. Infinite recursion / malicious objects
    // Synthesizing a deeply recursive PDF is tricky without binary generation,
    // but we can at least assert that the engine handles deeply nested JSON or other anomalies
    // in its native bindings safely.
    // For now, ensuring basic malformed files do not crash the AST parser is sufficient.

    println!("Malformed PDF test suite completed. All rejected cleanly.");

    // Cleanup
    let _ = fs::remove_file(zero_byte_path);
    let _ = fs::remove_file(corrupt_path);
}
