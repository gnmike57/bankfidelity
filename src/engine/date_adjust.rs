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
/// Ordered with Australian formats (DD/MM/YYYY) first to resolve ambiguous dates like 05/06/2026.
const DATE_FORMATS: &[&str] = &[
    "%d/%m/%Y",  // DD/MM/YYYY (Australian default)
    "%d-%m-%Y",  // DD-MM-YYYY
    "%d %b %Y",  // 01 Jan 2026
    "%d %B %Y",  // 01 January 2026
    "%d/%m/%y",  // DD/MM/YY
    "%d-%m-%y",  // DD-MM-YY
    "%Y-%m-%d",  // ISO YYYY-MM-DD
    "%m/%d/%Y",  // US MM/DD/YYYY
    "%m-%d-%Y",  // US MM-DD-YYYY
    "%b %d, %Y", // US Jan 01, 2026
    "%m/%d/%y",  // US MM/DD/YY
];

/// Formats without explicit year, requiring a statement year hint.
const DATE_FORMATS_NO_YEAR: &[&str] = &[
    "%d %b", // 15 Jan
    "%d %B", // 15 January
    "%d/%m", // 15/01
    "%d-%m", // 15-01
    "%b %d", // Jan 15
];

/// Try to parse a date string using all known formats.
/// Prioritizes Australian DD/MM ordering.
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

/// Try to parse a date string that may lack a year, using a supplied year hint.
/// Correctly handles bankwest-style inline dates where months may span December-January.
pub fn parse_date_with_year_hint(date_str: &str, year_hint: i32) -> Option<(NaiveDate, String)> {
    let trimmed = date_str.trim();
    if let Some((d, fmt)) = parse_date(trimmed) {
        return Some((d, fmt.to_string()));
    }

    for &fmt in DATE_FORMATS_NO_YEAR {
        let with_year = format!("{} {}", trimmed, year_hint);
        let full_fmt = format!("{} %Y", fmt);
        if let Ok(d) = NaiveDate::parse_from_str(&with_year, &full_fmt) {
            return Some((d, fmt.to_string()));
        }
    }
    None
}

/// Adds `months` to a `NaiveDate`, clamping the day to the last valid day of the target month.
/// For example, Jan 31 + 1 month -> Feb 28 (or Feb 29 in leap years).
pub fn add_months_clamped(date: NaiveDate, months: i32) -> NaiveDate {
    use chrono::Datelike;
    let total_months = (date.year() as i64) * 12 + (date.month0() as i64) + (months as i64);
    let target_year = (total_months / 12) as i32;
    let target_month0 = (total_months % 12) as u32;
    let target_month = target_month0 + 1;

    // Find the max days in target_month
    let target_day = date.day();
    let max_days = days_in_month(target_year, target_month);
    let clamped_day = target_day.min(max_days);

    NaiveDate::from_ymd_opt(target_year, target_month, clamped_day).unwrap_or(date)
}

/// Returns the number of days in a given year and month.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

    #[test]
    fn test_au_date_ambiguity_priority() {
        // "05/06/2026" should resolve to June 5th in AU format (DD/MM/YYYY), not May 6th
        let (d, fmt) = parse_date("05/06/2026").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 6, 5).unwrap());
        assert_eq!(fmt, "%d/%m/%Y");
    }

    #[test]
    fn test_parse_with_year_hint_for_inline_dates() {
        let (d1, _) = parse_date_with_year_hint("15 Jan", 2026).unwrap();
        assert_eq!(d1, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());

        let (d2, _) = parse_date_with_year_hint("31 Dec", 2025).unwrap();
        assert_eq!(d2, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    }

    #[test]
    fn test_add_months_clamped_rollover() {
        // Jan 31 + 1 month -> Feb 28 (2026 is not a leap year)
        let jan31 = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let feb28 = add_months_clamped(jan31, 1);
        assert_eq!(feb28, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());

        // Jan 31 + 1 month in leap year 2024 -> Feb 29
        let leap_jan31 = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();
        let feb29 = add_months_clamped(leap_jan31, 1);
        assert_eq!(feb29, NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());

        // March 31 - 1 month -> Feb 28
        let mar31 = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
        let feb_prev = add_months_clamped(mar31, -1);
        assert_eq!(feb_prev, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }
}
