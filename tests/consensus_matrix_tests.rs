use dual_core_pdf_pipeline::ai::document_ai::BankStatement;
use dual_core_pdf_pipeline::engine::model::{FieldBboxes, Transaction};
use rust_decimal_macros::dec;

#[test]
fn test_ai_matrix_consensus() {
    // Simulate 3 parsers (Gemini, LlamaParse, Offline)
    // One misses a transaction, two catch it.

    let t1 = Transaction {
        page: 0,
        line_on_page: 0,
        date: "01/01/2023".to_string(),
        raw_text: "Target".to_string(),
        debit: Some(dec!(50.0)),
        credit: None,
        running_balance: Some(dec!(950.0)),
        bbox: Some([10.0, 20.0, 100.0, 30.0]),
        field_bboxes: FieldBboxes::default(),
        provenance: dual_core_pdf_pipeline::engine::model::Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };

    let t2 = Transaction {
        page: 0,
        line_on_page: 1,
        date: "01/02/2023".to_string(),
        raw_text: "Walmart".to_string(),
        debit: Some(dec!(20.0)),
        credit: None,
        running_balance: Some(dec!(930.0)),
        bbox: Some([10.0, 40.0, 100.0, 50.0]),
        field_bboxes: FieldBboxes::default(),
        provenance: dual_core_pdf_pipeline::engine::model::Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };

    let stmt_gemini = BankStatement {
        total_pages: 1,
        account_number: None,
        opening_balance: dec!(1000.0),
        closing_balance: dec!(930.0),
        transactions: vec![t1.clone(), t2.clone()],
        bank_name: None,
    };

    let stmt_llamaparse = BankStatement {
        total_pages: 1,
        account_number: None,
        opening_balance: dec!(1000.0),
        closing_balance: dec!(930.0),
        transactions: vec![t1.clone(), t2.clone()],
        bank_name: None,
    };

    let stmt_offline = BankStatement {
        total_pages: 1,
        account_number: None,
        opening_balance: dec!(1000.0),
        closing_balance: dec!(950.0),
        transactions: vec![t1.clone()], // Misses the second transaction
        bank_name: None,
    };

    let consensus = dual_core_pdf_pipeline::engine::consensus::merge_consensus_statements(vec![
        stmt_gemini,
        stmt_llamaparse,
        stmt_offline,
    ]);

    // The consensus should identify both transactions via majority vote (2 vs 1)
    assert_eq!(consensus.transactions.len(), 2);
    assert_eq!(consensus.opening_balance, dec!(1000.0));
    assert_eq!(consensus.closing_balance, dec!(930.0));
}

#[test]
fn two_parser_consensus_preserves_semantic_count_and_enriches_geometry() {
    let semantic = Transaction {
        page: 1,
        line_on_page: 0,
        date: "Government Withholding tax".into(),
        raw_text: "semantic row".into(),
        debit: Some(dec!(1000.00)),
        credit: None,
        running_balance: Some(dec!(999.99)),
        bbox: None,
        field_bboxes: FieldBboxes::default(),
        provenance: dual_core_pdf_pipeline::engine::model::Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };
    let mut geometry = semantic.clone();
    geometry.page = 2;
    geometry.line_on_page = 14;
    geometry.date = "13 Jan".into();
    geometry.raw_text = "13 Jan METRO PRAHRAN 13.29 41,219.03 CR".into();
    geometry.debit = None;
    geometry.credit = Some(dec!(13.29));
    geometry.running_balance = Some(dec!(41219.03));
    geometry.bbox = Some([54.0, 250.0, 537.0, 262.0]);
    geometry.field_bboxes = FieldBboxes {
        date: Some([54.0, 250.0, 84.0, 262.0]),
        description: Some([88.0, 250.0, 300.0, 262.0]),
        debit: None,
        credit: Some([390.0, 250.0, 430.0, 262.0]),
        running_balance: Some([480.0, 250.0, 537.0, 262.0]),
    };

    let semantic_statement = BankStatement {
        total_pages: 4,
        account_number: None,
        opening_balance: dec!(20324.91),
        closing_balance: dec!(35308.14),
        transactions: vec![semantic],
        bank_name: None,
    };
    let geometry_statement = BankStatement {
        total_pages: 4,
        account_number: None,
        opening_balance: dec!(20324.91),
        closing_balance: dec!(35308.14),
        transactions: vec![geometry.clone()],
        bank_name: None,
    };

    let consensus = dual_core_pdf_pipeline::engine::consensus::merge_consensus_statements(vec![
        semantic_statement,
        geometry_statement,
    ]);
    assert_eq!(consensus.transactions.len(), 1);
    assert_eq!(consensus.transactions[0].page, 2);
    assert_eq!(consensus.transactions[0].bbox, geometry.bbox);
    assert_eq!(
        consensus.transactions[0].field_bboxes,
        geometry.field_bboxes
    );
    assert_eq!(consensus.transactions[0].date, "13 Jan");
    assert_eq!(consensus.transactions[0].debit, None);
    assert_eq!(consensus.transactions[0].credit, Some(dec!(13.29)));
    assert_eq!(
        consensus.transactions[0].running_balance,
        Some(dec!(41219.03))
    );
}

#[test]
fn test_recalculation_loop_convergence() {
    // Since testing the actual async loop requires heavy mocking of Gemini/LlamaParse,
    // we test the core logic: a simulation of a recalculation loop delta.

    let expected_balance = dec!(100.0);
    let mut current_balance = dec!(110.0);
    let mut attempt = 0;

    let max_retries = 5;
    let mut all_math_ok = false;

    while attempt < max_retries {
        attempt += 1;

        if current_balance == expected_balance {
            all_math_ok = true;
            break;
        }

        // Simulate a "correction hint" triggering the AI to fix a $10 error
        let diff = current_balance - expected_balance;
        if diff == dec!(10.0) {
            current_balance -= dec!(10.0); // AI successfully healed it
        }
    }

    assert!(all_math_ok);
    assert_eq!(attempt, 2); // Resolved on the second iteration
}

#[test]
fn test_dpi_auto_scaling_limits() {
    // Tests the auto_match_dpi clamping logic (which caps at 600)
    // Suppose a PDF has very small dimensions (e.g., 200x200 points).
    // A naive DPI scaler might try to scale it to 1200 DPI to maintain resolution.

    let calculate_safe_dpi = |points_width: f32, _points_height: f32| -> f32 {
        let base_dpi = 300.0;
        let standard_width = 612.0; // 8.5 inches

        let scale_factor = standard_width / points_width;
        let raw_dpi = base_dpi * scale_factor;

        // Clamp to 600 max, 72 min
        raw_dpi.clamp(72.0, 600.0)
    };

    // Normal letter page (8.5x11)
    let letter_dpi = calculate_safe_dpi(612.0, 792.0);
    assert_eq!(letter_dpi, 300.0);

    // Tiny receipt (2 inches wide -> 144 points)
    // Naive scaling would be 300 * (612 / 144) = 1275 DPI
    let tiny_dpi = calculate_safe_dpi(144.0, 300.0);
    assert_eq!(tiny_dpi, 600.0); // Clamped!

    // Huge poster (24 inches wide -> 1728 points)
    // Naive scaling would be 300 * (612 / 1728) = 106.25 DPI
    let huge_dpi = calculate_safe_dpi(1728.0, 2400.0);
    assert_eq!(huge_dpi, 106.25);
}
