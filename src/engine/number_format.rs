//! Locale-aware number formatting (Stage 8 / Item #12).
//!
//! Bank statements vary in how they render numbers:
//!   - "$1,234.56"  US/AU/UK
//!   - "€1.234,56"  EU
//!   - "1234.56"    plain
//!   - "(50.00)"    accounting parens for negatives
//!   - "50.00-"     trailing-sign for negatives
//!
//! When the user edits one of these we want the new value to render with
//! the exact same pattern; otherwise even a "right" math edit will show up
//! as a forensic anomaly (comma -> space, e.g.).
//!
//! This module mirrors the same logic that Python's
//! `_detect_number_format` / `_format_number` use, so the value the GUI
//! shows the user matches what the binary editor will write to the PDF.

use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NegativeStyle {
    /// `-50.00`
    Minus,
    /// `(50.00)`
    Paren,
    /// `50.00-`
    TrailingMinus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CurrencyPosition {
    /// `$1,234.56` (default).
    Leading,
    /// `1,234.56 $`.
    Trailing,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NumberFormat {
    pub currency: String,
    pub currency_position: CurrencyPosition,
    pub thousand_sep: String,
    pub decimal_sep: String,
    pub negative_style: NegativeStyle,
    pub decimals: u32,
}

impl Default for NumberFormat {
    fn default() -> Self {
        Self {
            currency: String::new(),
            currency_position: CurrencyPosition::Leading,
            thousand_sep: ",".into(),
            decimal_sep: ".".into(),
            negative_style: NegativeStyle::Minus,
            decimals: 2,
        }
    }
}

/// Parse `old_text` and return the format pattern it uses. See module docs
/// for the patterns we recognise.
pub fn detect_format(old_text: &str) -> NumberFormat {
    let mut fmt = NumberFormat::default();
    let trimmed = old_text.trim();
    if trimmed.is_empty() {
        return fmt;
    }

    let mut working = trimmed.to_string();

    // Negative style.
    if working.starts_with('(') && working.ends_with(')') {
        fmt.negative_style = NegativeStyle::Paren;
        working = working[1..working.len() - 1].to_string();
    } else if working.ends_with('-') && !working.starts_with('-') {
        fmt.negative_style = NegativeStyle::TrailingMinus;
        working.pop();
    }

    // Currency. Single-character symbols only. Stage 14c / Item #11:
    // remember whether it appeared before or after the digits so the
    // formatter can reproduce the original placement.
    for sym in ["$", "€", "£", "¥"] {
        if let Some(idx) = working.find(sym) {
            fmt.currency = sym.to_string();
            // Look at the first ASCII digit position to decide leading/trailing.
            let digit_idx = working
                .char_indices()
                .find(|(_, c)| c.is_ascii_digit())
                .map(|(i, _)| i);
            fmt.currency_position = match digit_idx {
                Some(d) if idx > d => CurrencyPosition::Trailing,
                _ => CurrencyPosition::Leading,
            };
            working = working.replace(sym, "");
            break;
        }
    }

    let stripped: String = working.chars().filter(|c| !c.is_whitespace()).collect();
    let stripped = stripped.trim_matches('-').to_string();
    let digits_only: String = stripped.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits_only.is_empty() {
        return fmt;
    }

    let last_dot = stripped.rfind('.');
    let last_comma = stripped.rfind(',');
    match (last_dot, last_comma) {
        (Some(d), Some(c)) if d > c => {
            fmt.thousand_sep = ",".into();
            fmt.decimal_sep = ".".into();
        }
        (Some(d), Some(c)) if c > d => {
            fmt.thousand_sep = ".".into();
            fmt.decimal_sep = ",".into();
        }
        (Some(d), None) => {
            let right = &stripped[d + 1..];
            // Heuristic: "1.234" looks like thousands sep; "12.34" like decimals.
            if right.len() == 3
                && digits_only.len() >= 4
                && right.chars().all(|c| c.is_ascii_digit())
            {
                fmt.thousand_sep = ".".into();
                fmt.decimal_sep = ",".into();
                fmt.decimals = 0;
            } else {
                fmt.thousand_sep = String::new();
                fmt.decimal_sep = ".".into();
            }
        }
        (None, Some(c)) => {
            let right = &stripped[c + 1..];
            if right.len() == 3
                && digits_only.len() >= 4
                && right.chars().all(|c| c.is_ascii_digit())
            {
                fmt.thousand_sep = ",".into();
                fmt.decimal_sep = ".".into();
                fmt.decimals = 0;
            } else {
                fmt.thousand_sep = String::new();
                fmt.decimal_sep = ",".into();
            }
        }
        _ => {
            fmt.thousand_sep = String::new();
            fmt.decimal_sep = ".".into();
            fmt.decimals = 0;
        }
    }

    // Decimal place count.
    if !fmt.decimal_sep.is_empty() {
        if let Some(idx) = stripped.rfind(fmt.decimal_sep.as_str()) {
            let right = &stripped[idx + fmt.decimal_sep.len()..];
            let digits: String = right.chars().filter(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                fmt.decimals = digits.len() as u32;
            }
        }
    }

    fmt
}

/// Apply `fmt` to `value`. Returns a string like `"$1,234.56"` or
/// `"(50.00)"` depending on the format.
pub fn format_decimal(value: Decimal, fmt: &NumberFormat) -> String {
    let is_negative = value.is_sign_negative();
    let abs = value.abs();
    let scaled = abs.round_dp(fmt.decimals);

    // Build integer / fractional parts via string surgery on Decimal's Display.
    // (Decimal::to_string() honours scale, so we trim/extend as needed.)
    let raw = format!("{scaled}");
    let (int_part, frac_part) = match raw.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (raw, String::new()),
    };

    // Insert thousand separators every 3 digits in `int_part`.
    let int_with_sep = if fmt.thousand_sep.is_empty() {
        int_part
    } else {
        let bytes: Vec<char> = int_part.chars().rev().collect();
        let chunks: Vec<String> = bytes
            .chunks(3)
            .map(|c| c.iter().rev().collect::<String>())
            .collect();

        chunks
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(&fmt.thousand_sep)
    };

    // Pad / trim fractional part to fmt.decimals.
    let frac = if fmt.decimals == 0 {
        String::new()
    } else if frac_part.len() >= fmt.decimals as usize {
        frac_part[..fmt.decimals as usize].to_string()
    } else {
        let mut s = frac_part;
        while s.len() < fmt.decimals as usize {
            s.push('0');
        }
        s
    };

    let mut body = int_with_sep;
    if !frac.is_empty() {
        body.push_str(&fmt.decimal_sep);
        body.push_str(&frac);
    }
    if !fmt.currency.is_empty() {
        body = match fmt.currency_position {
            CurrencyPosition::Leading => format!("{}{}", fmt.currency, body),
            CurrencyPosition::Trailing => format!("{} {}", body, fmt.currency),
        };
    }

    if !is_negative {
        return body;
    }
    match fmt.negative_style {
        NegativeStyle::Minus => format!("-{body}"),
        NegativeStyle::Paren => format!("({body})"),
        NegativeStyle::TrailingMinus => format!("{body}-"),
    }
}

/// Format `value` using the format inferred from `original`. Convenience.
pub fn format_like(value: Decimal, original: &str) -> String {
    let fmt = detect_format(original);
    format_decimal(value, &fmt)
}

/// Stage 14c / Item #13: infer a format from a list of neighbouring
/// strings (typically the same column's other cells). When the cell
/// being edited has empty or ambiguous content, fall back to the
/// dominant format among the neighbours.
///
/// "Dominant" = the format whose `(currency, currency_position,
/// thousand_sep, decimal_sep, negative_style, decimals)` tuple appears
/// most often. Ties are broken arbitrarily.
pub fn detect_format_from_neighbours(neighbours: &[&str]) -> NumberFormat {
    let mut counts: std::collections::HashMap<
        (String, CurrencyPosition, String, String, NegativeStyle, u32),
        u32,
    > = std::collections::HashMap::new();
    for n in neighbours {
        let trimmed = n.trim();
        if trimmed.is_empty() {
            continue;
        }
        let f = detect_format(trimmed);
        let key = (
            f.currency.clone(),
            f.currency_position,
            f.thousand_sep.clone(),
            f.decimal_sep.clone(),
            f.negative_style.clone(),
            f.decimals,
        );
        *counts.entry(key).or_insert(0) += 1;
    }
    let best = counts.into_iter().max_by_key(|(_, c)| *c);
    match best {
        Some((
            (currency, currency_position, thousand_sep, decimal_sep, negative_style, decimals),
            _,
        )) => NumberFormat {
            currency,
            currency_position,
            thousand_sep,
            decimal_sep,
            negative_style,
            decimals,
        },
        None => NumberFormat::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn detect_us_style() {
        let fmt = detect_format("$1,234.56");
        assert_eq!(fmt.currency, "$");
        assert_eq!(fmt.thousand_sep, ",");
        assert_eq!(fmt.decimal_sep, ".");
        assert_eq!(fmt.decimals, 2);
    }

    #[test]
    fn detect_eu_style() {
        let fmt = detect_format("€1.234,56");
        assert_eq!(fmt.currency, "€");
        assert_eq!(fmt.thousand_sep, ".");
        assert_eq!(fmt.decimal_sep, ",");
        assert_eq!(fmt.decimals, 2);
    }

    #[test]
    fn detect_plain_decimal() {
        let fmt = detect_format("100.50");
        assert_eq!(fmt.currency, "");
        assert_eq!(fmt.thousand_sep, "");
        assert_eq!(fmt.decimal_sep, ".");
        assert_eq!(fmt.decimals, 2);
    }

    #[test]
    fn detect_paren_negative() {
        let fmt = detect_format("($50.00)");
        assert_eq!(fmt.negative_style, NegativeStyle::Paren);
        assert_eq!(fmt.currency, "$");
    }

    #[test]
    fn detect_trailing_minus() {
        let fmt = detect_format("50.00-");
        assert_eq!(fmt.negative_style, NegativeStyle::TrailingMinus);
    }

    #[test]
    fn detect_no_decimals() {
        let fmt = detect_format("1,234");
        assert_eq!(fmt.thousand_sep, ",");
        assert_eq!(fmt.decimals, 0);
    }

    #[test]
    fn round_trip_us() {
        let fmt = detect_format("$1,234.56");
        assert_eq!(format_decimal(dec!(1234.56), &fmt), "$1,234.56");
        assert_eq!(format_decimal(dec!(50), &fmt), "$50.00");
        assert_eq!(format_decimal(dec!(1000000), &fmt), "$1,000,000.00");
    }

    #[test]
    fn round_trip_eu() {
        let fmt = detect_format("€1.234,56");
        assert_eq!(format_decimal(dec!(1234.56), &fmt), "€1.234,56");
        assert_eq!(format_decimal(dec!(7654321.89), &fmt), "€7.654.321,89");
    }

    #[test]
    fn round_trip_paren_negative() {
        let fmt = detect_format("$1,234.56");
        // Same fmt with explicit paren negatives:
        let mut paren_fmt = fmt.clone();
        paren_fmt.negative_style = NegativeStyle::Paren;
        assert_eq!(format_decimal(dec!(-50), &paren_fmt), "($50.00)");
    }

    #[test]
    fn format_like_preserves_original_pattern() {
        // The most important regression: the user changes 242.83 to 999.99
        // in a US-formatted statement. The new value must come out US-formatted.
        assert_eq!(format_like(dec!(999.99), "$242.83"), "$999.99");
        // And in EU-style:
        assert_eq!(format_like(dec!(999.99), "€242,83"), "€999,99");
    }

    #[test]
    fn negative_round_trips_under_minus_style() {
        let fmt = detect_format("-50.00");
        assert_eq!(format_decimal(dec!(-50), &fmt), "-50.00");
    }

    #[test]
    fn test_trailing_currency() {
        let fmt = detect_format("1,234.56 $");
        assert_eq!(fmt.currency, "$");
        assert_eq!(fmt.currency_position, CurrencyPosition::Trailing);
        assert_eq!(format_decimal(dec!(1234.56), &fmt), "1,234.56 $");
    }

    #[test]
    fn test_empty_and_invalid() {
        let fmt = detect_format("");
        assert_eq!(fmt, NumberFormat::default());

        let fmt2 = detect_format("   ");
        assert_eq!(fmt2, NumberFormat::default());

        let fmt3 = detect_format("abc");
        assert_eq!(fmt3, NumberFormat::default());
    }

    #[test]
    fn test_detect_ambiguous_thousands() {
        // "1.234" heuristic => thousands dot, decimal comma, 0 decimals
        let fmt1 = detect_format("1.234");
        assert_eq!(fmt1.thousand_sep, ".");
        assert_eq!(fmt1.decimal_sep, ",");
        assert_eq!(fmt1.decimals, 0);

        // "1,234" heuristic => thousands comma, decimal dot, 0 decimals
        let fmt2 = detect_format("1,234");
        assert_eq!(fmt2.thousand_sep, ",");
        assert_eq!(fmt2.decimal_sep, ".");
        assert_eq!(fmt2.decimals, 0);

        // "12.34" heuristic => no thousand sep, decimal dot, 2 decimals
        let fmt3 = detect_format("12.34");
        assert_eq!(fmt3.thousand_sep, "");
        assert_eq!(fmt3.decimal_sep, ".");
        assert_eq!(fmt3.decimals, 2);

        // "12,34" heuristic => no thousand sep, decimal comma, 2 decimals
        let fmt4 = detect_format("12,34");
        assert_eq!(fmt4.thousand_sep, "");
        assert_eq!(fmt4.decimal_sep, ",");
        assert_eq!(fmt4.decimals, 2);
    }

    #[test]
    fn test_format_from_neighbours() {
        let mut n = detect_format_from_neighbours(&[]);
        assert_eq!(n, NumberFormat::default());

        n = detect_format_from_neighbours(&["  ", "abc"]); // All ignored
        assert_eq!(n, NumberFormat::default());

        // Majority rules
        n = detect_format_from_neighbours(&["$1.23", "$4.56", "1,234"]);
        assert_eq!(n.currency, "$");
        assert_eq!(n.decimal_sep, ".");
    }

    #[test]
    fn test_format_no_decimals() {
        let fmt = detect_format("123");
        assert_eq!(fmt.decimals, 0);
        let out = format_decimal(dec!(123.456), &fmt);
        assert_eq!(out, "123"); // rounded
    }
}
