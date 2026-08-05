use crate::ai::backend::AiBackend;
use crate::ai::document_ai::BankStatement;
use crate::app::config::AiProviderMode;
use crate::engine::balance::recalculate_and_validate;

pub async fn verify_and_repair_extraction(
    backend: &AiBackend,
    mut stmt: BankStatement,
    raw_ocr_text: &str,
) -> Result<BankStatement, String> {
    // 1. Check math locally
    let res = recalculate_and_validate(stmt.transactions.clone(), stmt.opening_balance);

    let mut needs_repair = false;
    let mut error_msg = String::new();

    match res {
        Ok(validated_tx) => {
            stmt.transactions = validated_tx;
            // Now check closing balance
            let closing = stmt.closing_balance;
            {
                let calc_closing = stmt
                    .transactions
                    .last()
                    .map(|tx| tx.running_balance.unwrap_or(stmt.opening_balance))
                    .unwrap_or(stmt.opening_balance);
                let diff = (closing - calc_closing).abs();
                if diff >= rust_decimal_macros::dec!(0.01) {
                    needs_repair = true;
                    error_msg = format!(
                        "Final balance mismatch. Expected: {}, Calculated: {}. Diff: {}",
                        closing, calc_closing, diff
                    );
                }
            }
        }
        Err(e) => {
            needs_repair = true;
            error_msg = e.to_string();
        }
    }

    if !needs_repair {
        return Ok(stmt);
    }

    if backend.primary == AiProviderMode::ManualOnly {
        tracing::warn!("[repair] AI repair skipped (Manual Only mode). Returning statement as-is.");
        return Ok(stmt);
    }

    tracing::warn!(
        "[repair] Extraction math verification failed: {}. Attempting AI repair...",
        error_msg
    );

    // 2. Call AI to repair the transactions based on raw OCR text
    let repaired_transactions = backend
        .repair_extracted_transactions(
            &stmt.transactions,
            stmt.opening_balance,
            stmt.closing_balance,
            raw_ocr_text,
            &error_msg,
        )
        .await
        .map_err(|e| format!("AI repair failed: {}", e))?;

    stmt.transactions = repaired_transactions;

    // Validate the repaired statement
    if let Err(e2) = recalculate_and_validate(stmt.transactions.clone(), stmt.opening_balance) {
        tracing::error!("[repair] AI repair failed to fix math: {}", e2);
    } else {
        tracing::info!("[repair] AI extraction repair was successful!");
    }

    Ok(stmt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::model::{Provenance, Transaction};
    use rust_decimal_macros::dec;

    fn default_tx(
        debit: Option<rust_decimal::Decimal>,
        credit: Option<rust_decimal::Decimal>,
        running: Option<rust_decimal::Decimal>,
    ) -> Transaction {
        Transaction {
            page: 1,
            line_on_page: 1,
            date: "2023-01-01".to_string(),
            raw_text: "test".to_string(),
            debit,
            credit,
            running_balance: running,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Computed,
            category: None,
            canonical: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_repair_not_needed() {
        let backend = AiBackend::new_mock();
        let stmt = BankStatement {
            total_pages: 1,
            transactions: vec![default_tx(Some(dec!(100)), None, Some(dec!(200)))],
            opening_balance: dec!(100),
            closing_balance: dec!(200),
            account_number: None,
            bank_name: None,
        };
        let res = verify_and_repair_extraction(&backend, stmt, "raw_text").await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_repair_needed_due_to_math() {
        let backend = AiBackend::new_mock();
        let stmt = BankStatement {
            total_pages: 1,
            transactions: vec![
                default_tx(Some(dec!(100)), None, Some(dec!(300))), // Math is wrong (100+100 = 200 != 300)
            ],
            opening_balance: dec!(100),
            closing_balance: dec!(300),
            account_number: None,
            bank_name: None,
        };
        let res = verify_and_repair_extraction(&backend, stmt, "raw_text").await;
        // Since it's a mock backend, it will fail to repair and return an Err
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("AI repair failed:"));
    }
}
