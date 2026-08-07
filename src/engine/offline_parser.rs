//! Offline fallback parser: converts `LineGeometry` (from any local
//! `GeometryProvider`) into a `BankStatement` compatible with the
//! Document AI output structure.
//!
//! This allows the full workflow to proceed without any network access by
//! using PyMuPDF/Pdfium text extraction + template-based heuristics.

use crate::ai::document_ai::BankStatement;
use crate::engine::model::{FieldBboxes, Provenance, Transaction};
use crate::extractors::geometry::LineGeometry;
use crate::extractors::templates::parsers;
use crate::pdf::PdfEngine;
use rust_decimal::Decimal;
use std::path::Path;
use std::sync::Arc;

/// Parse a bank statement from offline text-layer extraction.
///
/// Uses the PDF engine's `get_text_blocks` + `PyMuPdfHeuristicProvider` to
/// extract rows, then applies winnow parsers to identify dates, amounts,
/// and running balances. Produces a `BankStatement` with lower confidence
/// than Document AI but sufficient for the workflow to proceed offline.
pub fn parse_statement_offline(
    pdf_path: &Path,
    engine: Arc<dyn PdfEngine>,
) -> Result<BankStatement, String> {
    // Step 1: Get layout for page count
    let layout = engine
        .analyze_layout(pdf_path)
        .map_err(|e| format!("layout analysis failed: {e}"))?;

    let total_pages = layout.total_pages;
    if total_pages == 0 {
        return Err("PDF has 0 pages".into());
    }

    // Step 2: Extract text blocks from all pages and cluster into rows
    let mut all_rows: Vec<RawRow> = Vec::new();

    for page in 0..total_pages {
        #[allow(unused_mut)] // mutated only when cfg(feature = "ocr") is active
        let mut blocks = match engine.get_text_blocks(pdf_path, page) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    page,
                    error = %e,
                    "[offline_parser] get_text_blocks failed; treating page as empty"
                );
                Vec::new()
            }
        };

        #[cfg(feature = "ocr")]
        {
            if blocks.is_empty() {
                tracing::info!(
                    "[offline_parser] No text found on page {}, falling back to OCR",
                    page
                );
                blocks = extract_text_via_ocr(pdf_path, page, engine.clone());
            }
        }

        // Stable reading order before row clustering (top→bottom, then left→right).
        blocks.sort_by(|a, b| {
            let y_a = (a.bbox[1] + a.bbox[3]) / 2.0;
            let y_b = (b.bbox[1] + b.bbox[3]) / 2.0;
            if (y_a - y_b).abs() < 5.0 {
                a.bbox[0]
                    .partial_cmp(&b.bbox[0])
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                y_a.partial_cmp(&y_b).unwrap_or(std::cmp::Ordering::Equal)
            }
        });

        // Cluster blocks into rows by y-coordinate proximity (±5pt)
        let mut current_y: Option<f32> = None;
        let mut current_row_blocks = Vec::new();
        let mut line_idx = 0usize;

        for block in &blocks {
            let y_center = (block.bbox[1] + block.bbox[3]) / 2.0;

            if let Some(y) = current_y {
                if (y_center - y).abs() < 5.0 {
                    current_row_blocks.push(block.clone());
                    continue;
                }
                // Flush current row
                if !current_row_blocks.is_empty() {
                    all_rows.push(RawRow::from_blocks(page, line_idx, &current_row_blocks));
                    line_idx += 1;
                }
                current_row_blocks.clear();
            }

            current_y = Some(y_center);
            current_row_blocks.push(block.clone());
        }
        // Flush final row on page
        if !current_row_blocks.is_empty() {
            all_rows.push(RawRow::from_blocks(page, line_idx, &current_row_blocks));
        }
    }

    // Step 3: Parse each row to identify transactions
    let (transactions, opening_balance, closing_balance) = parse_rows_into_transactions(&all_rows);

    tracing::info!(
        "[offline_parser] extracted {} transactions from {} pages (opening={}, closing={})",
        transactions.len(),
        total_pages,
        opening_balance,
        closing_balance,
    );

    let mut statement = BankStatement {
        total_pages,
        transactions,
        opening_balance,
        closing_balance,
        account_number: extract_account_number(&all_rows),
        bank_name: None,
    };
    statement.ensure_canonical_metadata();
    calibrate_offline_confidence(&mut statement);
    Ok(statement)
}

/// Parse a bank statement from pre-extracted `LineGeometry` entries.
pub fn parse_statement_from_geometry(
    geometries: &[LineGeometry],
    total_pages: usize,
) -> Result<BankStatement, String> {
    let rows: Vec<RawRow> = geometries
        .iter()
        .map(|g| RawRow {
            page: g.page,
            line_on_page: g.line_on_page,
            text: g.text.clone(),
            bbox: g.bbox,
            blocks: Vec::new(),
        })
        .collect();

    let (transactions, opening_balance, closing_balance) = parse_rows_into_transactions(&rows);

    let mut statement = BankStatement {
        total_pages,
        transactions,
        opening_balance,
        closing_balance,
        account_number: extract_account_number(&rows),
        bank_name: None,
    };
    statement.ensure_canonical_metadata();
    calibrate_offline_confidence(&mut statement);
    Ok(statement)
}

fn calibrate_offline_confidence(statement: &mut BankStatement) {
    for transaction in &mut statement.transactions {
        let has_amount_geometry = if transaction.debit.is_some() {
            transaction.field_bboxes.debit.is_some()
        } else if transaction.credit.is_some() {
            transaction.field_bboxes.credit.is_some()
        } else {
            false
        };
        let exact_geometry = transaction.field_bboxes.date.is_some()
            && transaction.field_bboxes.description.is_some()
            && transaction.field_bboxes.running_balance.is_some()
            && has_amount_geometry;
        transaction.canonical.confidence = Some(if exact_geometry { 0.95 } else { 0.75 });
        transaction.canonical.review_required = !exact_geometry;
        transaction.canonical.review_reason = (!exact_geometry)
            .then(|| "offline row lacks complete field-level geometry".to_string());
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RawRow {
    page: usize,
    line_on_page: usize,
    text: String,
    bbox: [f32; 4],
    blocks: Vec<crate::pdf::TextBlock>,
}

impl RawRow {
    fn from_blocks(page: usize, line_on_page: usize, blocks: &[crate::pdf::TextBlock]) -> Self {
        let mut ordered_blocks = blocks.to_vec();
        ordered_blocks.sort_by(|left, right| {
            left.bbox[0]
                .partial_cmp(&right.bbox[0])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut text = String::new();
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for b in &ordered_blocks {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&b.text);
            min_x = min_x.min(b.bbox[0]);
            min_y = min_y.min(b.bbox[1]);
            max_x = max_x.max(b.bbox[2]);
            max_y = max_y.max(b.bbox[3]);
        }

        Self {
            page,
            line_on_page,
            text: text.trim().to_string(),
            bbox: [min_x, min_y, max_x, max_y],
            blocks: ordered_blocks,
        }
    }
}

// ---------------------------------------------------------------------------
// Row-level parsing
// ---------------------------------------------------------------------------

/// Currency regex for quick scanning before invoking the winnow parser
static AMOUNT_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"-?\$?[\d,]+\.\d{2}").unwrap());

static DATE_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?ix)\b(?:
            \d{4}[-/.]\d{1,2}[-/.]\d{1,2}
            |\d{1,2}[-/.]\d{1,2}[-/.]\d{2,4}
            |\d{1,2}\s+(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*(?:\s+\d{2,4})?
            |(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\s+\d{1,2}(?:\s+\d{2,4})?
        )\b",
    )
    .unwrap()
});

/// Balance-related keywords (case-insensitive match)
static OPENING_KW: &[&str] = &[
    "opening balance",
    "beginning balance",
    "balance brought forward",
    "balance b/f",
    "opening bal",
    "brought forward",
];

static CLOSING_KW: &[&str] = &[
    "closing balance",
    "ending balance",
    "balance carried forward",
    "balance c/f",
    "closing bal",
    "carried forward",
];

/// Max vertical gap (PDF points) for attaching a description-only continuation
/// line onto the previous offline transaction. Mirrors the Python geometry
/// `continuation_gap` upper bound.
const OFFLINE_CONTINUATION_GAP_PTS: f32 = 34.0;

fn is_balance_keyword_row(text_lower: &str) -> bool {
    OPENING_KW.iter().any(|kw| text_lower.contains(kw))
        || CLOSING_KW.iter().any(|kw| text_lower.contains(kw))
}

/// Expand `previous` with a description-only continuation row when the vertical
/// gap is non-negative and within the offline continuation budget.
fn attach_offline_continuation(previous: &mut Transaction, row: &RawRow) -> bool {
    let Some(prev_bbox) = previous.bbox else {
        return false;
    };
    let gap = row.bbox[1] - prev_bbox[3];
    if !(0.0..=OFFLINE_CONTINUATION_GAP_PTS).contains(&gap) {
        return false;
    }
    let cont = row.text.trim();
    if cont.is_empty() {
        return false;
    }
    if !previous.raw_text.ends_with(' ') && !cont.starts_with(' ') {
        previous.raw_text.push(' ');
    }
    previous.raw_text.push_str(cont);
    previous.bbox = Some([
        prev_bbox[0].min(row.bbox[0]),
        prev_bbox[1].min(row.bbox[1]),
        prev_bbox[2].max(row.bbox[2]),
        prev_bbox[3].max(row.bbox[3]),
    ]);
    if let Some(desc) = previous.field_bboxes.description.as_mut() {
        desc[0] = desc[0].min(row.bbox[0]);
        desc[1] = desc[1].min(row.bbox[1]);
        desc[2] = desc[2].max(row.bbox[2]);
        desc[3] = desc[3].max(row.bbox[3]);
    } else {
        previous.field_bboxes.description = Some(row.bbox);
    }
    true
}

/// Attach a description-only row that sits *above* a dated transaction
/// (Westpac-style preceding description).
fn attach_offline_preceding(tx: &mut Transaction, pending: &RawRow) -> bool {
    let Some(tx_bbox) = tx.bbox else {
        return false;
    };
    let gap = tx_bbox[1] - pending.bbox[3];
    if !(0.0..=OFFLINE_CONTINUATION_GAP_PTS).contains(&gap) {
        return false;
    }
    let pending_text = pending.text.trim();
    if pending_text.is_empty() {
        return false;
    }
    // Prefer prepending so description reads naturally before the dated line.
    if !tx.raw_text.contains(pending_text) {
        let mut combined = String::with_capacity(pending_text.len() + tx.raw_text.len() + 1);
        combined.push_str(pending_text);
        if !tx.raw_text.starts_with(' ') {
            combined.push(' ');
        }
        combined.push_str(&tx.raw_text);
        tx.raw_text = combined;
    }
    tx.bbox = Some([
        tx_bbox[0].min(pending.bbox[0]),
        tx_bbox[1].min(pending.bbox[1]),
        tx_bbox[2].max(pending.bbox[2]),
        tx_bbox[3].max(pending.bbox[3]),
    ]);
    if let Some(desc) = tx.field_bboxes.description.as_mut() {
        desc[0] = desc[0].min(pending.bbox[0]);
        desc[1] = desc[1].min(pending.bbox[1]);
        desc[2] = desc[2].max(pending.bbox[2]);
        desc[3] = desc[3].max(pending.bbox[3]);
    } else {
        tx.field_bboxes.description = Some(pending.bbox);
    }
    true
}

fn parse_rows_into_transactions(rows: &[RawRow]) -> (Vec<Transaction>, Decimal, Decimal) {
    let mut transactions = Vec::new();
    let mut opening_balance = Decimal::ZERO;
    let mut closing_balance = Decimal::ZERO;
    let mut found_opening = false;
    let mut found_closing = false;
    let mut continuity_balance: Option<Decimal> = None;
    // Description-only line waiting for the next dated amount row (preceding).
    let mut pending_preceding: Option<RawRow> = None;

    for row in rows {
        let text_lower = row.text.to_lowercase();

        // Check for opening/closing balance lines
        let is_opening = OPENING_KW.iter().any(|kw| text_lower.contains(kw));
        let is_closing = CLOSING_KW.iter().any(|kw| text_lower.contains(kw));

        // Extract all amounts from this line
        let amounts = extract_amounts(&row.text);

        if is_opening && !amounts.is_empty() && !found_opening {
            opening_balance = *amounts.last().unwrap();
            continuity_balance = Some(opening_balance);
            found_opening = true;
            pending_preceding = None;
            continue;
        }
        if is_closing && !amounts.is_empty() && !found_closing {
            closing_balance = *amounts.last().unwrap();
            found_closing = true;
            pending_preceding = None;
            continue;
        }

        // Description-only: prefer below-date wrap on previous tx; otherwise
        // hold as preceding description for the next dated row.
        if !DATE_RE.is_match(&row.text)
            && amounts.is_empty()
            && !is_balance_keyword_row(&text_lower)
        {
            if let Some(previous) = transactions.last_mut() {
                if attach_offline_continuation(previous, row) {
                    pending_preceding = None;
                    continue;
                }
            }
            if !row.text.trim().is_empty() {
                pending_preceding = Some(row.clone());
            }
            continue;
        }

        // Skip non-transaction rows (headers, labels, etc.)
        if !DATE_RE.is_match(&row.text) || amounts.is_empty() {
            continue;
        }

        // This row looks like a transaction: has a date and at least one amount
        let date = extract_date(&row.text);

        // Heuristic: if there are 3+ amounts, the last is likely the running balance
        // If 2 amounts, the first is debit/credit and the second is running balance
        // If 1 amount, it's a debit or credit with no running balance shown
        let (mut debit, mut credit, mut running_balance) = match amounts.len() {
            1 => {
                // Single amount - assume it's a debit (money in) if positive
                let amt = amounts[0];
                if amt >= Decimal::ZERO {
                    (Some(amt), None, None)
                } else {
                    (None, Some(amt.abs()), None)
                }
            }
            2 => {
                // Two amounts: first is debit/credit, second is running balance
                let amt = amounts[0];
                let bal = amounts[1];
                if amt >= Decimal::ZERO {
                    (Some(amt), None, Some(bal))
                } else {
                    (None, Some(amt.abs()), Some(bal))
                }
            }
            _ => {
                // 3+ amounts: try to identify debit, credit, running balance
                // Common layout: description | debit | credit | balance
                // where one of debit/credit is blank (shows as no match)
                let bal = *amounts.last().unwrap();

                // Look at the 2nd-to-last and 3rd-to-last
                // If only one non-balance amount, it's either debit or credit
                let non_bal = &amounts[..amounts.len() - 1];
                match non_bal.len() {
                    1 => {
                        let amt = non_bal[0];
                        if amt >= Decimal::ZERO {
                            (Some(amt), None, Some(bal))
                        } else {
                            (None, Some(amt.abs()), Some(bal))
                        }
                    }
                    _ => {
                        // Two amounts before balance: first=debit, second=credit (or vice versa)
                        let d = non_bal[non_bal.len() - 2];
                        let c = non_bal[non_bal.len() - 1];
                        (
                            if d != Decimal::ZERO {
                                Some(d.abs())
                            } else {
                                None
                            },
                            if c != Decimal::ZERO {
                                Some(c.abs())
                            } else {
                                None
                            },
                            Some(bal),
                        )
                    }
                }
            }
        };

        let mut field_bboxes = FieldBboxes::default();
        if let Some(spatial) = extract_spatial_amounts(row) {
            running_balance = Some(spatial.running_balance);
            field_bboxes.date = spatial.date_bbox;
            field_bboxes.description = spatial.description_bbox;
            field_bboxes.running_balance = Some(spatial.running_balance_bbox);

            if let Some(previous) = continuity_balance {
                let action = spatial.action.abs();
                let adds = (previous + action).round_dp(2) == spatial.running_balance.round_dp(2);
                let subtracts =
                    (previous - action).round_dp(2) == spatial.running_balance.round_dp(2);
                match (adds, subtracts) {
                    (true, false) => {
                        debit = Some(action);
                        credit = None;
                        field_bboxes.debit = Some(spatial.action_bbox);
                    }
                    (false, true) => {
                        debit = None;
                        credit = Some(action);
                        field_bboxes.credit = Some(spatial.action_bbox);
                    }
                    _ => {
                        tracing::warn!(
                            page = row.page,
                            line = row.line_on_page,
                            previous = %previous,
                            action = %action,
                            running = %spatial.running_balance,
                            "offline row direction is not uniquely supported by balance continuity"
                        );
                    }
                }
            }
            continuity_balance = Some(spatial.running_balance);
        } else if let Some(balance) = running_balance {
            continuity_balance = Some(balance);
        }

        let mut tx = Transaction {
            page: row.page,
            line_on_page: row.line_on_page,
            date,
            raw_text: row.text.clone(),
            debit,
            credit,
            running_balance,
            bbox: Some(row.bbox),
            field_bboxes,
            provenance: Provenance::Computed,
            category: None,
            canonical: Default::default(),
        };
        if let Some(pending) = pending_preceding.take() {
            let _ = attach_offline_preceding(&mut tx, &pending);
        }
        transactions.push(tx);
    }

    // If we didn't find explicit opening/closing, try to infer from transactions
    if !found_opening && !transactions.is_empty() {
        if let Some(first_bal) = transactions[0].running_balance {
            // opening = first_balance - first_tx_net_delta
            let net = transactions[0].debit.unwrap_or(Decimal::ZERO)
                - transactions[0].credit.unwrap_or(Decimal::ZERO);
            opening_balance = first_bal - net;
        }
    }
    if !found_closing && !transactions.is_empty() {
        if let Some(last_bal) = transactions.last().and_then(|t| t.running_balance) {
            closing_balance = last_bal;
        }
    }

    (transactions, opening_balance, closing_balance)
}

#[derive(Debug, Clone, Copy)]
struct SpatialAmounts {
    action: Decimal,
    action_bbox: [f32; 4],
    running_balance: Decimal,
    running_balance_bbox: [f32; 4],
    date_bbox: Option<[f32; 4]>,
    description_bbox: Option<[f32; 4]>,
}

fn extract_spatial_amounts(row: &RawRow) -> Option<SpatialAmounts> {
    let mut monetary_blocks: Vec<(&crate::pdf::TextBlock, Decimal)> = row
        .blocks
        .iter()
        .filter_map(|block| {
            let amounts = extract_amounts(&block.text);
            (amounts.len() == 1).then(|| (block, amounts[0]))
        })
        .collect();
    if monetary_blocks.len() < 2 {
        return None;
    }
    monetary_blocks.sort_by(|(left, _), (right, _)| {
        left.bbox[0]
            .partial_cmp(&right.bbox[0])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let (balance_block, running_balance) = monetary_blocks.pop()?;
    let action_candidates: Vec<_> = monetary_blocks
        .iter()
        .copied()
        .filter(|(_, amount)| !amount.is_zero())
        .collect();
    let (action_block, action) = match action_candidates.as_slice() {
        [only] => *only,
        [] if monetary_blocks.len() == 1 => monetary_blocks[0],
        _ => return None,
    };

    let date_bbox = row
        .blocks
        .iter()
        .find(|block| DATE_RE.is_match(&block.text))
        .map(|block| block.bbox);
    let description_limit = action_block.bbox[0].min(balance_block.bbox[0]);
    let description_bbox = union_bboxes(
        row.blocks
            .iter()
            .filter(|block| {
                block.bbox[0] < description_limit
                    && !DATE_RE.is_match(&block.text)
                    && extract_amounts(&block.text).is_empty()
            })
            .map(|block| block.bbox),
    );

    Some(SpatialAmounts {
        action,
        action_bbox: action_block.bbox,
        running_balance,
        running_balance_bbox: balance_block.bbox,
        date_bbox,
        description_bbox,
    })
}

fn union_bboxes(boxes: impl Iterator<Item = [f32; 4]>) -> Option<[f32; 4]> {
    boxes.fold(None, |accumulator, bbox| {
        Some(match accumulator {
            Some(current) => [
                current[0].min(bbox[0]),
                current[1].min(bbox[1]),
                current[2].max(bbox[2]),
                current[3].max(bbox[3]),
            ],
            None => bbox,
        })
    })
}

fn extract_amounts(text: &str) -> Vec<Decimal> {
    AMOUNT_RE
        .find_iter(text)
        .filter_map(|m| {
            let s = m.as_str();
            let mut input = s;
            parsers::parse_currency(&mut input).ok()
        })
        .collect()
}

fn extract_date(text: &str) -> String {
    if let Some(m) = DATE_RE.find(text) {
        m.as_str().to_string()
    } else {
        String::new()
    }
}

fn extract_account_number(rows: &[RawRow]) -> Option<String> {
    let acct_re = regex::Regex::new(
        r"(?i)(?:account|acct|a/c)\s*(?:no\.?|number|#)?\s*[:.]?\s*(\d[\d\s-]{4,20}\d)",
    )
    .ok()?;
    for row in rows.iter().take(20) {
        // Only check first ~20 rows (header area)
        if let Some(caps) = acct_re.captures(&row.text) {
            if let Some(m) = caps.get(1) {
                let cleaned: String = m.as_str().chars().filter(|c| c.is_ascii_digit()).collect();
                if cleaned.len() >= 6 {
                    return Some(cleaned);
                }
            }
        }
    }
    None
}

/// Split OCR plain text into synthetic per-line `TextBlock`s so the offline
/// row clusterer and multi-line continuation logic can still run.
#[cfg(any(feature = "ocr", test))]
fn ocr_text_to_line_blocks(
    page: usize,
    text: &str,
    width_pts: f32,
    height_pts: f32,
) -> Vec<crate::pdf::TextBlock> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return vec![];
    }
    let line_height = (height_pts / lines.len() as f32).max(10.0);
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let y0 = index as f32 * line_height;
            let y1 = ((index + 1) as f32 * line_height).min(height_pts.max(y0 + line_height));
            crate::pdf::TextBlock {
                page,
                bbox: [0.0, y0, width_pts, y1],
                text: line.to_string(),
                font: String::new(),
                size: 12.0,
                obj_id: None,
            }
        })
        .collect()
}

#[cfg(feature = "ocr")]
fn extract_text_via_ocr(
    pdf_path: &Path,
    page: usize,
    engine: Arc<dyn PdfEngine>,
) -> Vec<crate::pdf::TextBlock> {
    tracing::info!(
        "[offline_parser] Attempting to render page {} for OCR fallback...",
        page
    );
    let rendered = match engine.render_page(pdf_path, page, 300.0) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("[offline_parser] Failed to render page for OCR: {}", e);
            return vec![];
        }
    };

    // T4: Run real OCR via OcrsEngine
    let ocr_config = crate::extractors::ocrs_engine::OcrsConfig::default();
    let ocr_engine = crate::extractors::ocrs_engine::OcrsEngine::new(ocr_config);
    match ocr_engine.extract_text_from_image(&rendered.png_bytes) {
        Ok(text) => {
            if text.trim().is_empty() {
                tracing::warn!("[offline_parser] OCR returned empty text for page {}", page);
                return vec![];
            }
            tracing::info!(
                "[offline_parser] OCR extracted {} chars from page {}",
                text.len(),
                page
            );
            // Split OCR text into per-line blocks with synthetic vertical
            // layout so multi-line transactions and continuation attach can
            // still operate. True glyph geometry is unavailable from the
            // basic OCR text API.
            ocr_text_to_line_blocks(page, &text, rendered.width_pts, rendered.height_pts)
        }
        Err(e) => {
            tracing::warn!(
                "[offline_parser] OCR extraction failed for page {}: {}",
                page,
                e
            );
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn extract_amounts_parses_various_formats() {
        let amounts = extract_amounts("Payment $1,234.56 Balance $5,678.90");
        assert_eq!(amounts.len(), 2);
        assert_eq!(amounts[0], dec!(1234.56));
        assert_eq!(amounts[1], dec!(5678.90));
    }

    #[test]
    fn extract_amounts_handles_negative() {
        let amounts = extract_amounts("-$500.00 remainder 1,000.00");
        assert_eq!(amounts.len(), 2);
        assert_eq!(amounts[0], dec!(-500.00));
        assert_eq!(amounts[1], dec!(1000.00));
    }

    #[test]
    fn extract_date_finds_date() {
        let d = extract_date("15/01/2024 Payment to grocery store $42.50");
        assert_eq!(d, "15/01/2024");
    }

    #[test]
    fn extract_date_finds_iso_and_hyphenated_dates() {
        assert_eq!(
            extract_date("2026-03-01 Direct Deposit $2,256.02"),
            "2026-03-01"
        );
        assert_eq!(
            extract_date("01-03-2026 Direct Deposit $2,256.02"),
            "01-03-2026"
        );
    }

    #[test]
    fn extract_date_finds_month_name() {
        let d = extract_date("15 Jan 2024 Direct debit $100.00");
        assert_eq!(d, "15 Jan 2024");
    }

    #[test]
    fn extract_date_finds_yearless_au_statement_dates() {
        assert_eq!(
            extract_date("13 Jan METRO PRAHRAN 13.29 41,219.03 CR"),
            "13 Jan"
        );
        assert_eq!(
            extract_date("Mar 17 CLOSING BALANCE 35,308.14 CR"),
            "Mar 17"
        );
    }

    #[test]
    fn parse_rows_identifies_opening_closing() {
        let rows = vec![
            RawRow {
                page: 0,
                line_on_page: 0,
                text: "Opening Balance $1,000.00".into(),
                bbox: [0.0; 4],
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 1,
                text: "15/01/2024 Direct Deposit $500.00 $1,500.00".into(),
                bbox: [0.0; 4],
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 2,
                text: "16/01/2024 ATM Withdrawal -$200.00 $1,300.00".into(),
                bbox: [0.0; 4],
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 3,
                text: "Closing Balance $1,300.00".into(),
                bbox: [0.0; 4],
                blocks: Vec::new(),
            },
        ];
        let (txs, opening, closing) = parse_rows_into_transactions(&rows);
        assert_eq!(opening, dec!(1000.00));
        assert_eq!(closing, dec!(1300.00));
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn parse_rows_attaches_below_date_description_continuation() {
        let rows = vec![
            RawRow {
                page: 0,
                line_on_page: 0,
                text: "Opening Balance $1,000.00".into(),
                bbox: [0.0, 0.0, 200.0, 10.0],
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 1,
                text: "15/01/2024 Payment to Merchant XYZ $50.00 $1,050.00".into(),
                bbox: [0.0, 40.0, 400.0, 52.0],
                blocks: Vec::new(),
            },
            // Below-date wrap: within 34pt of previous bottom (52) → top 60.
            RawRow {
                page: 0,
                line_on_page: 2,
                text: "Ref 1394711 Osko".into(),
                bbox: [40.0, 60.0, 200.0, 72.0],
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 3,
                text: "16/01/2024 Coffee Shop $5.00 $1,045.00".into(),
                bbox: [0.0, 90.0, 400.0, 102.0],
                blocks: Vec::new(),
            },
            // Far continuation must not attach (gap > 34pt).
            RawRow {
                page: 0,
                line_on_page: 4,
                text: "Orphan far away".into(),
                bbox: [40.0, 200.0, 200.0, 212.0],
                blocks: Vec::new(),
            },
        ];
        let (txs, _opening, _closing) = parse_rows_into_transactions(&rows);
        assert_eq!(txs.len(), 2);
        assert!(
            txs[0].raw_text.contains("Payment to Merchant XYZ")
                && txs[0].raw_text.contains("Ref 1394711 Osko"),
            "below-date wrap must append to previous raw_text, got {:?}",
            txs[0].raw_text
        );
        assert!(
            !txs[1].raw_text.contains("Ref 1394711"),
            "continuation must not attach to next tx: {:?}",
            txs[1].raw_text
        );
        assert!(
            !txs.iter().any(|t| t.raw_text.contains("Orphan far away")),
            "out-of-gap orphan must be dropped"
        );
        let bbox = txs[0].bbox.expect("bbox");
        assert!(bbox[3] >= 72.0, "bbox must expand to cover continuation");
    }

    #[test]
    fn parse_rows_rejects_above_row_as_continuation() {
        let rows = vec![
            RawRow {
                page: 0,
                line_on_page: 0,
                text: "15/01/2024 Deposit $50.00 $150.00".into(),
                bbox: [0.0, 100.0, 400.0, 112.0],
                blocks: Vec::new(),
            },
            // "Above" the previous bottom — negative gap must not attach.
            RawRow {
                page: 0,
                line_on_page: 1,
                text: "Should not attach".into(),
                bbox: [40.0, 50.0, 200.0, 62.0],
                blocks: Vec::new(),
            },
        ];
        let (txs, _, _) = parse_rows_into_transactions(&rows);
        assert_eq!(txs.len(), 1);
        assert!(
            !txs[0].raw_text.contains("Should not attach"),
            "negative Y gap must not merge: {:?}",
            txs[0].raw_text
        );
    }

    #[test]
    fn parse_rows_attaches_preceding_description_to_next_date() {
        let rows = vec![
            RawRow {
                page: 0,
                line_on_page: 0,
                text: "14/01/2024 Prior Purchase $10.00 $510.00".into(),
                bbox: [0.0, 40.0, 400.0, 52.0],
                blocks: Vec::new(),
            },
            // Far enough that it is NOT a below-wrap of the prior row.
            RawRow {
                page: 0,
                line_on_page: 1,
                text: "Withdrawal-Osko Payment 1394711".into(),
                bbox: [40.0, 100.0, 280.0, 112.0],
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 2,
                text: "25/09/23 $25.00 $535.00".into(),
                bbox: [0.0, 120.0, 400.0, 132.0],
                blocks: Vec::new(),
            },
        ];
        let (txs, _, _) = parse_rows_into_transactions(&rows);
        assert_eq!(txs.len(), 2);
        assert!(
            !txs[0].raw_text.contains("Osko"),
            "preceding desc must not attach to prior far tx: {:?}",
            txs[0].raw_text
        );
        assert!(
            txs[1].raw_text.contains("Withdrawal-Osko Payment 1394711"),
            "preceding desc must prepend to next dated row: {:?}",
            txs[1].raw_text
        );
    }

    #[test]
    fn ocr_text_splits_into_per_line_blocks() {
        let blocks = ocr_text_to_line_blocks(
            0,
            "15/01/2024 Payment $50.00 $150.00\nRef 1394711\n16/01/2024 Coffee $5.00 $145.00\n",
            400.0,
            300.0,
        );
        assert_eq!(blocks.len(), 3);
        assert!(blocks[0].text.contains("Payment"));
        assert_eq!(blocks[1].text, "Ref 1394711");
        assert!(blocks[0].bbox[3] <= blocks[1].bbox[1] + 0.01);
        assert!(blocks[1].bbox[3] <= blocks[2].bbox[1] + 0.01);
    }

    #[test]
    fn extract_account_number_finds_number() {
        let rows = vec![
            RawRow {
                page: 0,
                line_on_page: 0,
                text: "Bank of Test".into(),
                bbox: [0.0; 4],
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 1,
                text: "Account No. 123-456-789".into(),
                bbox: [0.0; 4],
                blocks: Vec::new(),
            },
        ];
        let acct = extract_account_number(&rows);
        assert_eq!(acct, Some("123456789".to_string()));
    }

    #[test]
    fn representative_two_page_statement_extracts_all_rows_offline() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/stress_pdfs/Standard_Bank_Statement_01.pdf");
        let engine = Arc::new(crate::pdf::native_engine::OxidizePdfEngine::new());

        let statement = parse_statement_offline(&fixture, engine).unwrap();

        assert_eq!(statement.total_pages, 2);
        assert_eq!(statement.transactions.len(), 30);
        assert_eq!(statement.opening_balance, dec!(10000.00));
        assert_eq!(statement.closing_balance, dec!(19741.65));
        assert_eq!(statement.transactions[0].debit, Some(dec!(2256.02)));
        assert_eq!(statement.transactions[0].credit, None);
        assert_eq!(statement.transactions[1].debit, None);
        assert_eq!(statement.transactions[1].credit, Some(dec!(28.89)));
        assert!(statement.transactions[0].field_bboxes.date.is_some());
        assert!(statement.transactions[0].field_bboxes.description.is_some());
        assert!(statement.transactions[0].field_bboxes.debit.is_some());
        assert!(statement.transactions[0]
            .field_bboxes
            .running_balance
            .is_some());

        let mut expected = statement.opening_balance;
        for transaction in &statement.transactions {
            assert!(!transaction.date.is_empty());
            assert!(transaction.bbox.is_some());
            expected = (expected + transaction.delta_in() - transaction.delta_out()).round_dp(2);
            assert_eq!(transaction.running_balance, Some(expected));
        }
        assert_eq!(expected, statement.closing_balance);
    }

    #[test]
    fn parse_statement_from_geom() {
        use crate::extractors::geometry::GeometrySource;
        let geoms = vec![
            LineGeometry {
                page: 0,
                line_on_page: 0,
                bbox: [0.0; 4],
                text: "Opening Balance $100.00".to_string(),
                confidence: 1.0,
                source: GeometrySource::TextLayer,
            },
            LineGeometry {
                page: 0,
                line_on_page: 1,
                bbox: [0.0; 4],
                text: "15/01/2024 Deposit $50.00 $150.00".to_string(),
                confidence: 1.0,
                source: GeometrySource::TextLayer,
            },
        ];

        let stmt = parse_statement_from_geometry(&geoms, 1).unwrap();
        assert_eq!(stmt.opening_balance, dec!(100.00));
        assert_eq!(stmt.transactions.len(), 1);
        assert_eq!(stmt.transactions[0].debit, Some(dec!(50.00)));
        assert_eq!(stmt.transactions[0].running_balance, Some(dec!(150.00)));
    }

    #[test]
    fn parse_rows_one_amount_and_three_amounts() {
        let rows = vec![
            RawRow {
                page: 0,
                line_on_page: 0,
                bbox: [0.0; 4],
                text: "15/01/2024 Deposit $50.00".into(),
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 1,
                bbox: [0.0; 4],
                text: "16/01/2024 Fee -$10.00".into(),
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 2,
                bbox: [0.0; 4],
                text: "17/01/2024 Deposit $100.00 $0.00 $140.00".into(),
                blocks: Vec::new(),
            },
            RawRow {
                page: 0,
                line_on_page: 3,
                bbox: [0.0; 4],
                text: "18/01/2024 Withdrawal $0.00 $20.00 $120.00".into(),
                blocks: Vec::new(),
            },
        ];

        let (txs, _opening, _closing) = parse_rows_into_transactions(&rows);
        assert_eq!(txs.len(), 4);
        assert_eq!(txs[0].debit, Some(dec!(50.00)));
        assert_eq!(txs[0].credit, None);
        assert_eq!(txs[0].running_balance, None);

        assert_eq!(txs[1].debit, None);
        assert_eq!(txs[1].credit, Some(dec!(10.00)));

        assert_eq!(txs[2].debit, Some(dec!(100.00)));
        assert_eq!(txs[2].credit, None);
        assert_eq!(txs[2].running_balance, Some(dec!(140.00)));

        assert_eq!(txs[3].debit, None);
        assert_eq!(txs[3].credit, Some(dec!(20.00)));
        assert_eq!(txs[3].running_balance, Some(dec!(120.00)));
    }

    #[test]
    fn parse_rows_inference() {
        let rows = vec![RawRow {
            page: 0,
            line_on_page: 0,
            bbox: [0.0; 4],
            text: "15/01/2024 Deposit $50.00 $150.00".into(),
            blocks: Vec::new(),
        }];
        let (_txs, opening, closing) = parse_rows_into_transactions(&rows);
        assert_eq!(opening, dec!(100.00));
        assert_eq!(closing, dec!(150.00));
    }
}
