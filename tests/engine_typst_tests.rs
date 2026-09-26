#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

#[tokio::test]
async fn test_chase_markup_header_labels_match_emitted_column_order() {
    let engine = TypstEngine::new();
    let stmt = BankStatement {
        total_pages: 1,
        transactions: vec![Transaction {
            page: 1,
            line_on_page: 1,
            raw_text: "Payroll Deposit".to_string(),
            date: "03/01".to_string(),
            debit: Some(dec!(25.00)),
            credit: Some(dec!(1000.00)),
            running_balance: Some(dec!(1975.00)),
            bbox: None,
            field_bboxes: FieldBboxes::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        }],
        opening_balance: dec!(1000.00),
        closing_balance: dec!(1975.00),
        account_number: Some("CHASE-999".to_string()),
        bank_name: Some("Chase".to_string()),
    };

    let markup = engine.generate_markup(&stmt);
    // Column header assertion: [*Credit*] is column 3, [*Debit*] is column 4
    assert!(
        markup.contains("[*Date*], [*Description*], [*Credit*], [*Debit*], [*Balance*]"),
        "Chase header must have [*Credit*] followed by [*Debit*], markup was:\n{markup}"
    );
    assert!(
        !markup.contains("[*Amount*]"),
        "Chase header must not contain stale [*Amount*]"
    );
    // Emitted data row: credit ($1000.00) is col 3, debit ($25.00) is col 4
    assert!(
        markup.contains(r#"[\$1000.00], [\$25.00], [\$1975.00]"#),
        "Chase data row must match column header order (credit, debit, bal), markup was:\n{markup}"
    );
}

#[tokio::test]
async fn test_typst_escaping_special_characters() {
    let engine = TypstEngine::new();
    let stmt = BankStatement {
        total_pages: 1,
        transactions: vec![Transaction {
            page: 1,
            line_on_page: 1,
            raw_text: "Coffee #1 \\ special *promo* _discount_ $10.00 [receipt]".to_string(),
            date: "03/01#promo".to_string(),
            debit: Some(dec!(10.00)),
            credit: None,
            running_balance: Some(dec!(990.00)),
            bbox: None,
            field_bboxes: FieldBboxes::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        }],
        opening_balance: dec!(1000.00),
        closing_balance: dec!(990.00),
        account_number: Some("CHASE-SPECIAL".to_string()),
        bank_name: Some("Chase".to_string()),
    };

    let markup = engine.generate_markup(&stmt);
    // Verify escaped characters are present in markup
    assert!(markup
        .contains("Coffee \\#1 \\\\ special \\*promo\\* \\_discount\\_ \\$10.00 \\[receipt\\]"));
    assert!(markup.contains("03/01\\#promo"));

    // Verify Typst compiler successfully parses and renders the escaped content without errors
    let temp_out = NamedTempFile::new().unwrap();
    let out_path = temp_out.path().to_path_buf();
    let result = engine.reconstruct_pdf(&stmt, &out_path).await;
    assert!(
        result.is_ok(),
        "Typst compilation failed on escaped content: {:?}",
        result.err()
    );
    assert!(out_path.exists());
    assert!(std::fs::metadata(&out_path).unwrap().len() > 0);
}
