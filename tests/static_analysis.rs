use std::fs;

#[test]
fn test_no_pymupdf_in_split_merge() {
    let path = "src/engine/pdf_split_merge.rs";
    let content = fs::read_to_string(path).expect("Failed to read split_merge module");

    let restricted = ["pymupdf", "pyo3", "fitz", "pro.unlock", "Python"];

    for word in restricted {
        if content.to_lowercase().contains(&word.to_lowercase()) {
            panic!("Subsystem A (split_merge) MUST NOT use PyMuPDF or PyO3. Found restricted word: '{word}'");
        }
    }
}
