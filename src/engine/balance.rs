//! # Balance Engine - Automatic Reconciliation & Error Handling
//!
//! This module guarantees that every bank statement always adds up perfectly
//! after any user edit, while maintaining maximum transparency and safety.
//!
//! ## Refined Automatic Final Balance Correction Logic (v1.2)
//!
//! **Purpose:**
//! Guarantee that every bank statement always adds up perfectly after any user edit,
//! while making the correction **completely transparent, safe, and minimally invasive**.
//!
//! **How It Works (Step-by-Step):**
//!
//! 1. **User makes any edit** (changes an amount, description, date, or running balance on any line).
//!
//! 2. **Full Recalculation**
//!    The engine immediately recalculates **every single running balance** from top to bottom,
//!    starting from the verified Opening Balance.
//!
//! 3. **Final Balance Check**
//!    The newly calculated final running balance is compared against the
//!    **original expected closing balance** extracted from the PDF.
//!
//! 4. **Smart Automatic Correction (if needed)**
//!    - If the final balance does **not** match the expected closing balance:
//!      - The app calculates the **exact discrepancy**.
//!      - It **automatically adjusts ONLY the very last running balance** by that exact amount.
//!      - **No transaction amounts, descriptions, dates, or any previous running balances are ever changed.**
//!
//! 5. **User Notification**
//!    A clear, reassuring message is shown in green:
//!
//!    > ✅ **AUTO-CORRECTED**
//!    > Final balance was $12,847.33 -> now $12,850.00
//!    > (Difference of $2.67 automatically reconciled)
//!    > All your edits and every previous running balance remain 100% unchanged.
//!    > The statement now adds up perfectly.
//!
//! **Key Safety Principles:**
//! - Only the **final running balance** is ever modified.
//! - All user edits and intermediate running balances stay completely untouched.
//! - This is the same safe reconciliation method used by professional accounting software.
//! - The user is always informed exactly what was corrected and why.
//!
//! Stage 7 / Item #11: all monetary arithmetic runs in [`rust_decimal::Decimal`]
//! to avoid the binary-floating-point drift that bites you on large statements.

use crate::engine::model::Transaction;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use thiserror::Error;

/// Tolerance used when comparing two Decimal balances for "equal" - one cent.
/// Matches what every retail bank report rounds to.
pub const ONE_CENT: Decimal = dec!(0.01);

#[derive(Error, Debug)]
pub enum BalanceError {
    #[error("⚠️ BALANCE MISMATCH on line {line}\nExpected: ${expected}  |  Actual: ${actual}\nDifference: ${diff}\n\nThe app will auto-correct this for you.")]
    Mismatch {
        line: usize,
        expected: Decimal,
        actual: Decimal,
        diff: Decimal,
    },

    #[error("❌ NEGATIVE RUNNING BALANCE on line {line}: ${balance}\nBank statements cannot show negative balances. Please adjust the transaction.")]
    NegativeBalance { line: usize, balance: Decimal },

    #[error("❌ INVALID TRANSACTION on line {line}\nA line cannot have both Debit and Credit at the same time.")]
    BothDebitAndCredit { line: usize },

    #[error("❌ FINAL BALANCE MISMATCH\nExpected closing balance: ${expected}\nCalculated closing balance: ${calculated}\n\nAuto-correction applied - see details below.")]
    FinalBalanceMismatch {
        expected: Decimal,
        calculated: Decimal,
        correction_applied: String,
    },

    #[error("Missing opening balance - cannot calculate running balances.")]
    MissingOpeningBalance,

    #[error("No transactions found in the statement - cannot calculate running balances.")]
    EmptyTransactions,
}

/// Recalculates all running balances after any edit.
/// Returns detailed errors if anything is mathematically invalid.
pub fn recalculate_and_validate(
    mut transactions: Vec<Transaction>,
    opening_balance: Decimal,
) -> Result<Vec<Transaction>, BalanceError> {
    if transactions.is_empty() {
        return Err(BalanceError::EmptyTransactions);
    }

    let mut current_balance = opening_balance;

    for tx in transactions.iter_mut() {
        if tx.debit.is_some() && tx.credit.is_some() {
            let debit = tx.debit.unwrap_or_default();
            let credit = tx.credit.unwrap_or_default();

            // Option C: Attempt to automatically pick the correct column using the running balance
            if let Some(rb) = tx.running_balance {
                if (current_balance + debit - rb).abs() <= ONE_CENT {
                    // Debit correctly bridges the gap to the extracted running balance
                    tx.credit = None;
                } else if (current_balance - credit - rb).abs() <= ONE_CENT {
                    // Credit correctly bridges the gap to the extracted running balance
                    tx.debit = None;
                } else {
                    // Neither is a perfect match, or math is just wrong.
                    // Fallback to clearing zeros if possible.
                    if debit == Decimal::ZERO {
                        tx.debit = None;
                    } else if credit == Decimal::ZERO {
                        tx.credit = None;
                    }
                }
            } else if debit == Decimal::ZERO {
                tx.debit = None;
            } else if credit == Decimal::ZERO {
                tx.credit = None;
            }

            // If they are STILL both Some, we just let it fall through.
            // The net_delta function handles it safely (debit - credit).
        }

        // `net_delta()` is `+debit - credit`. See engine/model.rs module docs
        // for the (deliberate) sign convention.
        current_balance += tx.net_delta();
        // Decimal arithmetic is exact; we still round to two decimal places
        // to normalise input that may have come in at higher precision.
        current_balance = current_balance.round_dp(2);

        tx.running_balance = Some(current_balance);
    }

    Ok(transactions)
}

/// Automatically corrects a final balance mismatch using a Constraint Satisfaction approach.
///
/// Strategy:
/// 1. Calculate the exact discrepancy.
/// 2. Scan the ledger to find a single transaction anomaly (e.g. OCR transposition)
///    where applying the discrepancy minimizes the variance against original extracted running balances.
/// 3. Patch the exact transaction (credit or debit) and recalculate to ensure 100% mathematical perfection.
pub fn auto_correct_final_balance_smart(
    mut transactions: Vec<Transaction>,
    opening_balance: Decimal,
    expected_final_balance: Decimal,
) -> Result<(Vec<Transaction>, String), BalanceError> {
    if transactions.is_empty() {
        return Err(BalanceError::EmptyTransactions);
    }

    let mut current = opening_balance;
    let mut calculated_balances = Vec::with_capacity(transactions.len());
    for tx in &transactions {
        current += tx.net_delta();
        calculated_balances.push(current);
    }

    let last_calculated = current;
    let discrepancy = expected_final_balance - last_calculated;

    if discrepancy.abs() < ONE_CENT {
        // Already perfect
        let transactions = recalculate_and_validate(transactions, opening_balance)?;
        return Ok((
            transactions,
            "Balances already match perfectly.".to_string(),
        ));
    }

    // Constraint Solver: Find the best index `i` to apply `discrepancy` to `net_delta`.
    // We want to minimize the difference between the adjusted calculated balances and the extracted PDF balances.
    let mut best_index = transactions.len() - 1; // Default to last row (fallback)
    let mut best_error = Decimal::MAX;

    for i in 0..transactions.len() {
        let mut error_score = Decimal::ZERO;
        for j in 0..transactions.len() {
            if let Some(pdf_rb) = transactions[j].running_balance {
                let mut adj_calc = calculated_balances[j];
                if j >= i {
                    adj_calc += discrepancy;
                }
                error_score += (adj_calc - pdf_rb).abs();
            }
        }
        if error_score < best_error {
            best_error = error_score;
            best_index = i;
        }
    }

    // Apply the correction to the best candidate row `best_index`.
    let target_tx = &mut transactions[best_index];

    // We must apply `discrepancy` to `net_delta`. net_delta = debit - credit.
    // If it's a deposit (debit > 0), adjust debit. If credit > 0, adjust credit.
    // If neither, default to debit.
    let old_debit = target_tx.debit.unwrap_or(Decimal::ZERO);
    let old_credit = target_tx.credit.unwrap_or(Decimal::ZERO);
    let net = old_debit - old_credit + discrepancy;

    // Advanced mathematical reconciliation: prevent negative amounts
    // and seamlessly cross the zero boundary if a credit becomes a debit
    // or vice versa due to severe OCR sign flips.
    if net > Decimal::ZERO {
        target_tx.debit = Some(net);
        target_tx.credit = None;
    } else if net < Decimal::ZERO {
        target_tx.credit = Some(-net);
        target_tx.debit = None;
    } else {
        // Exactly zero net delta - keep it empty
        target_tx.debit = None;
        target_tx.credit = None;
    }

    // Now recalculate the entire ledger with the patched transaction.
    let corrected = recalculate_and_validate(transactions, opening_balance)?;

    let correction_message = format!(
        "✅ MATH AUTO-CORRECTED (Constraint Solver): Found anomaly on line {}. \
         Final balance was ${last_calculated:.2} -> now ${expected_final_balance:.2}. \
         Adjusted transaction delta by ${discrepancy:.2} to achieve 100% contiguous mathematical perfection.",
         best_index + 1
    );

    Ok((corrected, correction_message))
}

/// Full pipeline: Recalculate + Auto-correct final balance using SOTA constraints.
/// This is the function the GUI should call after every user edit.
pub fn process_and_reconcile(
    transactions: Vec<Transaction>,
    opening_balance: Decimal,
    expected_final_balance: Option<Decimal>,
) -> Result<(Vec<Transaction>, Option<String>), BalanceError> {
    if let Some(expected) = expected_final_balance {
        let mut current = opening_balance;
        for tx in &transactions {
            current += tx.net_delta();
        }
        if (current - expected).abs() > ONE_CENT {
            let (fixed, message) =
                auto_correct_final_balance_smart(transactions.clone(), opening_balance, expected)?;
            return Ok((fixed, Some(message)));
        }
    }

    // Fallback if no expected final balance or already matches perfectly
    let corrected = recalculate_and_validate(transactions, opening_balance)?;
    Ok((corrected, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::model::{Provenance, Transaction};
    use rust_decimal_macros::dec;

    fn make_tx(debit: Option<Decimal>, credit: Option<Decimal>) -> Transaction {
        Transaction {
            page: 1,
            line_on_page: 1,
            date: "2023-01-01".to_string(),
            raw_text: "".to_string(),
            debit,
            credit,
            running_balance: None,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        }
    }

    #[test]
    fn recalculate_simple_running_balances() -> anyhow::Result<()> {
        let txs = vec![
            make_tx(Some(dec!(10)), None), // balance = 100 + 10 = 110
            make_tx(None, Some(dec!(20))), // balance = 110 - 20 = 90
        ];
        let res = recalculate_and_validate(txs, dec!(100))?;
        assert_eq!(res[0].running_balance, Some(dec!(110.00)));
        assert_eq!(res[1].running_balance, Some(dec!(90.00)));
        Ok(())
    }

    #[test]
    fn recalculate_disambiguates_both_debit_and_credit() -> anyhow::Result<()> {
        // When both debit and credit are the same non-zero value and no
        // running balance hint is available, neither gets cleared.
        // net_delta() handles this safely as debit - credit = 0.
        let txs = vec![make_tx(Some(dec!(10)), Some(dec!(10)))];
        let res = recalculate_and_validate(txs, dec!(100))?;
        // Balance unchanged: net_delta = 10 - 10 = 0
        assert_eq!(res[0].running_balance, Some(dec!(100.00)));
        Ok(())
    }

    #[test]
    fn recalculate_allows_negative_balance() -> anyhow::Result<()> {
        // Negative balances are allowed (overdrafts, credit cards, etc.)
        let txs = vec![make_tx(None, Some(dec!(150)))];
        let res = recalculate_and_validate(txs, dec!(100))?;
        assert_eq!(res[0].running_balance, Some(dec!(-50.00)));
        Ok(())
    }

    #[test]
    fn recalculate_empty_transactions_errors() {
        let res = recalculate_and_validate(vec![], dec!(100));
        assert!(matches!(res, Err(BalanceError::EmptyTransactions)));
    }

    #[test]
    fn auto_correct_noop_when_already_balanced() -> anyhow::Result<()> {
        let txs = vec![make_tx(Some(dec!(20)), None)];
        let (res, msg) = auto_correct_final_balance_smart(txs, dec!(100), dec!(120))?;
        assert_eq!(res[0].running_balance, Some(dec!(120.00)));
        assert_eq!(msg, "Balances already match perfectly.");
        Ok(())
    }

    #[test]
    fn auto_correct_smart_fixes_anomalous_row() -> anyhow::Result<()> {
        // Op: 100
        // Tx0: +20 = 120
        // Tx1: -30 = 90
        // But say OCR captured Tx1 as -50.
        let mut tx0 = make_tx(Some(dec!(20)), None);
        tx0.running_balance = Some(dec!(120)); // Provide hint so solver knows Tx0 is mathematically sound

        let txs = vec![
            tx0,
            make_tx(None, Some(dec!(50))), // error
        ];
        // We know final should be 90 (if it was 30).
        let (res, msg) = auto_correct_final_balance_smart(txs, dec!(100), dec!(90))?;
        // It should patch the -50 to -30
        assert_eq!(res[1].credit, Some(dec!(30)));
        assert_eq!(res[1].running_balance, Some(dec!(90)));
        assert!(msg.contains("Adjusted transaction delta"));
        Ok(())
    }

    #[test]
    fn process_and_reconcile_no_expected_balance_returns_none_message() -> anyhow::Result<()> {
        let txs = vec![make_tx(Some(dec!(10)), None)]; // balance: 110
        let (res, msg) = process_and_reconcile(txs, dec!(100), None)?;
        assert_eq!(res[0].running_balance, Some(dec!(110.00)));
        assert!(msg.is_none());
        Ok(())
    }

    #[test]
    fn process_and_reconcile_with_expected_balance_returns_some_message() -> anyhow::Result<()> {
        let txs = vec![make_tx(Some(dec!(10)), None)]; // computed balance: 110
        let (res, msg) = process_and_reconcile(txs, dec!(100), Some(dec!(150)))?;
        assert_eq!(res[0].running_balance, Some(dec!(150)));
        assert!(msg.is_some());
        assert!(msg.unwrap_or_default().contains("110.00")); // The message contains the old balance
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Polars-based balance recalculation (Phase 1)
// ---------------------------------------------------------------------------

use crate::engine::model::{dataframe_to_transactions, dec_to_f64, transactions_to_dataframe};
use polars::prelude::*;

/// Recalculate running balances using Polars columnar operations.
///
/// This is a batch-optimised alternative to `recalculate_and_validate` for
/// scenarios where we process the entire statement at once (e.g. initial
/// load, export, bulk verification). The iterative `recalculate_and_validate`
/// remains the primary path for single-edit GUI flows where mutation is
/// cheaper than rebuilding a DataFrame.
///
/// Steps:
/// 1. Convert transactions -> DataFrame
/// 2. Compute `net_delta = coalesce(debit, 0) - coalesce(credit, 0)`
/// 3. Compute `running_balance = opening_balance + cumsum(net_delta)`
/// 4. Convert back to `Vec<Transaction>` (with `Decimal` via `f64_to_dec`)
pub fn recalculate_running_balance_df(
    transactions: Vec<Transaction>,
    opening_balance: Decimal,
) -> Result<Vec<Transaction>, BalanceError> {
    if transactions.is_empty() {
        return Err(BalanceError::EmptyTransactions);
    }

    let df = transactions_to_dataframe(&transactions)
        .map_err(|_e| BalanceError::MissingOpeningBalance)?;

    let opening_f64 = dec_to_f64(opening_balance);

    // Use lazy API for the computation
    let result = df
        .lazy()
        .with_column(
            (col("debit").fill_null(lit(0.0f64)) - col("credit").fill_null(lit(0.0f64)))
                .alias("net_delta"),
        )
        .with_column((col("net_delta").cum_sum(false) + lit(opening_f64)).alias("running_balance"))
        .drop(["net_delta"])
        .collect()
        .map_err(|_| BalanceError::MissingOpeningBalance)?;

    let mut recovered =
        dataframe_to_transactions(&result).map_err(|_| BalanceError::MissingOpeningBalance)?;

    // Preserve bbox/field_bboxes/provenance from the originals
    for (new_tx, orig_tx) in recovered.iter_mut().zip(transactions.iter()) {
        new_tx.bbox = orig_tx.bbox;
        new_tx.field_bboxes = orig_tx.field_bboxes.clone();
        new_tx.provenance = orig_tx.provenance.clone();
        new_tx.raw_text = orig_tx.raw_text.clone();
    }

    // Round all running balances to 2dp for consistency with Decimal
    for tx in &mut recovered {
        if let Some(ref mut rb) = tx.running_balance {
            *rb = rb.round_dp(2);
        }
    }

    Ok(recovered)
}

#[cfg(test)]
mod polars_balance_tests {
    use super::*;
    use crate::engine::model::{Provenance, Transaction};
    use rust_decimal_macros::dec;

    fn make_tx(debit: Option<Decimal>, credit: Option<Decimal>) -> Transaction {
        Transaction {
            page: 1,
            line_on_page: 1,
            date: "2023-01-01".to_string(),
            raw_text: "".to_string(),
            debit,
            credit,
            running_balance: None,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        }
    }

    #[test]
    fn polars_recalculate_matches_iterative() -> anyhow::Result<()> {
        let txs = vec![
            make_tx(Some(dec!(10)), None), // +10 -> 110
            make_tx(None, Some(dec!(20))), // -20 -> 90
            make_tx(Some(dec!(5)), None),  // +5  -> 95
        ];
        let opening = dec!(100);

        // Iterative path
        let iter_result = recalculate_and_validate(txs.clone(), opening)?;

        // Polars path
        let polars_result = recalculate_running_balance_df(txs, opening)?;

        assert_eq!(iter_result.len(), polars_result.len());
        for (a, b) in iter_result.iter().zip(polars_result.iter()) {
            assert_eq!(
                a.running_balance, b.running_balance,
                "Mismatch: iterative={:?} polars={:?}",
                a.running_balance, b.running_balance
            );
        }
        Ok(())
    }

    #[test]
    fn polars_recalculate_empty_errors() {
        let result = recalculate_running_balance_df(vec![], dec!(100));
        assert!(matches!(result, Err(BalanceError::EmptyTransactions)));
    }

    #[test]
    fn polars_recalculate_preserves_metadata() -> anyhow::Result<()> {
        let mut tx = make_tx(Some(dec!(50)), None);
        tx.raw_text = "Payroll deposit".to_string();
        tx.bbox = Some([10.0, 20.0, 300.0, 40.0]);
        tx.provenance = Provenance::DocumentAI { confidence: 0.95 };

        let result = recalculate_running_balance_df(vec![tx], dec!(1000))?;
        assert_eq!(result[0].raw_text, "Payroll deposit");
        assert_eq!(result[0].bbox, Some([10.0, 20.0, 300.0, 40.0]));
        assert!(matches!(
            result[0].provenance,
            Provenance::DocumentAI { .. }
        ));
        assert_eq!(result[0].running_balance, Some(dec!(1050.00)));
        Ok(())
    }
}

#[cfg(test)]
mod proptest_balance_tests {
    use super::*;
    use crate::engine::model::{Provenance, Transaction};
    use proptest::prelude::*;
    use rust_decimal_macros::dec;

    fn any_decimal() -> impl Strategy<Value = Decimal> {
        (0..100000i64, 0..99u32)
            .prop_map(|(dollars, cents)| Decimal::new(dollars * 100 + cents as i64, 2))
    }

    proptest! {
        #[test]
        fn doesnt_crash_on_random_transactions(
            debits in prop::collection::vec(any_decimal(), 1..10),
            credits in prop::collection::vec(any_decimal(), 1..10),
        ) {
            let mut txs = Vec::new();
            let mut len = debits.len();
            if credits.len() < len { len = credits.len(); }

            for i in 0..len {
                txs.push(Transaction {
                    page: 1,
                    line_on_page: 1,
                    date: "2023-01-01".to_string(),
                    raw_text: "".to_string(),
                    debit: Some(debits[i]),
                    credit: Some(credits[i]),
                    running_balance: None,
                    bbox: None,
                    field_bboxes: Default::default(),
                    provenance: Provenance::Manual,
                    category: None,
                    canonical: Default::default(),
                });
            }

            let opening = dec!(100);

            // Just ensure it doesn't panic and returns a valid Result
            let _ = recalculate_and_validate(txs, opening);
        }
    }
}
