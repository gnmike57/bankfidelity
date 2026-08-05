use crate::engine::model::Transaction;
use std::collections::HashMap;

/// A simple heuristic-based categorizer for transactions.
pub fn categorize_transactions(transactions: &mut [Transaction]) {
    // A basic mapping of keywords to categories
    let mut keyword_map: HashMap<&str, &str> = HashMap::new();
    keyword_map.insert("uber", "Transport");
    keyword_map.insert("lyft", "Transport");
    keyword_map.insert("mcdonalds", "Food & Dining");
    keyword_map.insert("starbucks", "Food & Dining");
    keyword_map.insert("dunkin", "Food & Dining");
    keyword_map.insert("walmart", "Shopping");
    keyword_map.insert("target", "Shopping");
    keyword_map.insert("amazon", "Shopping");
    keyword_map.insert("netflix", "Entertainment");
    keyword_map.insert("spotify", "Entertainment");
    keyword_map.insert("shell", "Gas");
    keyword_map.insert("exxon", "Gas");
    keyword_map.insert("chevron", "Gas");
    keyword_map.insert("payroll", "Income");
    keyword_map.insert("salary", "Income");
    keyword_map.insert("deposit", "Income");

    for tx in transactions.iter_mut() {
        let desc = tx.raw_text.to_lowercase();
        let mut matched = false;

        for (keyword, category) in &keyword_map {
            if desc.contains(keyword) {
                tx.category = Some(category.to_string());
                matched = true;
                break;
            }
        }

        if !matched {
            // Check if it's an income based on debit amount (money in)
            if tx.delta_in() > rust_decimal::Decimal::ZERO {
                tx.category = Some("Income".to_string());
            } else {
                tx.category = Some("Uncategorized".to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::model::{Provenance, Transaction};
    use rust_decimal::Decimal;

    fn default_tx(desc: &str, delta_in: Decimal, delta_out: Decimal) -> Transaction {
        Transaction {
            page: 1,
            line_on_page: 1,
            date: "2023-01-01".to_string(),
            raw_text: desc.to_string(),
            debit: Some(delta_in),
            credit: Some(delta_out),
            running_balance: None,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Computed,
            category: None,
            canonical: Default::default(),
        }
    }

    #[test]
    fn test_categorize_transactions() {
        let mut txs = vec![
            default_tx("Uber ride", Decimal::ZERO, Decimal::new(15, 0)),
            default_tx("McDonalds", Decimal::ZERO, Decimal::new(10, 0)),
            default_tx("Salary deposit", Decimal::new(1000, 0), Decimal::ZERO),
            default_tx("Random store", Decimal::ZERO, Decimal::new(50, 0)),
            default_tx("Refund", Decimal::new(20, 0), Decimal::ZERO),
        ];

        categorize_transactions(&mut txs);

        assert_eq!(txs[0].category, Some("Transport".to_string()));
        assert_eq!(txs[1].category, Some("Food & Dining".to_string()));
        assert_eq!(txs[2].category, Some("Income".to_string()));
        assert_eq!(txs[3].category, Some("Uncategorized".to_string()));
        assert_eq!(txs[4].category, Some("Income".to_string()));
    }
}
