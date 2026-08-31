#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dual_core_pdf_pipeline::engine::balance::{
    auto_correct_final_balance_smart, diagnose_ledger_discrepancies, recalculate_and_validate,
};
use dual_core_pdf_pipeline::engine::model::{Provenance, Transaction};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Test1GroundTruth {
    opening_balance: f64,
    closing_balance: f64,
    transactions: Vec<Test1Tx>,
}

#[derive(Debug, Deserialize)]
struct Test1Tx {
    #[allow(dead_code)]
    line: usize,
    date: String,
    description: String,
    debit: Option<f64>,
    credit: Option<f64>,
    balance: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct Test3GroundTruth {
    opening_balance: f64,
    displayed_closing: f64,
    #[serde(default)]
    correct_closing: Option<f64>,
    #[serde(default)]
    #[allow(dead_code)]
    discrepancy: Option<f64>,
    #[serde(default)]
    error_introduced_at_line: Option<usize>,
    transactions: Vec<Test3Tx>,
}

#[derive(Debug, Deserialize)]
struct Test3Tx {
    #[allow(dead_code)]
    line: usize,
    date: String,
    description: String,
    debit: Option<f64>,
    credit: Option<f64>,
    #[serde(default)]
    displayed_balance: Option<f64>,
    #[serde(default)]
    #[allow(dead_code)]
    correct_balance: Option<f64>,
}

#[test]
fn test1_ground_truth_standard_bank_statement_balances() {
    let path = Path::new("tests/stress_pdfs/test1_ground_truth.json");
    if !path.exists() {
        eprintln!("[skip] test1_ground_truth.json not found");
        return;
    }

    let content = fs::read_to_string(path).expect("Failed to read test1 ground truth json");
    let gt: Test1GroundTruth =
        serde_json::from_str(&content).expect("Failed to parse test1 ground truth json");

    let opening = Decimal::from_f64_retain(gt.opening_balance)
        .unwrap()
        .round_dp(2);
    let expected_closing = Decimal::from_f64_retain(gt.closing_balance)
        .unwrap()
        .round_dp(2);

    // In python generator: credit = deposit (in), debit = withdrawal (out)
    // In dual-core engine: debit = in, credit = out
    let txs: Vec<Transaction> = gt
        .transactions
        .iter()
        .enumerate()
        .map(|(idx, t)| Transaction {
            page: 1,
            line_on_page: idx + 1,
            date: t.date.clone(),
            raw_text: t.description.clone(),
            debit: t
                .credit
                .and_then(|c| Decimal::from_f64_retain(c).map(|v| v.round_dp(2))),
            credit: t
                .debit
                .and_then(|d| Decimal::from_f64_retain(d).map(|v| v.round_dp(2))),
            running_balance: t
                .balance
                .and_then(|b| Decimal::from_f64_retain(b).map(|v| v.round_dp(2))),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        })
        .collect();

    let audit = diagnose_ledger_discrepancies(&txs, opening, Some(expected_closing));

    assert!(
        audit.is_balanced,
        "Test 1 ground truth must balance cleanly: {:?}",
        audit.diagnostic_message
    );
    assert_eq!(audit.line_discrepancies.len(), 0);
    assert_eq!(audit.first_imbalance_line, None);
    assert_eq!(audit.calculated_closing_balance, expected_closing);
}

#[test]
fn test3_ground_truth_unbalanced_ledger_identifies_exact_line_and_delta() {
    let path = Path::new("tests/stress_pdfs/test3_ground_truth.json");
    if !path.exists() {
        eprintln!("[skip] test3_ground_truth.json not found");
        return;
    }

    let content = fs::read_to_string(path).expect("Failed to read test3 ground truth json");
    let gt: Test3GroundTruth =
        serde_json::from_str(&content).expect("Failed to parse test3 ground truth json");

    let opening = Decimal::from_f64_retain(gt.opening_balance)
        .unwrap()
        .round_dp(2);
    let displayed_closing = Decimal::from_f64_retain(gt.displayed_closing)
        .unwrap()
        .round_dp(2);
    let correct_closing = gt
        .correct_closing
        .map(|c| Decimal::from_f64_retain(c).unwrap().round_dp(2));

    // Mapping Python credit (in) -> Rust debit (in), Python debit (out) -> Rust credit (out)
    let txs: Vec<Transaction> = gt
        .transactions
        .iter()
        .enumerate()
        .map(|(idx, t)| Transaction {
            page: 1,
            line_on_page: idx + 1,
            date: t.date.clone(),
            raw_text: t.description.clone(),
            debit: t
                .credit
                .and_then(|c| Decimal::from_f64_retain(c).map(|v| v.round_dp(2))),
            credit: t
                .debit
                .and_then(|d| Decimal::from_f64_retain(d).map(|v| v.round_dp(2))),
            running_balance: t
                .displayed_balance
                .and_then(|b| Decimal::from_f64_retain(b).map(|v| v.round_dp(2))),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        })
        .collect();

    let audit = diagnose_ledger_discrepancies(&txs, opening, Some(displayed_closing));

    // Test 3 has intentional error introduced at line 17 with discrepancy +$45.00
    assert!(
        !audit.is_balanced,
        "Test 3 must detect the intentional imbalance"
    );
    assert_eq!(
        audit.first_imbalance_line, gt.error_introduced_at_line,
        "First imbalance line must match ground truth line 17"
    );

    let first_disc = audit
        .line_discrepancies
        .first()
        .expect("Must have at least one line discrepancy");
    assert_eq!(first_disc.line_number, 17);
    assert_eq!(first_disc.discrepancy, dec!(45.00));

    // Calculated balance matches correct_closing
    if let Some(correct) = correct_closing {
        assert_eq!(audit.calculated_closing_balance, correct);
    }

    // Now verify that auto_correct_final_balance_smart resolves the ledger back to mathematical perfection
    let (repaired, correction_msg) =
        auto_correct_final_balance_smart(txs, opening, displayed_closing)
            .expect("Auto-correct solver must successfully repair the ledger");

    assert!(correction_msg.contains("MATH AUTO-CORRECTED"));

    // Recalculate repaired ledger
    let repaired_audit = diagnose_ledger_discrepancies(&repaired, opening, Some(displayed_closing));
    assert!(
        repaired_audit.is_balanced,
        "Repaired ledger must be perfectly balanced"
    );
    assert_eq!(repaired_audit.calculated_closing_balance, displayed_closing);
}

#[test]
fn test_exact_cent_1000_transaction_continuity() {
    let mut txs = Vec::with_capacity(1000);
    let mut current_expected = dec!(50000.00);

    for i in 0..1000 {
        // Pattern of credits and debits with irregular cents
        let (d, c) = if i % 3 == 0 {
            let val = dec!(142.87);
            current_expected += val;
            (Some(val), None)
        } else if i % 3 == 1 {
            let val = dec!(89.33);
            current_expected -= val;
            (None, Some(val))
        } else {
            let val = dec!(12.01);
            current_expected += val;
            (Some(val), None)
        };

        txs.push(Transaction {
            page: (i / 30) + 1,
            line_on_page: (i % 30) + 1,
            date: "2026-03-01".to_string(),
            raw_text: format!("Sequential Tx {}", i + 1),
            debit: d,
            credit: c,
            running_balance: None,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        });
    }

    let balanced =
        recalculate_and_validate(txs, dec!(50000.00)).expect("Recalculation must succeed");
    assert_eq!(
        balanced.last().unwrap().running_balance,
        Some(current_expected)
    );

    let audit = diagnose_ledger_discrepancies(&balanced, dec!(50000.00), Some(current_expected));
    assert!(audit.is_balanced);
    assert_eq!(audit.line_discrepancies.len(), 0);
}
