//! Date Period Adjustment.
//!
//! Bulk-shift or remap all transaction dates in a parsed statement.
//! Used by the "📅 Adjust Date Periods" popup in the GUI.

use crate::engine::model::Transaction;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Record of a single date shift applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateShiftRecord {
    pub page: usize,
    pub line_on_page: usize,
    pub old_date: String,
    pub new_date: String,
}

/// Mode of date adjustment.
#[derive(Debug, Clone)]
pub enum DateAdjustMode {
    /// Shift every date by a fixed number of days.
    ShiftDays(i64),
    /// Remap dates from one period to another, preserving relative offsets.
    RemapPeriod {
        from_start: NaiveDate,
        to_start: NaiveDate,
    },
}

/// Common date format patterns for parsing/formatting.
const DATE_FORMATS: &[&str] = &[
    "%d/%m/%Y",  // DD/MM/YYYY
    "%m/%d/%Y",  // MM/DD/YYYY
    "%Y-%m-%d",  // YYYY-MM-DD
    "%d-%m-%Y",  // DD-MM-YYYY
    "%m-%d-%Y",  // MM-DD-YYYY
    "%d %b %Y",  // 01 Jan 2026
    "%b %d, %Y", // Jan 01, 2026
    "%d/%m/%y",  // DD/MM/YY
    "%m/%d/%y",  // MM/DD/YY
];

/// Try to parse a date string using all known formats.
/// Returns the parsed date and the format string that worked.
pub fn parse_date(date_str: &str) -> Option<(NaiveDate, &'static str)> {
    let trimmed = date_str.trim();
    for &fmt in DATE_FORMATS {
        if let Ok(d) = NaiveDate::parse_from_str(trimmed, fmt) {
            return Some((d, fmt));
        }
    }
    None
}

/// Shift all transaction dates by a fixed number of days.
/// Returns a record of every shift applied.
pub fn shift_dates(transactions: &mut [Transaction], days: i64) -> Vec<DateShiftRecord> {
    let offset = chrono::Duration::days(days);
    let mut records = Vec::new();

    for tx in transactions.iter_mut() {
        if let Some((parsed, fmt)) = parse_date(&tx.date) {
            let new_date = parsed + offset;
            let new_date_str = new_date.format(fmt).to_string();
            records.push(DateShiftRecord {
                page: tx.page,
                line_on_page: tx.line_on_page,
                old_date: tx.date.clone(),
                new_date: new_date_str.clone(),
            });
            tx.date = new_date_str;
        }
    }

    records
}

/// Remap transaction dates from one period to another.
/// Each date's offset from `from_start` is preserved and applied relative to `to_start`.
/// For example, if `from_start` is Jan 1 and `to_start` is Feb 1, then Jan 5 -> Feb 5.
pub fn remap_date_period(
    transactions: &mut [Transaction],
    from_start: NaiveDate,
    to_start: NaiveDate,
) -> Vec<DateShiftRecord> {
    let mut records = Vec::new();

    for tx in transactions.iter_mut() {
        if let Some((parsed, fmt)) = parse_date(&tx.date) {
            let offset_days = (parsed - from_start).num_days();
            let new_date = to_start + chrono::Duration::days(offset_days);
            let new_date_str = new_date.format(fmt).to_string();
            records.push(DateShiftRecord {
                page: tx.page,
                line_on_page: tx.line_on_page,
                old_date: tx.date.clone(),
                new_date: new_date_str.clone(),
            });
            tx.date = new_date_str;
        }
    }

    records
}

/// Preview what the date shifts would look like without mutating.
pub fn preview_shift(transactions: &[Transaction], days: i64) -> Vec<DateShiftRecord> {
    let offset = chrono::Duration::days(days);
    let mut records = Vec::new();

    for tx in transactions.iter() {
        if let Some((parsed, fmt)) = parse_date(&tx.date) {
            let new_date = parsed + offset;
            records.push(DateShiftRecord {
                page: tx.page,
                line_on_page: tx.line_on_page,
                old_date: tx.date.clone(),
                new_date: new_date.format(fmt).to_string(),
            });
        }
    }

    records
}

/// Preview what a period remap would look like without mutating.
pub fn preview_remap(
    transactions: &[Transaction],
    from_start: NaiveDate,
    to_start: NaiveDate,
) -> Vec<DateShiftRecord> {
    let mut records = Vec::new();

    for tx in transactions.iter() {
        if let Some((parsed, fmt)) = parse_date(&tx.date) {
            let offset_days = (parsed - from_start).num_days();
            let new_date = to_start + chrono::Duration::days(offset_days);
            records.push(DateShiftRecord {
                page: tx.page,
                line_on_page: tx.line_on_page,
                old_date: tx.date.clone(),
                new_date: new_date.format(fmt).to_string(),
            });
        }
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::model::Provenance;
    use rust_decimal_macros::dec;

    fn make_tx(date: &str, page: usize, line: usize) -> Transaction {
        Transaction {
            page,
            line_on_page: line,
            date: date.to_string(),
            raw_text: String::new(),
            debit: Some(dec!(100)),
            credit: None,
            running_balance: Some(dec!(1000)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Manual,
            category: None,
            canonical: Default::default(),
        }
    }

    #[test]
    fn shift_dates_by_30_days() {
        let mut txns = vec![make_tx("15/01/2026", 0, 0), make_tx("20/01/2026", 0, 1)];
        let records = shift_dates(&mut txns, 30);
        assert_eq!(records.len(), 2);
        assert_eq!(txns[0].date, "14/02/2026");
        assert_eq!(txns[1].date, "19/02/2026");
    }

    #[test]
    fn shift_dates_negative() {
        let mut txns = vec![make_tx("15/03/2026", 0, 0)];
        let records = shift_dates(&mut txns, -15);
        assert_eq!(records.len(), 1);
        assert_eq!(txns[0].date, "28/02/2026");
    }

    #[test]
    fn remap_period_jan_to_feb() -> anyhow::Result<()> {
        let from =
            NaiveDate::from_ymd_opt(2026, 1, 1).ok_or_else(|| anyhow::anyhow!("Invalid date"))?;
        let to =
            NaiveDate::from_ymd_opt(2026, 2, 1).ok_or_else(|| anyhow::anyhow!("Invalid date"))?;
        let mut txns = vec![make_tx("05/01/2026", 0, 0), make_tx("25/01/2026", 0, 1)];
        let records = remap_date_period(&mut txns, from, to);
        assert_eq!(records.len(), 2);
        assert_eq!(txns[0].date, "05/02/2026");
        assert_eq!(txns[1].date, "25/02/2026");
        Ok(())
    }

    #[test]
    fn preview_does_not_mutate() {
        let txns = vec![make_tx("15/01/2026", 0, 0)];
        let records = preview_shift(&txns, 30);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].new_date, "14/02/2026");
        assert_eq!(txns[0].date, "15/01/2026"); // unchanged
    }

    #[test]
    fn parse_various_formats() {
        assert!(parse_date("15/01/2026").is_some());
        assert!(parse_date("01/15/2026").is_some());
        assert!(parse_date("2026-01-15").is_some());
        assert!(parse_date("garbage").is_none());
    }
}
