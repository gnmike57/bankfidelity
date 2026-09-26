#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dual_core_pdf_pipeline::ai::document_ai::BankStatement;
use dual_core_pdf_pipeline::engine::model::{Provenance, Transaction};
use dual_core_pdf_pipeline::engine::offline_parser::parse_statement_offline;
use dual_core_pdf_pipeline::engine::typst_engine::TypstEngine;
use dual_core_pdf_pipeline::engine::verification::VerificationGateStatus;
use dual_core_pdf_pipeline::engine::verification_structural::verify_structural_invariants;
use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn make_tx(
    page: usize,
    line: usize,
    date: &str,
    desc: &str,
    debit: Option<Decimal>,
    credit: Option<Decimal>,
    bal: Decimal,
) -> Transaction {
    Transaction {
        page,
        line_on_page: line,
        date: date.to_string(),
        raw_text: desc.to_string(),
        debit,
        credit,
        running_balance: Some(bal),
        bbox: None,
        field_bboxes: Default::default(),
        provenance: Provenance::Manual,
        category: None,
        canonical: Default::default(),
    }
}

fn synthesized_statements() -> Vec<(&'static str, BankStatement)> {
    vec![
        (
            "anz_plus_au",
            BankStatement {
                total_pages: 1,
                opening_balance: dec!(5420.50),
                closing_balance: dec!(6120.50),
                account_number: Some("012-345 67890123".to_string()),
                bank_name: Some("ANZ Plus".to_string()),
                transactions: vec![
                    make_tx(
                        1,
                        1,
                        "01/08/2026",
                        "Direct Credit - Payroll",
                        Some(dec!(1500.00)),
                        None,
                        dec!(6920.50),
                    ),
                    make_tx(
                        1,
                        2,
                        "03/08/2026",
                        "Coles Supermarkets",
                        None,
                        Some(dec!(125.40)),
                        dec!(6795.10),
                    ),
                    make_tx(
                        1,
                        3,
                        "05/08/2026",
                        "Woolworths Petrol",
                        None,
                        Some(dec!(85.00)),
                        dec!(6710.10),
                    ),
                    make_tx(
                        1,
                        4,
                        "10/08/2026",
                        "Transfer to Savings",
                        None,
                        Some(dec!(589.60)),
                        dec!(6120.50),
                    ),
                ],
            },
        ),
        (
            "bankwest_example",
            BankStatement {
                total_pages: 1,
                opening_balance: dec!(12000.00),
                closing_balance: dec!(11450.25),
                account_number: Some("302-111 9876543".to_string()),
                bank_name: Some("Bankwest".to_string()),
                transactions: vec![
                    make_tx(
                        1,
                        1,
                        "02/08/2026",
                        "Office Supplies Express",
                        None,
                        Some(dec!(245.50)),
                        dec!(11754.50),
                    ),
                    make_tx(
                        1,
                        2,
                        "04/08/2026",
                        "Client Payment - Invoice 104",
                        Some(dec!(850.00)),
                        None,
                        dec!(12604.50),
                    ),
                    make_tx(
                        1,
                        3,
                        "08/08/2026",
                        "ATO Business Activity Statement",
                        None,
                        Some(dec!(1154.25)),
                        dec!(11450.25),
                    ),
                ],
            },
        ),
        (
            "commbank_smartaccess_example",
            BankStatement {
                total_pages: 1,
                opening_balance: dec!(3250.00),
                closing_balance: dec!(3980.50),
                account_number: Some("062-000 12345678".to_string()),
                bank_name: Some("Commonwealth Bank".to_string()),
                transactions: vec![
                    make_tx(
                        1,
                        1,
                        "01 Aug 2026",
                        "SALARY PAYMENT ACME CORP",
                        Some(dec!(2100.00)),
                        None,
                        dec!(5350.00),
                    ),
                    make_tx(
                        1,
                        2,
                        "04 Aug 2026",
                        "NETFLIX AUSTRALIA SYDNEY",
                        None,
                        Some(dec!(22.99)),
                        dec!(5327.01),
                    ),
                    make_tx(
                        1,
                        3,
                        "09 Aug 2026",
                        "SYDNEY WATER UTILITIES",
                        None,
                        Some(dec!(346.51)),
                        dec!(4980.50),
                    ),
                    make_tx(
                        1,
                        4,
                        "12 Aug 2026",
                        "TRANSFER TO NETBANK SAVER",
                        None,
                        Some(dec!(1000.00)),
                        dec!(3980.50),
                    ),
                ],
            },
        ),
        (
            "ing_orange_au",
            BankStatement {
                total_pages: 1,
                opening_balance: dec!(450.00),
                closing_balance: dec!(1820.00),
                account_number: Some("923-100 55443322".to_string()),
                bank_name: Some("ING Orange Everyday".to_string()),
                transactions: vec![
                    make_tx(
                        1,
                        1,
                        "01/08/2026",
                        "Pay Anyone Transfer Received",
                        Some(dec!(2000.00)),
                        None,
                        dec!(2450.00),
                    ),
                    make_tx(
                        1,
                        2,
                        "03/08/2026",
                        "Bunnings Warehouse",
                        None,
                        Some(dec!(340.00)),
                        dec!(2110.00),
                    ),
                    make_tx(
                        1,
                        3,
                        "06/08/2026",
                        "JB Hi-Fi Electrical",
                        None,
                        Some(dec!(290.00)),
                        dec!(1820.00),
                    ),
                ],
            },
        ),
        (
            "macquarie_au",
            BankStatement {
                total_pages: 1,
                opening_balance: dec!(15400.00),
                closing_balance: dec!(16250.00),
                account_number: Some("182-500 88776655".to_string()),
                bank_name: Some("Macquarie Bank".to_string()),
                transactions: vec![
                    make_tx(
                        1,
                        1,
                        "02/08/2026",
                        "Dividend Reinvestment Macquarie",
                        Some(dec!(1250.00)),
                        None,
                        dec!(16650.00),
                    ),
                    make_tx(
                        1,
                        2,
                        "05/08/2026",
                        "Management Fee - Monthly",
                        None,
                        Some(dec!(400.00)),
                        dec!(16250.00),
                    ),
                ],
            },
        ),
        (
            "westpac_choice_basic_au",
            BankStatement {
                total_pages: 1,
                opening_balance: dec!(2890.00),
                closing_balance: dec!(3450.00),
                account_number: Some("032-001 44556677".to_string()),
                bank_name: Some("Westpac Choice".to_string()),
                transactions: vec![
                    make_tx(
                        1,
                        1,
                        "01/08/2026",
                        "DIRECT CREDIT SALARY",
                        Some(dec!(1800.00)),
                        None,
                        dec!(4690.00),
                    ),
                    make_tx(
                        1,
                        2,
                        "04/08/2026",
                        "TELSTRA TELECOM BILL",
                        None,
                        Some(dec!(140.00)),
                        dec!(4550.00),
                    ),
                    make_tx(
                        1,
                        3,
                        "08/08/2026",
                        "MORTGAGE OFFSET TRANSFER",
                        None,
                        Some(dec!(1100.00)),
                        dec!(3450.00),
                    ),
                ],
            },
        ),
    ]
}

async fn render_synthesized_templates() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create synthesized template temp dir");
    let rendered_dir = temp_dir.path();
    let engine = TypstEngine::new();

    for (bank_id, statement) in synthesized_statements() {
        let out_pdf = rendered_dir.join(format!("{bank_id}.pdf"));
        engine
            .reconstruct_pdf(&statement, &out_pdf)
            .await
            .unwrap_or_else(|e| panic!("Failed to synthesize {}: {}", out_pdf.display(), e));
    }

    temp_dir
}

#[tokio::test]
async fn test_synthesized_templates_self_consistency_and_verification() {
    // These PDFs are build artifacts, not source fixtures. Render them into an
    // isolated temp directory so the regression remains hermetic on clean
    // checkouts and CI workers where bank_templates/rendered is intentionally
    // absent.
    let temp_dir = render_synthesized_templates().await;
    let rendered_dir = temp_dir.path();
    assert_rendered_templates_self_consistent(rendered_dir);
}

fn assert_rendered_templates_self_consistent(rendered_dir: &Path) {
    assert!(
        rendered_dir.exists(),
        "Rendered target templates directory must exist"
    );

    let entries = fs::read_dir(rendered_dir).expect("Failed to read rendered templates directory");
    let mut tested = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("pdf") {
            let pdf_bytes = fs::read(&path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
            assert!(
                !pdf_bytes.is_empty(),
                "PDF {} cannot be empty",
                path.display()
            );

            // 1. Structural Invariant Self-Verification (identical control)
            let gates = verify_structural_invariants(&path, &path).unwrap_or_else(|e| {
                panic!(
                    "Structural verification crashed for {}: {}",
                    path.display(),
                    e
                )
            });
            for gate in &gates {
                assert_eq!(
                    gate.status,
                    VerificationGateStatus::Passed,
                    "Gate {} failed on {}: {}",
                    gate.id,
                    path.display(),
                    gate.message
                );
            }

            // 2. Offline Parser Extraction
            let engine = Arc::new(OxidizePdfEngine::new());
            let parsed = parse_statement_offline(&path, engine)
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

    assert!(
        tested >= 6,
        "Expected at least 6 synthesized target templates, verified {}",
        tested
    );
}
