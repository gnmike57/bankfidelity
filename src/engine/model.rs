//! Core transaction model.
//!
//! # Sign convention (important)
//!
//! Throughout this codebase the running ledger is computed as:
//!
//! ```text
//! new_balance = old_balance + debit - credit
//! ```
//!
//! That is, **`debit` adds to the balance and `credit` subtracts**. This is
//! the inverse of formal double-entry accounting (where a credit increases an
//! asset/liability balance and a debit reduces it). We made this choice
//! historically because most retail bank statements show "Debit" as the
//! "money in" column for the *customer's* account, and "Credit" as
//! "money out" - these field names match what the user sees on their
//! statement, not the accountant-side journal.
//!
//! When you need to think in formal accounting terms, use [`Transaction::delta_in`]
//! and [`Transaction::delta_out`], which are unambiguous regardless of which
//! convention you grew up with.
//!
//! # Why `Decimal` and not `f64`
//!
//! Stage 7 / Item #11. Bank statements use exact decimal arithmetic; running
//! the cascade in `f64` accumulates representation drift across hundreds of
//! rows (the classic `0.1 + 0.2 != 0.3` problem). We use [`rust_decimal::Decimal`]
//! end-to-end for any monetary field, and only cross to `f64` at the GUI
//! plotting / image-pixel boundary where the precision doesn't matter.
//!
//! Helpers [`dec_to_f64`] and [`f64_to_dec`] convert at those edges.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// One row of a bank statement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub page: usize,
    pub line_on_page: usize,
    pub date: String,
    pub raw_text: String,
    /// Money flowing **into** the account on this row (in the customer's view).
    /// Adds to the running balance. See module docs for the sign convention.
    pub debit: Option<Decimal>,
    /// Money flowing **out of** the account on this row (in the customer's view).
    /// Subtracts from the running balance. See module docs for the sign convention.
    pub credit: Option<Decimal>,
    pub running_balance: Option<Decimal>,
    /// Bbox of the entire row in PDF points (x0,y0,x1,y1).
    pub bbox: Option<[f32; 4]>,
    /// Per-field bboxes (Date, Description, Debit, Credit, RunningBalance).
    /// When present, an edit on a specific field uses that bbox instead of
    /// the row-level `bbox`. Stage 7.5 - without these the binary editor
    /// would redact the entire row when the user only changed one cell.
    #[serde(default, skip_serializing_if = "FieldBboxes::is_empty")]
    pub field_bboxes: FieldBboxes,
    pub provenance: Provenance,

    /// Auto-categorization label (e.g. "Food", "Travel").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    /// Canonical extraction metadata shared by every parser and workflow.
    #[serde(default, skip_serializing_if = "CanonicalMetadata::is_empty")]
    pub canonical: CanonicalMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CanonicalMetadata {
    /// Stable identity inside the source document, normally `p{page}:r{line}`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stable_row_id: String,
    /// ISO 4217 currency code when known (for example `AUD`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// BCP 47 locale tag when known (for example `en-AU`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Parser confidence in the semantic row, constrained to 0..=1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// True when this row must be reviewed before automatic editing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub review_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl CanonicalMetadata {
    pub fn for_row(page: usize, line_on_page: usize, confidence: Option<f32>) -> Self {
        Self {
            stable_row_id: format!("p{page}:r{line_on_page}"),
            confidence: confidence.map(|value| value.clamp(0.0, 1.0)),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stable_row_id.is_empty()
            && self.currency.is_none()
            && self.locale.is_none()
            && self.confidence.is_none()
            && !self.review_required
            && self.review_reason.is_none()
    }
}

/// Per-field bounding boxes for a single transaction row. All bboxes are
/// in PDF points (origin top-left of page); `None` means the field wasn't
/// present in this row (e.g. a debit-only row has no `credit` bbox).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FieldBboxes {
    pub date: Option<[f32; 4]>,
    pub description: Option<[f32; 4]>,
    pub debit: Option<[f32; 4]>,
    pub credit: Option<[f32; 4]>,
    pub running_balance: Option<[f32; 4]>,
}

impl FieldBboxes {
    pub fn is_empty(&self) -> bool {
        self.date.is_none()
            && self.description.is_none()
            && self.debit.is_none()
            && self.credit.is_none()
            && self.running_balance.is_none()
    }
}

impl Transaction {
    /// Money flowing into the account on this row. Positive contribution to
    /// the running balance. Returns `0` when not present.
    #[inline]
    pub fn delta_in(&self) -> Decimal {
        self.debit.unwrap_or(Decimal::ZERO)
    }

    /// Money flowing out of the account on this row. Positive contribution
    /// to the *out* side (i.e. subtracted from the running balance).
    /// Returns `0` when not present.
    #[inline]
    pub fn delta_out(&self) -> Decimal {
        self.credit.unwrap_or(Decimal::ZERO)
    }

    /// Net change in the running balance contributed by this transaction.
    /// Positive = balance grows, negative = balance shrinks.
    #[inline]
    pub fn net_delta(&self) -> Decimal {
        self.delta_in() - self.delta_out()
    }

    /// Fill canonical metadata deterministically for legacy/parser rows while
    /// preserving any explicit currency, locale, confidence, or review flag.
    pub fn ensure_canonical_metadata(&mut self) {
        if self.canonical.stable_row_id.is_empty() {
            self.canonical.stable_row_id = format!("p{}:r{}", self.page, self.line_on_page);
        }
        if self.canonical.confidence.is_none() {
            self.canonical.confidence = Some(match self.provenance {
                Provenance::DocumentAI { confidence } => confidence.clamp(0.0, 1.0),
                Provenance::Manual => 1.0,
                Provenance::Computed => 0.8,
            });
        }
        if self
            .canonical
            .confidence
            .is_some_and(|confidence| confidence < 0.85)
        {
            self.canonical.review_required = true;
            if self.canonical.review_reason.is_none() {
                self.canonical.review_reason = Some("parser confidence below 0.85".into());
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Provenance {
    DocumentAI { confidence: f32 },

    Manual,
    Computed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposedChange {
    pub page: usize,
    pub old_text: String,
    pub new_text: String,
    pub reason: String,
    pub confidence: f32,
    pub affects_subsequent_balances: bool,
    pub bbox: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ParserStats {
    pub total_attempts: usize,
    pub gemini_wins: usize,
    pub docai_wins: usize,
    pub llamaparse_wins: usize,

    pub offline_wins: usize,
}

// ---------------------------------------------------------------------------
// Decimal / f64 conversion helpers (Stage 7 / Item #11)
// ---------------------------------------------------------------------------

/// Convert a `Decimal` to `f64`. Use only at GUI plotting / image-pixel
/// boundaries where precision is acceptable.
#[inline]
pub fn dec_to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

/// Convert an `f64` to a `Decimal`, rounded to two decimal places (the
/// natural precision of bank statements).
#[inline]
pub fn f64_to_dec(v: f64) -> Decimal {
    Decimal::from_f64_retain(v)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn tx(debit: Option<Decimal>, credit: Option<Decimal>) -> Transaction {
        Transaction {
            page: 0,
            line_on_page: 0,
            date: "2026-01-01".into(),
            raw_text: "test".into(),
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
    fn proposed_change_roundtrips_with_bbox() -> anyhow::Result<()> {
        let change_with_bbox = ProposedChange {
            page: 1,
            old_text: "100.00".into(),
            new_text: "150.00".into(),
            reason: "Adjust".into(),
            confidence: 0.95,
            affects_subsequent_balances: true,
            bbox: Some([10.0, 20.0, 50.0, 40.0]),
        };
        let json = serde_json::to_string(&change_with_bbox)?;
        let decoded: ProposedChange = serde_json::from_str(&json)?;
        assert_eq!(decoded.bbox, Some([10.0, 20.0, 50.0, 40.0]));

        let change_no_bbox = ProposedChange {
            page: 1,
            old_text: "100.00".into(),
            new_text: "150.00".into(),
            reason: "Adjust".into(),
            confidence: 0.95,
            affects_subsequent_balances: true,
            bbox: None,
        };
        let json = serde_json::to_string(&change_no_bbox)?;
        let decoded: ProposedChange = serde_json::from_str(&json)?;
        assert_eq!(decoded.bbox, None);
        Ok(())
    }

    #[test]
    fn canonical_metadata_roundtrips_and_preserves_review_state() -> anyhow::Result<()> {
        let mut transaction = tx(Some(dec!(10.25)), None);
        transaction.ensure_canonical_metadata();
        transaction.canonical.currency = Some("AUD".into());
        transaction.canonical.locale = Some("en-AU".into());
        transaction.canonical.review_required = true;
        transaction.canonical.review_reason = Some("manual confirmation requested".into());

        let encoded = serde_json::to_string(&transaction)?;
        let decoded: Transaction = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, transaction);
        assert_eq!(decoded.canonical.stable_row_id, "p0:r0");
        assert_eq!(decoded.canonical.currency.as_deref(), Some("AUD"));
        assert_eq!(decoded.canonical.locale.as_deref(), Some("en-AU"));
        assert_eq!(decoded.canonical.confidence, Some(1.0));
        assert!(decoded.canonical.review_required);
        Ok(())
    }

    #[test]
    fn legacy_transaction_json_deserializes_then_normalizes() -> anyhow::Result<()> {
        let legacy = serde_json::json!({
            "page": 2,
            "line_on_page": 7,
            "date": "2026-01-01",
            "raw_text": "Legacy row",
            "debit": "5.00",
            "credit": null,
            "running_balance": "15.00",
            "bbox": [10.0, 20.0, 100.0, 30.0],
            "provenance": "Computed"
        });
        let mut transaction: Transaction = serde_json::from_value(legacy)?;
        assert!(transaction.canonical.is_empty());
        transaction.ensure_canonical_metadata();
        assert_eq!(transaction.canonical.stable_row_id, "p2:r7");
        assert_eq!(transaction.canonical.confidence, Some(0.8));
        assert!(transaction.canonical.review_required);
        Ok(())
    }

    /// Sign-convention regression: this codebase treats `debit` as money in
    /// (adds to balance) and `credit` as money out (subtracts). Anyone wiring
    /// formal double-entry semantics on top of this struct must use
    /// `delta_in()` / `delta_out()` / `net_delta()` to avoid silently
    /// inverting numbers.
    #[test]
    fn delta_helpers_match_running_balance_arithmetic() {
        let opening = dec!(100);
        let row_in = tx(Some(dec!(50)), None);
        let row_out = tx(None, Some(dec!(20)));
        let row_neither = tx(None, None);

        assert_eq!(row_in.delta_in(), dec!(50));
        assert_eq!(row_in.delta_out(), dec!(0));
        assert_eq!(row_in.net_delta(), dec!(50));

        assert_eq!(row_out.delta_in(), dec!(0));
        assert_eq!(row_out.delta_out(), dec!(20));
        assert_eq!(row_out.net_delta(), dec!(-20));

        assert_eq!(row_neither.net_delta(), dec!(0));

        // Compose: opening 100 + 50 - 20 = 130
        let final_balance = opening + row_in.net_delta() + row_out.net_delta();
        assert_eq!(final_balance, dec!(130));
    }

    /// The classic 0.1 + 0.2 != 0.3 problem under f64 - Decimal handles this
    /// exactly. This is the whole reason for Stage 7.
    #[test]
    fn decimal_avoids_f64_drift_across_hundreds_of_rows() {
        let one_cent = dec!(0.01);
        let mut bal = dec!(0);
        for _ in 0..1000 {
            bal += one_cent;
        }
        assert_eq!(bal, dec!(10.00));
    }
}

// ---------------------------------------------------------------------------
// Polars DataFrame integration (Phase 1)
// ---------------------------------------------------------------------------

use polars::prelude::*;

/// Error type for DataFrame conversions.
#[derive(Debug, thiserror::Error)]
pub enum DataFrameError {
    #[error("Polars error: {0}")]
    Polars(#[from] polars::error::PolarsError),
    #[error("Missing column: {0}")]
    MissingColumn(String),
    #[error("Invalid decimal value: {0}")]
    InvalidDecimal(String),
}

/// Convert a slice of transactions into a Polars DataFrame.
///
/// Monetary columns (`debit`, `credit`, `running_balance`) are stored as
/// `f64` inside the DataFrame (Polars does not natively support
/// `rust_decimal`). The `from_dataframe` path converts back to `Decimal`
/// with `round_dp(2)` so no precision is lost for bank-statement data.
pub fn transactions_to_dataframe(txs: &[Transaction]) -> Result<DataFrame, DataFrameError> {
    let pages: Vec<u32> = txs.iter().map(|t| t.page as u32).collect();
    let lines: Vec<u32> = txs.iter().map(|t| t.line_on_page as u32).collect();
    let dates: Vec<&str> = txs.iter().map(|t| t.date.as_str()).collect();
    let raw_texts: Vec<&str> = txs.iter().map(|t| t.raw_text.as_str()).collect();

    let debits: Vec<Option<f64>> = txs.iter().map(|t| t.debit.map(dec_to_f64)).collect();
    let credits: Vec<Option<f64>> = txs.iter().map(|t| t.credit.map(dec_to_f64)).collect();
    let balances: Vec<Option<f64>> = txs
        .iter()
        .map(|t| t.running_balance.map(dec_to_f64))
        .collect();

    let df = DataFrame::new(vec![
        Column::new("page".into(), &pages),
        Column::new("line_on_page".into(), &lines),
        Column::new("date".into(), dates.as_slice()),
        Column::new("raw_text".into(), raw_texts.as_slice()),
        Column::new("debit".into(), &debits),
        Column::new("credit".into(), &credits),
        Column::new("running_balance".into(), &balances),
    ])?;

    Ok(df)
}

/// Reconstruct a `Vec<Transaction>` from a Polars DataFrame.
///
/// The DataFrame must contain at minimum: `page`, `line_on_page`, `date`,
/// `debit`, `credit`, `running_balance`. Missing `raw_text` defaults to "".
pub fn dataframe_to_transactions(df: &DataFrame) -> Result<Vec<Transaction>, DataFrameError> {
    let pages = df
        .column("page")
        .map_err(|_| DataFrameError::MissingColumn("page".into()))?
        .u32()
        .map_err(DataFrameError::Polars)?;
    let lines = df
        .column("line_on_page")
        .map_err(|_| DataFrameError::MissingColumn("line_on_page".into()))?
        .u32()
        .map_err(DataFrameError::Polars)?;
    let dates = df
        .column("date")
        .map_err(|_| DataFrameError::MissingColumn("date".into()))?
        .str()
        .map_err(DataFrameError::Polars)?;

    let raw_texts = df.column("raw_text").ok().and_then(|c| c.str().ok());

    let debits = df
        .column("debit")
        .map_err(|_| DataFrameError::MissingColumn("debit".into()))?
        .f64()
        .map_err(DataFrameError::Polars)?;
    let credits = df
        .column("credit")
        .map_err(|_| DataFrameError::MissingColumn("credit".into()))?
        .f64()
        .map_err(DataFrameError::Polars)?;
    let balances = df
        .column("running_balance")
        .map_err(|_| DataFrameError::MissingColumn("running_balance".into()))?
        .f64()
        .map_err(DataFrameError::Polars)?;

    let mut txs = Vec::with_capacity(df.height());
    for i in 0..df.height() {
        txs.push(Transaction {
            page: pages.get(i).unwrap_or(0) as usize,
            line_on_page: lines.get(i).unwrap_or(0) as usize,
            date: dates.get(i).unwrap_or("").to_string(),
            raw_text: raw_texts.and_then(|rt| rt.get(i)).unwrap_or("").to_string(),
            debit: debits.get(i).map(f64_to_dec),
            credit: credits.get(i).map(f64_to_dec),
            running_balance: balances.get(i).map(f64_to_dec),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::Computed,
            category: None,
            canonical: Default::default(),
        });
    }

    Ok(txs)
}

#[cfg(test)]
mod dataframe_tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_txs() -> Vec<Transaction> {
        vec![
            Transaction {
                page: 1,
                line_on_page: 1,
                date: "2026-01-01".into(),
                raw_text: "Opening deposit".into(),
                debit: Some(dec!(1000.00)),
                credit: None,
                running_balance: Some(dec!(1000.00)),
                bbox: None,
                field_bboxes: Default::default(),
                provenance: Provenance::Manual,
                category: None,
                canonical: Default::default(),
            },
            Transaction {
                page: 1,
                line_on_page: 2,
                date: "2026-01-02".into(),
                raw_text: "Grocery store".into(),
                debit: None,
                credit: Some(dec!(42.50)),
                running_balance: Some(dec!(957.50)),
                bbox: None,
                field_bboxes: Default::default(),
                provenance: Provenance::Manual,
                category: None,
                canonical: Default::default(),
            },
        ]
    }

    #[test]
    fn round_trip_transactions_through_dataframe() -> anyhow::Result<()> {
        let txs = sample_txs();
        let df = transactions_to_dataframe(&txs)?;
        assert_eq!(df.height(), 2);
        assert_eq!(df.width(), 7);

        let recovered = dataframe_to_transactions(&df)?;
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].debit, Some(dec!(1000.00)));
        assert_eq!(recovered[0].credit, None);
        assert_eq!(recovered[0].running_balance, Some(dec!(1000.00)));
        assert_eq!(recovered[1].credit, Some(dec!(42.50)));
        assert_eq!(recovered[1].running_balance, Some(dec!(957.50)));
        Ok(())
    }

    #[test]
    fn empty_transactions_produce_empty_dataframe() -> anyhow::Result<()> {
        let df = transactions_to_dataframe(&[])?;
        assert_eq!(df.height(), 0);
        Ok(())
    }
}
