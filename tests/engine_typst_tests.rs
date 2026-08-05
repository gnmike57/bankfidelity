use dual_core_pdf_pipeline::ai::document_ai::BankStatement;
use dual_core_pdf_pipeline::engine::model::{FieldBboxes, Provenance, Transaction};
use dual_core_pdf_pipeline::engine::typst_engine::TypstEngine;
use rust_decimal_macros::dec;
use tempfile::NamedTempFile;

#[test]
fn test_typst_engine_initialization() {
    let _engine = TypstEngine::new();
    // Engine should initialize correctly
}

#[tokio::test]
async fn test_generate_generic_markup() {
    let engine = TypstEngine::new();
    let stmt = BankStatement {
        total_pages: 1,
        transactions: vec![],
        opening_balance: dec!(100.00),
        closing_balance: dec!(200.00),
        account_number: Some("123456".to_string()),
        bank_name: Some("Unknown".to_string()),
    };

    let temp_out = NamedTempFile::new().unwrap();
    let out_path = temp_out.path().to_path_buf();

    let result = engine.reconstruct_pdf(&stmt, &out_path).await;
    assert!(result.is_ok());
    assert!(out_path.exists());
    let md = std::fs::metadata(&out_path).unwrap();
    assert!(md.len() > 0);
}

#[tokio::test]
async fn test_generate_chase_markup() {
    let engine = TypstEngine::new();
    let stmt = BankStatement {
        total_pages: 1,
        transactions: vec![Transaction {
            page: 1,
            line_on_page: 10,
            raw_text: "Target".to_string(),
            date: "02/15".to_string(),
            debit: Some(dec!(20.00)),
            credit: None,
            running_balance: Some(dec!(130.00)),
            bbox: None,
            field_bboxes: FieldBboxes::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        }],
        opening_balance: dec!(150.00),
        closing_balance: dec!(130.00),
        account_number: Some("CHASE-123".to_string()),
        bank_name: Some("Chase".to_string()),
    };

    let temp_out = NamedTempFile::new().unwrap();
    let out_path = temp_out.path().to_path_buf();

    let result = engine.reconstruct_pdf(&stmt, &out_path).await;
    assert!(result.is_ok());
    assert!(out_path.exists());
    let md = std::fs::metadata(&out_path).unwrap();
    assert!(md.len() > 0);
}

#[tokio::test]
async fn test_generate_bofa_markup() {
    let engine = TypstEngine::new();
    let stmt = BankStatement {
        total_pages: 1,
        transactions: vec![],
        opening_balance: dec!(0.00),
        closing_balance: dec!(0.00),
        account_number: None,
        bank_name: Some("Bank of America".to_string()),
    };

    let temp_out = NamedTempFile::new().unwrap();
    let out_path = temp_out.path().to_path_buf();

    let result = engine.reconstruct_pdf(&stmt, &out_path).await;
    assert!(result.is_ok());
    assert!(out_path.exists());
    let md = std::fs::metadata(&out_path).unwrap();
    assert!(md.len() > 0);
}
