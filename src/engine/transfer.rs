//! Transaction Transfer Pipeline.
//!
//! Transfers transactions from a "source" bank statement PDF to a "target"
//! bank statement PDF, intelligently adapting formats (dates, numbers,
//! descriptions, column layouts) to match the target's visual style. The
//! pipeline runs through 9 stages with live progress reporting and exhaustive
//! AI + engine verification.

use crate::engine::model::FieldBboxes;
use crate::engine::number_format::NumberFormat;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Describes the visual and structural format of a parsed bank statement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatementFormat {
    pub bank_name: String,
    /// e.g. "DD/MM/YYYY", "MM/DD/YYYY", "YYYY-MM-DD"
    pub date_format: String,
    /// Number rendering style (currency, separators, negative convention).
    pub number_format: NumberFormat,
    /// Ordered list of columns in the transaction table.
    pub column_order: Vec<ColumnType>,
    pub has_running_balance: bool,
    pub currency_symbol: String,
    /// Estimated transaction rows that fit on a single page.
    pub rows_per_page: usize,
    /// Page header area height in PDF points (logo, account info).
    pub header_height_pts: f32,
    /// Page footer area height in PDF points.
    pub footer_height_pts: f32,
    /// Bounding box of the transaction table area on a typical page.
    pub transaction_area_bbox: [f32; 4],
    /// Primary font used in the transaction table.
    pub font_name: String,
    /// Font size in points.
    pub font_size: f32,
    /// Vertical spacing between transaction rows in points.
    pub row_height_pts: f32,
    /// Which Document AI processor version works best for this format.
    pub parser_version: Option<String>,
}

/// Column types found in bank statement transaction tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Date,
    Description,
    Debit,
    Credit,
    Amount,
    Balance,
    Reference,
    ValueDate,
}

/// A fully mapped transaction ready to be written into the target PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedTransaction {
    /// Target page index (0-based).
    pub target_page: usize,
    /// Line index within the target page.
    pub target_line: usize,
    /// Date string already converted to the target's format.
    pub date: String,
    /// Description adapted to the target's style.
    pub description: String,
    /// Debit amount (money in).
    pub debit: Option<Decimal>,
    /// Credit amount (money out).
    pub credit: Option<Decimal>,
    /// Running balance recomputed from the target's opening balance.
    pub running_balance: Decimal,
    /// Where each field should be placed on the target page.
    pub field_bboxes: FieldBboxes,
}

/// Gemini's plan for how to execute the transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferPlan {
    /// Per-transaction mapping instructions.
    pub mappings: Vec<TransactionMapping>,
    /// How many pages the output will have.
    pub output_page_count: usize,
    /// Pages from the target to clone (for extra capacity).
    pub pages_to_clone: Vec<usize>,
    /// Pages from the target to remove (excess capacity).
    pub pages_to_remove: Vec<usize>,
    /// Overall strategy description.
    pub strategy: String,
    /// Confidence score (0..1).
    pub confidence: f32,
    /// Path to the visual proof PDF (if generated).
    pub visual_proof_path: Option<PathBuf>,
    /// Whether the AI has explicitly approved this plan based on the visual proof.
    pub ai_approved: bool,
}

/// How a single source transaction maps to the target format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionMapping {
    /// Index into the source transaction list.
    pub source_index: usize,
    /// Target page the transaction lands on.
    pub target_page: usize,
    /// Target line within that page.
    pub target_line: usize,
    /// Date converted to the target's format.
    pub converted_date: String,
    /// Description adapted to the target's convention.
    pub adapted_description: String,
}

/// Result of the entire transfer pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResult {
    pub output_path: PathBuf,
    pub source_tx_count: usize,
    pub target_tx_count: usize,
    pub pages_added: usize,
    pub pages_removed: usize,
    pub math_verified: bool,
    pub visual_verified: bool,
    pub visual_score: f64,
    pub math_imbalance: Decimal,
    pub stages_completed: u8,
    pub total_duration_secs: f64,
    pub corrections_applied: usize,
    pub retries_attempted: usize,
    pub synthesized_fonts_used: bool,
    pub visual_proof_path: Option<PathBuf>,
}

/// Tracks which stage the pipeline is currently executing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStage {
    AnalyzeSource,
    AnalyzeTarget,
    AiFormatMapping,
    ComputeBalances,
    GeneratePreview,
    AiVisualReview,
    PdfSurgery,
    VisualFidelityCheck,
    MathVerificationEngine,
    MathVerificationGemini,
    FinalAudit,
}

impl TransferStage {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AnalyzeSource => "Analyzing source statement...",
            Self::AnalyzeTarget => "Analyzing target statement...",
            Self::AiFormatMapping => "AI mapping transaction formats...",
            Self::ComputeBalances => "Computing balances...",
            Self::GeneratePreview => "Generating visual proof of edits...",
            Self::AiVisualReview => "AI reviewing visual proof...",
            Self::PdfSurgery => "Applying PDF changes...",
            Self::VisualFidelityCheck => "Verifying visual fidelity...",
            Self::MathVerificationEngine => "Verifying math (engine)...",
            Self::MathVerificationGemini => "Verifying math (AI)...",
            Self::FinalAudit => "Writing audit report...",
        }
    }

    /// Progress fraction range [start, end) for this stage.
    pub fn fraction_range(&self) -> (f32, f32) {
        match self {
            Self::AnalyzeSource => (0.00, 0.10),
            Self::AnalyzeTarget => (0.10, 0.20),
            Self::AiFormatMapping => (0.20, 0.30),
            Self::ComputeBalances => (0.30, 0.33),
            Self::GeneratePreview => (0.33, 0.35),
            Self::AiVisualReview => (0.35, 0.40),
            Self::PdfSurgery => (0.40, 0.55),
            Self::VisualFidelityCheck => (0.55, 0.75),
            Self::MathVerificationEngine => (0.75, 0.85),
            Self::MathVerificationGemini => (0.85, 0.95),
            Self::FinalAudit => (0.95, 1.00),
        }
    }
}

/// Return the original template page for every page in the post-clone PDF.
/// Clone requests reference original page indices and are inserted immediately
/// after that page in request order.
pub fn cloned_page_template_map(
    original_page_count: usize,
    pages_to_clone: &[usize],
) -> Vec<usize> {
    let mut clone_counts = std::collections::HashMap::new();
    for &page in pages_to_clone {
        if page < original_page_count {
            *clone_counts.entry(page).or_insert(0usize) += 1;
        }
    }
    let mut templates = Vec::with_capacity(original_page_count + pages_to_clone.len());
    for page in 0..original_page_count {
        templates.push(page);
        for _ in 0..clone_counts.get(&page).copied().unwrap_or_default() {
            templates.push(page);
        }
    }
    templates
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferDateStyle {
    DayMonthNumeric {
        separator: char,
        year_digits: usize,
        pad_day: bool,
    },
    MonthDayNumeric {
        separator: char,
        year_digits: usize,
        pad_day: bool,
    },
    YearMonthDay {
        separator: char,
    },
    DayMonthName {
        year_digits: usize,
        pad_day: bool,
        uppercase: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct TransferDateParts {
    day: u32,
    month: u32,
    year: Option<i32>,
}

fn month_number(value: &str) -> Option<u32> {
    match value.get(..3)?.to_ascii_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn infer_transfer_date_style(
    transactions: &[crate::engine::model::Transaction],
    hint: Option<TransferDateStyle>,
) -> Result<TransferDateStyle, String> {
    let first = transactions
        .first()
        .ok_or_else(|| "cannot infer date style from an empty ledger".to_string())?;
    let first_date = first.date.trim();
    if first_date.chars().any(char::is_alphabetic) {
        let tokens: Vec<&str> = first_date.split_whitespace().collect();
        if !(tokens.len() == 2 || tokens.len() == 3)
            || tokens[0].parse::<u32>().is_err()
            || month_number(tokens[1]).is_none()
        {
            return Err(format!("unrecognized textual date '{}'", first.date));
        }
        let year_digits = tokens.get(2).map_or(0, |year| year.len());
        if !matches!(year_digits, 0 | 2 | 4) {
            return Err(format!("unsupported year width in '{}'", first.date));
        }
        let pad_day = transactions.iter().any(|transaction| {
            transaction
                .date
                .split_whitespace()
                .next()
                .is_some_and(|day| day.len() == 2 && day.starts_with('0'))
        });
        let uppercase = tokens[1]
            .chars()
            .all(|character| !character.is_ascii_lowercase());
        let style = TransferDateStyle::DayMonthName {
            year_digits,
            pad_day,
            uppercase,
        };
        return Ok(style);
    }

    let separator = ['/', '-', '.']
        .into_iter()
        .find(|separator| first_date.contains(*separator))
        .ok_or_else(|| format!("unrecognized date separator in '{}'", first.date))?;
    let mut year_first = false;
    let mut day_first_evidence = false;
    let mut month_first_evidence = false;
    let mut year_digits = 0usize;
    let mut pad_day = false;
    for transaction in transactions {
        if transaction.date.chars().any(char::is_alphabetic)
            || !transaction.date.contains(separator)
        {
            continue;
        }
        let parts: Vec<&str> = transaction.date.split(separator).collect();
        if parts.len() != 3 {
            return Err(format!(
                "expected three date parts in '{}'",
                transaction.date
            ));
        }
        let values: Vec<u32> = parts
            .iter()
            .map(|part| {
                part.parse::<u32>()
                    .map_err(|_| format!("non-numeric date part in '{}'", transaction.date))
            })
            .collect::<Result<_, _>>()?;
        if parts[0].len() == 4 {
            year_first = true;
        } else {
            year_digits = parts[2].len();
            pad_day |= parts[0].len() == 2;
            day_first_evidence |= values[0] > 12;
            month_first_evidence |= values[1] > 12;
        }
    }
    let style = if year_first {
        TransferDateStyle::YearMonthDay { separator }
    } else if day_first_evidence && !month_first_evidence {
        TransferDateStyle::DayMonthNumeric {
            separator,
            year_digits,
            pad_day,
        }
    } else if month_first_evidence && !day_first_evidence {
        TransferDateStyle::MonthDayNumeric {
            separator,
            year_digits,
            pad_day,
        }
    } else if let Some(hint) = hint {
        match hint {
            TransferDateStyle::DayMonthNumeric { .. } | TransferDateStyle::DayMonthName { .. } => {
                TransferDateStyle::DayMonthNumeric {
                    separator,
                    year_digits,
                    pad_day,
                }
            }
            TransferDateStyle::MonthDayNumeric { .. } => TransferDateStyle::MonthDayNumeric {
                separator,
                year_digits,
                pad_day,
            },
            TransferDateStyle::YearMonthDay { .. } => {
                return Err("ambiguous numeric date cannot inherit year-first ordering".into())
            }
        }
    } else {
        TransferDateStyle::DayMonthNumeric {
            separator,
            year_digits,
            pad_day,
        }
    };
    Ok(style)
}

fn normalize_transfer_year(value: i32, digits: usize) -> i32 {
    if digits == 2 {
        if value >= 70 {
            1900 + value
        } else {
            2000 + value
        }
    } else {
        value
    }
}

fn parse_transfer_date(value: &str, style: TransferDateStyle) -> Result<TransferDateParts, String> {
    let value = value.trim();
    let (day, month, year) = match style {
        TransferDateStyle::DayMonthName { year_digits, .. } => {
            let tokens: Vec<&str> = value.split_whitespace().collect();
            if tokens.len() != if year_digits == 0 { 2 } else { 3 } {
                return Err(format!(
                    "date '{}' does not match textual target style",
                    value
                ));
            }
            let day = tokens[0]
                .parse::<u32>()
                .map_err(|_| format!("invalid day in '{}'", value))?;
            let month =
                month_number(tokens[1]).ok_or_else(|| format!("invalid month in '{}'", value))?;
            let year = if year_digits == 0 {
                None
            } else {
                Some(normalize_transfer_year(
                    tokens[2]
                        .parse::<i32>()
                        .map_err(|_| format!("invalid year in '{}'", value))?,
                    year_digits,
                ))
            };
            (day, month, year)
        }
        TransferDateStyle::DayMonthNumeric {
            separator,
            year_digits,
            ..
        }
        | TransferDateStyle::MonthDayNumeric {
            separator,
            year_digits,
            ..
        } => {
            let parts: Vec<&str> = value.split(separator).collect();
            if parts.len() != 3 {
                return Err(format!("expected three date parts in '{}'", value));
            }
            let first = parts[0]
                .parse::<u32>()
                .map_err(|_| format!("invalid date in '{}'", value))?;
            let second = parts[1]
                .parse::<u32>()
                .map_err(|_| format!("invalid date in '{}'", value))?;
            let year = normalize_transfer_year(
                parts[2]
                    .parse::<i32>()
                    .map_err(|_| format!("invalid year in '{}'", value))?,
                year_digits,
            );
            match style {
                TransferDateStyle::DayMonthNumeric { .. } => (first, second, Some(year)),
                _ => (second, first, Some(year)),
            }
        }
        TransferDateStyle::YearMonthDay { separator } => {
            let parts: Vec<&str> = value.split(separator).collect();
            if parts.len() != 3 {
                return Err(format!("expected three date parts in '{}'", value));
            }
            (
                parts[2]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid day in '{}'", value))?,
                parts[1]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid month in '{}'", value))?,
                Some(
                    parts[0]
                        .parse::<i32>()
                        .map_err(|_| format!("invalid year in '{}'", value))?,
                ),
            )
        }
    };
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return Err(format!(
            "date '{}' is outside valid day/month ranges",
            value
        ));
    }
    Ok(TransferDateParts { day, month, year })
}

fn parse_transfer_date_flexible(
    value: &str,
    preferred_style: TransferDateStyle,
) -> Result<TransferDateParts, String> {
    if let Ok(parts) = parse_transfer_date(value, preferred_style) {
        return Ok(parts);
    }
    let value = value.trim();
    if value.chars().any(char::is_alphabetic) {
        let tokens: Vec<&str> = value.split_whitespace().collect();
        if !(tokens.len() == 2 || tokens.len() == 3) {
            return Err(format!("unrecognized textual date '{}'", value));
        }
        let day = tokens[0]
            .parse::<u32>()
            .map_err(|_| format!("invalid day in '{}'", value))?;
        let month =
            month_number(tokens[1]).ok_or_else(|| format!("invalid month in '{}'", value))?;
        let year = match tokens.get(2) {
            Some(year) => Some(normalize_transfer_year(
                year.parse::<i32>()
                    .map_err(|_| format!("invalid year in '{}'", value))?,
                year.len(),
            )),
            None => None,
        };
        if !(1..=31).contains(&day) {
            return Err(format!("invalid day in '{}'", value));
        }
        return Ok(TransferDateParts { day, month, year });
    }
    let separator = ['/', '-', '.']
        .into_iter()
        .find(|separator| value.contains(*separator))
        .ok_or_else(|| format!("unrecognized date separator in '{}'", value))?;
    let parts: Vec<&str> = value.split(separator).collect();
    if parts.len() != 3 {
        return Err(format!("expected three date parts in '{}'", value));
    }
    if parts[0].len() == 4 {
        return parse_transfer_date(value, TransferDateStyle::YearMonthDay { separator });
    }
    let first = parts[0]
        .parse::<u32>()
        .map_err(|_| format!("invalid date in '{}'", value))?;
    let second = parts[1]
        .parse::<u32>()
        .map_err(|_| format!("invalid date in '{}'", value))?;
    let year_digits = parts[2].len();
    let style = if first > 12 {
        TransferDateStyle::DayMonthNumeric {
            separator,
            year_digits,
            pad_day: parts[0].len() == 2,
        }
    } else if second > 12 || matches!(preferred_style, TransferDateStyle::MonthDayNumeric { .. }) {
        TransferDateStyle::MonthDayNumeric {
            separator,
            year_digits,
            pad_day: parts[1].len() == 2,
        }
    } else {
        TransferDateStyle::DayMonthNumeric {
            separator,
            year_digits,
            pad_day: parts[0].len() == 2,
        }
    };
    parse_transfer_date(value, style)
}

fn format_transfer_date(
    parts: TransferDateParts,
    style: TransferDateStyle,
    fallback_year: i32,
) -> String {
    let year = parts.year.unwrap_or(fallback_year);
    let day = |pad: bool| {
        if pad {
            format!("{:02}", parts.day)
        } else {
            parts.day.to_string()
        }
    };
    match style {
        TransferDateStyle::DayMonthNumeric {
            separator,
            year_digits,
            pad_day,
        } => {
            let year_text = if year_digits == 2 {
                format!("{:02}", year.rem_euclid(100))
            } else {
                format!("{:04}", year)
            };
            format!(
                "{}{}{:02}{}{}",
                day(pad_day),
                separator,
                parts.month,
                separator,
                year_text
            )
        }
        TransferDateStyle::MonthDayNumeric {
            separator,
            year_digits,
            pad_day,
        } => {
            let year_text = if year_digits == 2 {
                format!("{:02}", year.rem_euclid(100))
            } else {
                format!("{:04}", year)
            };
            format!(
                "{:02}{}{}{}{}",
                parts.month,
                separator,
                day(pad_day),
                separator,
                year_text
            )
        }
        TransferDateStyle::YearMonthDay { separator } => {
            format!(
                "{:04}{}{:02}{}{:02}",
                year, separator, parts.month, separator, parts.day
            )
        }
        TransferDateStyle::DayMonthName {
            year_digits,
            pad_day,
            uppercase,
        } => {
            let names = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let mut month = names[(parts.month - 1) as usize].to_string();
            if uppercase {
                month.make_ascii_uppercase();
            }
            if year_digits == 0 {
                format!("{} {}", day(pad_day), month)
            } else if year_digits == 2 {
                format!("{} {} {:02}", day(pad_day), month, year.rem_euclid(100))
            } else {
                format!("{} {} {:04}", day(pad_day), month, year)
            }
        }
    }
}

/// Build a deterministic transfer plan without an AI provider.
///
/// The local planner preserves source order, uses only exact target row geometry,
/// clones an observed template page when additional capacity is required, and
/// removes trailing target pages when the source ledger is smaller.
pub fn plan_transaction_transfer_deterministic(
    source_transactions: &[crate::engine::model::Transaction],
    target_transactions: &[crate::engine::model::Transaction],
    target_page_count: usize,
) -> Result<TransferPlan, String> {
    if source_transactions.is_empty() {
        return Err("deterministic transfer requires at least one source row".into());
    }
    if target_transactions.is_empty() {
        return Err("deterministic transfer requires at least one target row".into());
    }
    let target_style = infer_transfer_date_style(target_transactions, None)?;
    let source_style = infer_transfer_date_style(source_transactions, Some(target_style))?;
    let fallback_year = parse_transfer_date_flexible(&target_transactions[0].date, target_style)?
        .year
        .or(parse_transfer_date_flexible(&source_transactions[0].date, source_style)?.year)
        .unwrap_or(2000);

    let mut source_order: Vec<usize> = (0..source_transactions.len()).collect();
    source_order.sort_by_key(|index| {
        let tx = &source_transactions[*index];
        (tx.page, tx.line_on_page)
    });
    let mut target_by_page: std::collections::BTreeMap<
        usize,
        Vec<&crate::engine::model::Transaction>,
    > = std::collections::BTreeMap::new();
    for transaction in target_transactions {
        if transaction.bbox.is_none() && transaction.field_bboxes.is_empty() {
            return Err(format!(
                "target row at page {} line {} has no editable geometry",
                transaction.page, transaction.line_on_page
            ));
        }
        target_by_page
            .entry(transaction.page)
            .or_default()
            .push(transaction);
    }
    for rows in target_by_page.values_mut() {
        rows.sort_by_key(|transaction| transaction.line_on_page);
    }
    let observed_pages = target_by_page.keys().next_back().map_or(0, |page| page + 1);
    let original_page_count = target_page_count.max(observed_pages);
    let mut pages_to_clone = Vec::new();
    let mut capacity = target_transactions.len();
    if source_transactions.len() > capacity {
        let (&template_page, template_rows) = target_by_page
            .iter()
            .max_by_key(|(_, rows)| rows.len())
            .ok_or_else(|| "no target page has editable transaction rows".to_string())?;
        if template_rows.is_empty() {
            return Err("selected target template page has zero rows".into());
        }
        while capacity < source_transactions.len() {
            pages_to_clone.push(template_page);
            capacity += template_rows.len();
        }
    }
    let page_templates = cloned_page_template_map(original_page_count, &pages_to_clone);
    let mut target_slots = Vec::new();
    for (output_page, template_page) in page_templates.iter().copied().enumerate() {
        if let Some(rows) = target_by_page.get(&template_page) {
            for (line, transaction) in rows.iter().enumerate() {
                target_slots.push((output_page, line, *transaction));
            }
        }
    }
    if target_slots.len() < source_transactions.len() {
        return Err(format!(
            "deterministic capacity construction produced {} slots for {} source rows",
            target_slots.len(),
            source_transactions.len()
        ));
    }

    let mut mappings = Vec::with_capacity(source_transactions.len());
    for (source_index, (target_page, target_line, _target)) in
        source_order.into_iter().zip(target_slots.iter().copied())
    {
        let source = &source_transactions[source_index];
        if source.debit.is_none() && source.credit.is_none() {
            return Err(format!(
                "source row at page {} line {} has no monetary amount",
                source.page, source.line_on_page
            ));
        }
        mappings.push(TransactionMapping {
            source_index,
            target_page,
            target_line,
            converted_date: format_transfer_date(
                parse_transfer_date_flexible(&source.date, source_style)?,
                target_style,
                fallback_year,
            ),
            adapted_description: transaction_description(source)?,
        });
    }
    let last_used_page = mappings
        .iter()
        .map(|mapping| mapping.target_page)
        .max()
        .unwrap_or(0);
    let pages_to_remove = if pages_to_clone.is_empty() {
        ((last_used_page + 1)..original_page_count).collect()
    } else {
        Vec::new()
    };
    let output_page_count = original_page_count + pages_to_clone.len() - pages_to_remove.len();
    Ok(TransferPlan {
        mappings,
        output_page_count,
        pages_to_clone,
        pages_to_remove,
        strategy: "deterministic-local-exact-geometry-capacity".into(),
        confidence: 1.0,
        ai_approved: false,
        visual_proof_path: None,
    })
}

#[allow(dead_code)]
fn infer_statement_date_format(
    transactions: &[crate::engine::model::Transaction],
) -> Result<&'static str, String> {
    let first = transactions
        .first()
        .ok_or_else(|| "cannot infer date format from an empty ledger".to_string())?;
    let separator = if first.date.contains('/') {
        '/'
    } else if first.date.contains('-') {
        '-'
    } else if first.date.contains('.') {
        '.'
    } else {
        return Err(format!("unrecognized date separator in '{}'", first.date));
    };

    let mut first_is_day = false;
    let mut first_is_month = false;
    let mut year_first = false;
    for transaction in transactions {
        if !transaction.date.contains(separator) {
            return Err("statement contains inconsistent date separators".into());
        }
        let parts: Vec<&str> = transaction.date.split(separator).collect();
        if parts.len() != 3 {
            return Err(format!(
                "expected three date parts in '{}'",
                transaction.date
            ));
        }
        let values: Vec<u32> = parts
            .iter()
            .map(|part| {
                part.parse::<u32>()
                    .map_err(|_| format!("non-numeric date part in '{}'", transaction.date))
            })
            .collect::<Result<_, _>>()?;
        if parts[0].len() == 4 {
            year_first = true;
        } else if values[0] > 12 {
            first_is_day = true;
        } else if values[1] > 12 {
            first_is_month = true;
        }
    }
    if year_first && (first_is_day || first_is_month) {
        return Err("statement mixes year-first and day/month-first dates".into());
    }
    if first_is_day && first_is_month {
        return Err("statement contains contradictory day/month ordering".into());
    }

    let format = match (year_first, first_is_day, first_is_month, separator) {
        (true, false, false, '-') => "YYYY-MM-DD",
        (true, false, false, '/') => "YYYY/MM/DD",
        (true, false, false, '.') => "YYYY.MM.DD",
        (false, true, false, '-') => "DD-MM-YYYY",
        (false, true, false, '/') => "DD/MM/YYYY",
        (false, true, false, '.') => "DD.MM.YYYY",
        (false, false, true, '-') => "MM-DD-YYYY",
        (false, false, true, '/') => "MM/DD/YYYY",
        (false, false, true, '.') => "MM.DD.YYYY",
        _ => {
            return Err(
                "date ordering is ambiguous; human review or a configured mapper is required"
                    .into(),
            )
        }
    };
    for transaction in transactions {
        convert_date(&transaction.date, format, format)?;
    }
    Ok(format)
}

pub fn transaction_description(
    transaction: &crate::engine::model::Transaction,
) -> Result<String, String> {
    let expected_amounts = usize::from(transaction.debit.is_some())
        + usize::from(transaction.credit.is_some())
        + usize::from(transaction.running_balance.is_some());
    let mut tokens: Vec<String> = transaction
        .raw_text
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let date_tokens: Vec<&str> = transaction.date.split_whitespace().collect();

    // Leading date (most single-line rows).
    if !date_tokens.is_empty()
        && tokens.len() >= date_tokens.len()
        && tokens
            .iter()
            .take(date_tokens.len())
            .zip(date_tokens.iter())
            .all(|(raw, date)| raw.eq_ignore_ascii_case(date))
    {
        tokens.drain(..date_tokens.len());
    } else if !date_tokens.is_empty() {
        // Multi-line / preceding-description rows may place the date after
        // free text (e.g. "Withdrawal-Osko … 25/09/23 25.00 576.87").
        if let Some(start) = tokens
            .windows(date_tokens.len())
            .position(|window| {
                window
                    .iter()
                    .zip(date_tokens.iter())
                    .all(|(raw, date)| raw.eq_ignore_ascii_case(date))
            })
        {
            tokens.drain(start..start + date_tokens.len());
        }
    }
    if date_tokens.len() == 2
        && tokens.first().is_some_and(|token| {
            token.len() == 4
                && token
                    .parse::<u32>()
                    .is_ok_and(|year| (1900..=2100).contains(&year))
        })
    {
        tokens.remove(0);
    }
    let mut removed = 0usize;
    let mut index = tokens.len();
    while index > 0 && removed < expected_amounts {
        index -= 1;
        let upper = tokens[index].to_ascii_uppercase();
        if matches!(upper.as_str(), "CR" | "DR" | "AUD" | "USD" | "EUR" | "GBP") {
            tokens.remove(index);
            continue;
        }
        let cleaned = upper
            .trim_matches(|character: char| {
                matches!(character, '$' | '€' | '£' | '(' | ')' | '+' | '-')
            })
            .trim_end_matches("CR")
            .trim_end_matches("DR")
            .replace(',', "");
        if Decimal::from_str_exact(&cleaned).is_ok() {
            tokens.remove(index);
            removed += 1;
        }
    }
    while tokens
        .last()
        .is_some_and(|token| matches!(token.to_ascii_uppercase().as_str(), "CR" | "DR" | "AUD"))
    {
        tokens.pop();
    }
    let mut description_tokens = Vec::new();
    for token in tokens {
        // Pure dotted-leader tokens (multi-line table fill) terminate the desc.
        if !token.is_empty() && token.chars().all(|c| c == '.') {
            break;
        }
        if let Some(leader_start) = token.find("...") {
            let prefix = token[..leader_start].trim_end_matches('.');
            if !prefix.is_empty() {
                description_tokens.push(prefix.to_string());
            }
            break;
        }
        description_tokens.push(token);
    }
    let description = description_tokens
        .join(" ")
        .replace('\u{fb00}', "ff")
        .replace('\u{fb01}', "fi")
        .replace('\u{fb02}', "fl")
        .replace('\u{fb03}', "ffi")
        .replace('\u{fb04}', "ffl")
        .replace(['\u{fb05}', '\u{fb06}'], "st")
        .trim()
        .to_string();
    if description.is_empty() {
        return Err(format!(
            "source row at page {} line {} has no deterministic description",
            transaction.page, transaction.line_on_page
        ));
    }
    Ok(description)
}

/// Recompute running balances from an opening balance and a set of
/// transactions (using the codebase's sign convention: debit = money in,
/// credit = money out).
///
/// # Errors
///
/// Returns `BalanceError` if the running balance overflows or underflows
/// beyond the valid monetary range.
pub fn recompute_running_balances(
    opening: Decimal,
    txns: &mut [MappedTransaction],
) -> Result<(), String> {
    let mut balance = opening;
    for (idx, tx) in txns.iter_mut().enumerate() {
        let delta_in = tx.debit.unwrap_or(Decimal::ZERO);
        let delta_out = tx.credit.unwrap_or(Decimal::ZERO);

        // Check for overflow before arithmetic
        let new_balance = balance + delta_in - delta_out;
        if new_balance < Decimal::ZERO && balance >= Decimal::ZERO {
            // Allow negative balances (overdrafts) but log them
            tracing::warn!(
                "Negative running balance at transaction {}: {}",
                idx,
                new_balance
            );
        }

        // Check for unreasonable values (more than 1 trillion)
        if new_balance.abs() > Decimal::new(1000000000000, 0) {
            return Err(format!(
                "Balance overflow at transaction {}: balance = {}",
                idx, new_balance
            ));
        }

        balance = new_balance.round_dp(2);
        tx.running_balance = balance;
    }
    Ok(())
}

/// Independently verify the mapped ledger after exact PDF mutation. This is a
/// fallback only for outputs that generic reparsers cannot read; it must never
/// override a concrete mismatch from a non-empty reparsed ledger.
pub fn verify_mapped_balances(opening: Decimal, txns: &[MappedTransaction]) -> Result<(), String> {
    if txns.is_empty() {
        return Err("mapped ledger is empty".into());
    }
    let mut balance = opening;
    for (index, transaction) in txns.iter().enumerate() {
        if transaction.debit.is_some() == transaction.credit.is_some() {
            return Err(format!(
                "mapped transaction {index} must contain exactly one action amount"
            ));
        }
        let expected = (balance + transaction.debit.unwrap_or(Decimal::ZERO)
            - transaction.credit.unwrap_or(Decimal::ZERO))
        .round_dp(2);
        if expected != transaction.running_balance.round_dp(2) {
            return Err(format!(
                "mapped transaction {index} balance mismatch: expected {expected}, observed {}",
                transaction.running_balance
            ));
        }
        balance = expected;
    }
    Ok(())
}

/// Convert a date string from one format to another.
/// Supports DD/MM/YYYY, MM/DD/YYYY, YYYY-MM-DD and variants with '-' or '.'.
///
/// # Errors
///
/// Returns the original string if parsing fails or the format is unrecognized.
pub fn convert_date(date_str: &str, from_format: &str, to_format: &str) -> Result<String, String> {
    if from_format == to_format {
        return Ok(date_str.to_string());
    }

    // Normalize the date string and detect separator
    let date_str = date_str.trim();
    if date_str.is_empty() {
        return Err("Empty date string".to_string());
    }

    let sep_char = if date_str.contains('/') {
        '/'
    } else if date_str.contains('-') {
        '-'
    } else if date_str.contains('.') {
        '.'
    } else {
        return Err(format!("Unrecognized date separator in '{}'", date_str));
    };

    let parts: Vec<&str> = date_str.split(sep_char).collect();
    if parts.len() != 3 {
        return Err(format!(
            "Expected 3 date parts in '{}', got {}",
            date_str,
            parts.len()
        ));
    }

    let p1 = parts[0].trim();
    let p2 = parts[1].trim();
    let p3 = parts[2].trim();

    // Validate that parts are numeric
    for (i, p) in [p1, p2, p3].iter().enumerate() {
        if p.parse::<u32>().is_err() {
            return Err(format!(
                "Non-numeric date part '{}' at position {} in '{}'",
                p,
                i + 1,
                date_str
            ));
        }
    }

    let (day, month, year) = match from_format {
        "DD/MM/YYYY" | "DD-MM-YYYY" | "DD.MM.YYYY" => (p1, p2, p3),
        "MM/DD/YYYY" | "MM-DD-YYYY" | "MM.DD.YYYY" => (p2, p1, p3),
        "YYYY-MM-DD" | "YYYY/MM/DD" | "YYYY.MM.DD" => (p3, p2, p1),
        _ => return Err(format!("Unrecognized source format '{}'", from_format)),
    };

    // Validate day and month ranges
    let day_num: u32 = day.parse().unwrap_or(0);
    let month_num: u32 = month.parse().unwrap_or(0);
    if day_num == 0 || day_num > 31 {
        return Err(format!("Invalid day value: {}", day));
    }
    if month_num == 0 || month_num > 12 {
        return Err(format!("Invalid month value: {}", month));
    }

    let sep = if to_format.contains('/') {
        "/"
    } else if to_format.contains('-') {
        "-"
    } else if to_format.contains('.') {
        "."
    } else {
        return Err(format!("Unrecognized target format '{}'", to_format));
    };

    let result = match to_format {
        "DD/MM/YYYY" | "DD-MM-YYYY" | "DD.MM.YYYY" => {
            format!("{day}{sep}{month}{sep}{year}")
        }
        "MM/DD/YYYY" | "MM-DD-YYYY" | "MM.DD.YYYY" => {
            format!("{month}{sep}{day}{sep}{year}")
        }
        "YYYY-MM-DD" | "YYYY/MM/DD" | "YYYY.MM.DD" => {
            format!("{year}{sep}{month}{sep}{day}")
        }
        _ => return Err(format!("Unrecognized target format '{}'", to_format)),
    };

    Ok(result)
}

/// Build a JSON audit report for the transfer operation.
/// Uses atomic write with checksum to prevent corruption.
pub fn write_transfer_audit(
    result: &TransferResult,
    source_path: &std::path::Path,
    target_path: &std::path::Path,
) -> std::io::Result<PathBuf> {
    let audit_dir = PathBuf::from("audit/transfers");
    std::fs::create_dir_all(&audit_dir)?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let audit_path = audit_dir.join(format!("transfer_{timestamp}.json"));
    let tmp_path = audit_path.with_extension("tmp");

    let report = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "source_pdf": source_path.to_string_lossy(),
        "target_pdf": target_path.to_string_lossy(),
        "output_pdf": result.output_path.to_string_lossy(),
        "source_tx_count": result.source_tx_count,
        "target_tx_count": result.target_tx_count,
        "pages_added": result.pages_added,
        "pages_removed": result.pages_removed,
        "math_verified": result.math_verified,
        "visual_verified": result.visual_verified,
        "visual_score": result.visual_score,
        "math_imbalance": result.math_imbalance.to_string(),
        "stages_completed": result.stages_completed,
        "total_duration_secs": result.total_duration_secs,
        "corrections_applied": result.corrections_applied,
        "retries_attempted": result.retries_attempted,
        "synthesized_fonts_used": result.synthesized_fonts_used,
    });

    let pretty = serde_json::to_string_pretty(&report)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Atomic write: compute checksum, write to temp file, fsync, then rename
    let checksum = crc32fast::hash(pretty.as_bytes());
    let mut payload = pretty.into_bytes();
    payload.extend_from_slice(&checksum.to_le_bytes());

    let mut file = std::fs::File::create(&tmp_path)?;
    use std::io::Write;
    file.write_all(&payload)?;
    file.sync_all()?; // Ensure data is on disk

    std::fs::rename(&tmp_path, &audit_path)?;

    // Verify the write
    let verify_data = std::fs::read(&audit_path)?;
    if verify_data.len() >= 4 {
        let verify_checksum = crc32fast::hash(&verify_data[..verify_data.len() - 4]);
        let stored_checksum =
            u32::from_le_bytes(verify_data[verify_data.len() - 4..].try_into().unwrap());
        if verify_checksum != stored_checksum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Checksum mismatch after write",
            ));
        }
    }

    tracing::info!(
        "Transfer audit written to {:?} (checksum {:08x})",
        audit_path,
        checksum
    );
    Ok(audit_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn recompute_balances_from_opening() {
        let mut txns = vec![
            MappedTransaction {
                target_page: 0,
                target_line: 0,
                date: "01/01/2026".into(),
                description: "Deposit".into(),
                debit: Some(dec!(500)),
                credit: None,
                running_balance: Decimal::ZERO,
                field_bboxes: FieldBboxes::default(),
            },
            MappedTransaction {
                target_page: 0,
                target_line: 1,
                date: "02/01/2026".into(),
                description: "Withdrawal".into(),
                debit: None,
                credit: Some(dec!(200)),
                running_balance: Decimal::ZERO,
                field_bboxes: FieldBboxes::default(),
            },
        ];

        let result = recompute_running_balances(dec!(1000), &mut txns);
        assert!(result.is_ok(), "Balance recomputation should succeed");

        assert_eq!(txns[0].running_balance, dec!(1500.00));
        assert_eq!(txns[1].running_balance, dec!(1300.00));
    }

    #[test]
    fn mapped_balance_verifier_rejects_tampered_running_balance() {
        let mut rows = vec![MappedTransaction {
            target_page: 0,
            target_line: 0,
            date: "01/01/2026".into(),
            description: "TEST".into(),
            debit: Some(dec!(10.00)),
            credit: None,
            running_balance: Decimal::ZERO,
            field_bboxes: FieldBboxes::default(),
        }];
        recompute_running_balances(dec!(100.00), &mut rows).unwrap();
        verify_mapped_balances(dec!(100.00), &rows).unwrap();
        rows[0].running_balance = dec!(999.00);
        assert!(verify_mapped_balances(dec!(100.00), &rows)
            .unwrap_err()
            .contains("balance mismatch"));
    }

    #[test]
    fn convert_date_dd_mm_to_mm_dd() {
        let result = convert_date("25/12/2026", "DD/MM/YYYY", "MM/DD/YYYY");
        assert_eq!(result, Ok("12/25/2026".to_string()));
    }

    #[test]
    fn convert_date_mm_dd_to_yyyy_mm_dd() {
        let result = convert_date("12/25/2026", "MM/DD/YYYY", "YYYY-MM-DD");
        assert_eq!(result, Ok("2026-12-25".to_string()));
    }

    #[test]
    fn convert_date_same_format_is_identity() {
        let result = convert_date("25/12/2026", "DD/MM/YYYY", "DD/MM/YYYY");
        assert_eq!(result, Ok("25/12/2026".to_string()));
    }

    #[test]
    fn convert_date_invalid_format_returns_error() {
        let result = convert_date("25-12-2026", "INVALID", "MM/DD/YYYY");
        assert!(result.is_err(), "Should return error for invalid format");
    }

    #[test]
    fn convert_date_non_numeric_parts_return_error() {
        let result = convert_date("AB/12/2026", "DD/MM/YYYY", "MM/DD/YYYY");
        assert!(result.is_err(), "Should return error for non-numeric parts");
    }

    #[test]
    fn convert_date_empty_string_returns_error() {
        let result = convert_date("", "DD/MM/YYYY", "MM/DD/YYYY");
        assert!(result.is_err(), "Should return error for empty string");
    }

    fn transfer_tx(
        page: usize,
        line_on_page: usize,
        date: &str,
        raw_text: &str,
        amount: Decimal,
        balance: Decimal,
        bbox: Option<[f32; 4]>,
    ) -> crate::engine::model::Transaction {
        crate::engine::model::Transaction {
            page,
            line_on_page,
            date: date.into(),
            raw_text: raw_text.into(),
            debit: Some(amount),
            credit: None,
            running_balance: Some(balance),
            bbox,
            field_bboxes: FieldBboxes::default(),
            provenance: crate::engine::model::Provenance::Computed,
            category: None,
            canonical: Default::default(),
        }
    }

    #[test]
    fn deterministic_plan_maps_exact_capacity_without_provider() {
        let source = vec![
            transfer_tx(
                0,
                1,
                "26/12/2026",
                "26/12/2026 SECOND SHOP 20.00 970.00",
                dec!(20.00),
                dec!(970.00),
                Some([0.0; 4]),
            ),
            transfer_tx(
                0,
                0,
                "25/12/2026",
                "25/12/2026 FIRST SHOP 10.00 990.00",
                dec!(10.00),
                dec!(990.00),
                Some([0.0; 4]),
            ),
        ];
        let target = vec![
            transfer_tx(
                1,
                0,
                "12/26/2025",
                "target two",
                dec!(1.00),
                dec!(99.00),
                Some([10.0, 20.0, 30.0, 40.0]),
            ),
            transfer_tx(
                0,
                0,
                "12/25/2025",
                "target one",
                dec!(1.00),
                dec!(100.00),
                Some([10.0, 20.0, 30.0, 40.0]),
            ),
        ];

        let plan = plan_transaction_transfer_deterministic(&source, &target, 2).unwrap();
        assert_eq!(plan.strategy, "deterministic-local-exact-geometry-capacity");
        assert_eq!(plan.confidence, 1.0);
        assert_eq!(plan.output_page_count, 2);
        assert!(plan.pages_to_clone.is_empty());
        assert!(plan.pages_to_remove.is_empty());
        assert_eq!(plan.mappings.len(), 2);
        assert_eq!(plan.mappings[0].source_index, 1);
        assert_eq!(plan.mappings[0].converted_date, "12/25/2026");
        assert_eq!(plan.mappings[0].adapted_description, "FIRST SHOP");
        assert_eq!(plan.mappings[1].source_index, 0);
        assert_eq!(plan.mappings[1].target_page, 1);
    }

    #[test]
    fn deterministic_plan_defaults_ambiguous_numeric_dates_to_au_day_first() {
        let source = vec![transfer_tx(
            0,
            0,
            "01/02/2026",
            "01/02/2026 SHOP 10.00 90.00",
            dec!(10.00),
            dec!(90.00),
            Some([0.0; 4]),
        )];
        let target = vec![transfer_tx(
            0,
            0,
            "02/03/2026",
            "target",
            dec!(1.00),
            dec!(99.00),
            Some([0.0; 4]),
        )];
        let plan = plan_transaction_transfer_deterministic(&source, &target, 1).unwrap();
        assert_eq!(plan.mappings[0].converted_date, "01/02/2026");
    }

    #[test]
    fn deterministic_plan_supports_unequal_capacity_and_rejects_missing_geometry() {
        let source = vec![transfer_tx(
            0,
            0,
            "25/12/2026",
            "25/12/2026 SHOP 10.00 90.00",
            dec!(10.00),
            dec!(90.00),
            Some([0.0; 4]),
        )];
        let target = vec![
            transfer_tx(
                0,
                0,
                "12/25/2026",
                "target one",
                dec!(1.00),
                dec!(99.00),
                Some([0.0; 4]),
            ),
            transfer_tx(
                0,
                1,
                "12/26/2026",
                "target two",
                dec!(1.00),
                dec!(98.00),
                Some([0.0; 4]),
            ),
        ];
        let smaller = plan_transaction_transfer_deterministic(&source, &target, 1).unwrap();
        assert_eq!(smaller.mappings.len(), 1);
        assert_eq!(smaller.output_page_count, 1);
        assert!(smaller.pages_to_clone.is_empty());

        let expanded_source = vec![source[0].clone(), source[0].clone(), source[0].clone()];
        let expanded =
            plan_transaction_transfer_deterministic(&expanded_source, &target, 1).unwrap();
        assert_eq!(expanded.mappings.len(), 3);
        assert_eq!(expanded.pages_to_clone, vec![0]);
        assert_eq!(expanded.output_page_count, 2);
        assert_eq!(expanded.mappings[2].target_page, 1);
        assert_eq!(expanded.mappings[2].target_line, 0);

        let no_geometry = vec![transfer_tx(
            0,
            0,
            "12/25/2026",
            "target",
            dec!(1.00),
            dec!(99.00),
            None,
        )];
        assert!(
            plan_transaction_transfer_deterministic(&source, &no_geometry, 1)
                .unwrap_err()
                .contains("no editable geometry")
        );
    }

    #[test]
    fn deterministic_plan_converts_numeric_and_textual_au_dates() {
        let numeric_source = vec![transfer_tx(
            0,
            0,
            "01/09/2023",
            "01/09/2023 SHOP 10.00 90.00",
            dec!(10.00),
            dec!(90.00),
            Some([0.0; 4]),
        )];
        let textual_target = vec![transfer_tx(
            0,
            0,
            "19 Dec",
            "19 Dec TARGET 1.00 99.00",
            dec!(1.00),
            dec!(99.00),
            Some([0.0; 4]),
        )];
        let to_text =
            plan_transaction_transfer_deterministic(&numeric_source, &textual_target, 1).unwrap();
        assert_eq!(to_text.mappings[0].converted_date, "1 Sep");

        let textual_source = vec![transfer_tx(
            0,
            0,
            "01 SEP 23",
            "01 SEP 23 SHOP 10.00 90.00",
            dec!(10.00),
            dec!(90.00),
            Some([0.0; 4]),
        )];
        let numeric_target = vec![transfer_tx(
            0,
            0,
            "13/09/23",
            "13/09/23 TARGET 1.00 99.00",
            dec!(1.00),
            dec!(99.00),
            Some([0.0; 4]),
        )];
        let to_numeric =
            plan_transaction_transfer_deterministic(&textual_source, &numeric_target, 1).unwrap();
        assert_eq!(to_numeric.mappings[0].converted_date, "01/09/23");
    }

    #[test]
    fn transfer_stage_labels_all_defined() {
        let stages = [
            TransferStage::AnalyzeSource,
            TransferStage::AnalyzeTarget,
            TransferStage::AiFormatMapping,
            TransferStage::ComputeBalances,
            TransferStage::PdfSurgery,
            TransferStage::VisualFidelityCheck,
            TransferStage::MathVerificationEngine,
            TransferStage::MathVerificationGemini,
            TransferStage::FinalAudit,
        ];
        for s in stages {
            assert!(!s.label().is_empty());
            let (lo, hi) = s.fraction_range();
            assert!(lo < hi);
        }
    }

    #[test]
    fn cloned_page_templates_follow_clone_insertion_order() {
        assert_eq!(cloned_page_template_map(2, &[0, 0, 0]), vec![0, 0, 0, 0, 1]);
        assert_eq!(cloned_page_template_map(3, &[1, 0]), vec![0, 0, 1, 1, 2]);
    }
}
