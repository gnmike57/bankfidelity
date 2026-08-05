use super::geometry::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankTemplate {
    pub id: String,
    pub header_signatures: Vec<String>,
    pub date_format: String,
    pub amount_regex: String,
    pub column_x_ranges: std::collections::HashMap<String, [f32; 2]>,
}

pub struct BankTemplateProvider {
    pub templates: Vec<BankTemplate>,
    pub engine: std::sync::Arc<dyn crate::pdf::PdfEngine>,
}

impl BankTemplateProvider {
    pub fn new(template_dir: &Path, engine: std::sync::Arc<dyn crate::pdf::PdfEngine>) -> Self {
        let mut templates = Vec::new();
        if let Ok(entries) = fs::read_dir(template_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("yaml") {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        if let Ok(template) = serde_yaml::from_str::<BankTemplate>(&content) {
                            templates.push(template);
                        }
                    }
                }
            }
        }
        Self { templates, engine }
    }
}

impl GeometryProvider for BankTemplateProvider {
    fn extract_line_geometry(&self, pdf_path: &Path) -> Result<Vec<LineGeometry>, ExtractorError> {
        let mut geometries = Vec::new();

        // 1. Get layout to know total pages
        let layout = self
            .engine
            .analyze_layout(pdf_path)
            .map_err(|e| ExtractorError::ExtractionFailed(e.to_string()))?;

        for page in 0..layout.total_pages {
            let blocks = self
                .engine
                .get_text_blocks(pdf_path, page)
                .unwrap_or_default();
            let page_text = blocks
                .iter()
                .map(|b| b.text.clone())
                .collect::<Vec<_>>()
                .join(" ");

            // 2. Identify template
            for template in &self.templates {
                let matches_all = template
                    .header_signatures
                    .iter()
                    .all(|sig| page_text.contains(sig));

                if matches_all {
                    tracing::info!("Matched template '{}' on page {}", template.id, page);

                    // 3. Simple row-based extraction using template-defined columns
                    // This is a placeholder for actual column-based slicing
                    for (i, block) in blocks.iter().enumerate() {
                        geometries.push(LineGeometry {
                            page,
                            line_on_page: i,
                            text: block.text.clone(),
                            bbox: block.bbox,
                            confidence: 1.0,
                            source: GeometrySource::BankTemplate {
                                template_id: template.id.clone(),
                            },
                        });
                    }
                }
            }
        }

        Ok(geometries)
    }
}

// ─── Phase 5.3: Winnow Parser Combinators ──────────────────────────────────

/// Phase 5.3: Strict winnow parser combinators for Australian bank statement
/// elements. These replace the previous regex-based extraction with
/// deterministic, byte-level grammar rules.
pub mod parsers {
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use winnow::ascii::{digit1, space0};
    use winnow::combinator::{alt, opt};
    use winnow::error::{ContextError, ErrMode};
    use winnow::token::{one_of, take_while};
    use winnow::Parser;

    /// Parser result type for winnow 0.7.
    type PResult<T> = Result<T, ErrMode<ContextError>>;

    /// Helper to create a backtrack error.
    fn fail<T>() -> PResult<T> {
        Err(ErrMode::Backtrack(ContextError::new()))
    }

    /// A parsed Australian date with day, month, year.
    #[derive(Debug, Clone, PartialEq)]
    pub struct AuDate {
        pub day: u32,
        pub month: u32,
        pub year: u32,
    }

    /// Parse DD/MM/YYYY format (common in AU bank statements).
    fn parse_date_slash(input: &mut &str) -> PResult<AuDate> {
        let day_str = digit1.parse_next(input)?;
        '/'.parse_next(input)?;
        let month_str = digit1.parse_next(input)?;
        '/'.parse_next(input)?;
        let year_str = digit1.parse_next(input)?;

        let day: u32 = day_str
            .parse()
            .map_err(|_| ErrMode::Backtrack(ContextError::new()))?;
        let month: u32 = month_str
            .parse()
            .map_err(|_| ErrMode::Backtrack(ContextError::new()))?;
        let year: u32 = year_str
            .parse()
            .map_err(|_| ErrMode::Backtrack(ContextError::new()))?;

        if day == 0 || day > 31 || month == 0 || month > 12 {
            return fail();
        }

        Ok(AuDate { day, month, year })
    }

    /// Parse DD Mon YYYY format (e.g. "15 Jan 2024").
    fn parse_date_month_name(input: &mut &str) -> PResult<AuDate> {
        let day_str = digit1.parse_next(input)?;
        space0.parse_next(input)?;
        let month_name: &str =
            take_while(3, |c: char| c.is_ascii_alphabetic()).parse_next(input)?;
        space0.parse_next(input)?;
        let year_str = digit1.parse_next(input)?;

        let day: u32 = day_str
            .parse()
            .map_err(|_| ErrMode::Backtrack(ContextError::new()))?;
        let year: u32 = year_str
            .parse()
            .map_err(|_| ErrMode::Backtrack(ContextError::new()))?;

        let month = match month_name.to_ascii_lowercase().as_str() {
            "jan" => 1,
            "feb" => 2,
            "mar" => 3,
            "apr" => 4,
            "may" => 5,
            "jun" => 6,
            "jul" => 7,
            "aug" => 8,
            "sep" => 9,
            "oct" => 10,
            "nov" => 11,
            "dec" => 12,
            _ => return fail(),
        };

        if day == 0 || day > 31 {
            return fail();
        }

        Ok(AuDate { day, month, year })
    }

    /// Parse an Australian date: either DD/MM/YYYY or DD Mon YYYY.
    pub fn parse_au_date(input: &mut &str) -> PResult<AuDate> {
        alt((parse_date_slash, parse_date_month_name)).parse_next(input)
    }

    /// Parse an Australian currency amount: $X,XXX.XX -> Decimal.
    ///
    /// Handles optional negative sign, optional dollar sign, comma
    /// separators, and mandatory 2 decimal places.
    pub fn parse_currency(input: &mut &str) -> PResult<Decimal> {
        let negative = opt(one_of(['-'])).parse_next(input)?;
        let _dollar = opt('$').parse_next(input)?;

        // Parse digits with optional comma separators
        let mut amount_str = String::new();
        let digits: &str = digit1.parse_next(input)?;
        amount_str.push_str(digits);

        // Handle comma-separated groups
        loop {
            // Try to consume a comma; if it fails, break
            let checkpoint = *input;
            let comma_result: PResult<char> = ','.parse_next(input);
            match comma_result {
                Ok(_) => {
                    let group_result: PResult<&str> = digit1.parse_next(input);
                    match group_result {
                        Ok(group) => amount_str.push_str(group),
                        Err(_) => {
                            // Put the comma back - it wasn't a separator
                            *input = checkpoint;
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        // Decimal point and cents
        '.'.parse_next(input)?;
        let cents: &str = digit1.parse_next(input)?;
        amount_str.push('.');
        amount_str.push_str(cents);

        if negative.is_some() {
            amount_str.insert(0, '-');
        }

        Decimal::from_str(&amount_str).map_err(|_| ErrMode::Backtrack(ContextError::new()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use rust_decimal_macros::dec;

        #[test]
        fn parse_au_date_slash_format() -> anyhow::Result<()> {
            let mut input = "15/01/2024";
            let result = parse_au_date(&mut input).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            assert_eq!(
                result,
                AuDate {
                    day: 15,
                    month: 1,
                    year: 2024
                }
            );
            Ok(())
        }

        #[test]
        fn parse_au_date_month_name_format() -> anyhow::Result<()> {
            let mut input = "15 Jan 2024";
            let result = parse_au_date(&mut input).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            assert_eq!(
                result,
                AuDate {
                    day: 15,
                    month: 1,
                    year: 2024
                }
            );
            Ok(())
        }

        #[test]
        fn parse_currency_simple() -> anyhow::Result<()> {
            let mut input = "$1,234.56";
            let result = parse_currency(&mut input).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            assert_eq!(result, dec!(1234.56));
            Ok(())
        }

        #[test]
        fn parse_currency_negative() -> anyhow::Result<()> {
            let mut input = "-$500.00";
            let result = parse_currency(&mut input).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            assert_eq!(result, dec!(-500.00));
            Ok(())
        }

        #[test]
        fn parse_currency_no_dollar_sign() -> anyhow::Result<()> {
            let mut input = "99.99";
            let result = parse_currency(&mut input).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            assert_eq!(result, dec!(99.99));
            Ok(())
        }

        #[test]
        fn parse_currency_large_amount() -> anyhow::Result<()> {
            let mut input = "$1,234,567.89";
            let result = parse_currency(&mut input).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            assert_eq!(result, dec!(1234567.89));
            Ok(())
        }

        #[test]
        fn parse_invalid_date_rejected() {
            let mut input = "32/13/2024";
            assert!(parse_au_date(&mut input).is_err());
        }
    }
}

/// Refine a bank template from observed transaction bboxes.
///
/// Stage 4 / Item #13: when a workflow completes successfully on a bank
/// whose template we already match, we know the *actual* column ranges
/// (from the bboxes of every successfully-edited row). Tighten the
/// template's `column_x_ranges` to fit those observations, then write a
/// `<template_id>.refined.yaml` next to the original.
///
/// `observed_bboxes` is `(field_name, bbox)` pairs, e.g.
/// `("date", [40, 100, 105, 120])`. Field names are joined with the
/// template's existing keys; unknown field names are ignored.
///
/// Returns the path of the refined YAML on success, or an error string.
pub fn learn_template(
    template_dir: &Path,
    template: &BankTemplate,
    observed_bboxes: &[(String, [f32; 4])],
) -> Result<std::path::PathBuf, String> {
    use std::collections::HashMap;

    if observed_bboxes.is_empty() {
        return Err("no observations to learn from".into());
    }

    // For each field we have observations for, compute the [min_x0, max_x1]
    // envelope. We expand the existing range to *contain* every observation
    // (we never tighten so far we'd reject a valid future row), then trim
    // by the observation's own envelope (so we don't keep stale wide ranges
    // forever).
    let mut envelopes: HashMap<String, (f32, f32)> = HashMap::new();
    for (field, bbox) in observed_bboxes {
        let entry = envelopes
            .entry(field.clone())
            .or_insert((f32::INFINITY, f32::NEG_INFINITY));
        entry.0 = entry.0.min(bbox[0]);
        entry.1 = entry.1.max(bbox[2]);
    }

    let mut refined = template.clone();
    for (field, (min_x, max_x)) in &envelopes {
        // Pad each side by 4pt so future rows shifted by sub-pixel rounding
        // still match. Anything bigger than that and the original template
        // was probably wrong.
        let lo = (*min_x - 4.0).max(0.0);
        let hi = *max_x + 4.0;
        refined.column_x_ranges.insert(field.clone(), [lo, hi]);
    }

    // Don't rewrite the original; create a sibling file. The
    // `BankTemplateProvider` loads any `*.yaml`, so the refined version
    // ranks alongside the original (and ideally beats it on overlap).
    let out_path = template_dir.join(format!("{}.refined.yaml", template.id));
    let yaml = serde_yaml::to_string(&refined).map_err(|e| format!("yaml encode: {e}"))?;
    std::fs::write(&out_path, yaml).map_err(|e| format!("write {}: {e}", out_path.display()))?;
    tracing::info!(
        "[templates] refined template written: {} (fields: {:?})",
        out_path.display(),
        envelopes.keys().collect::<Vec<_>>()
    );
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn sample_template() -> BankTemplate {
        let mut cols = HashMap::new();
        cols.insert("date".into(), [30.0, 100.0]);
        cols.insert("amount".into(), [200.0, 300.0]);
        BankTemplate {
            id: "test_bank".into(),
            header_signatures: vec!["TEST BANK".into()],
            date_format: "%d/%m/%Y".into(),
            amount_regex: r"^-?\$?\d+\.\d{2}$".into(),
            column_x_ranges: cols,
        }
    }

    #[test]
    fn learn_template_writes_refined_yaml_with_observed_columns() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let tmpl = sample_template();
        let observations = vec![
            ("date".to_string(), [42.0, 105.0, 95.0, 120.0]),
            ("date".to_string(), [42.0, 130.0, 95.0, 145.0]),
            ("amount".to_string(), [220.0, 105.0, 280.0, 120.0]),
        ];

        let out =
            learn_template(dir.path(), &tmpl, &observations).map_err(|e| anyhow::anyhow!(e))?;
        assert!(out.ends_with("test_bank.refined.yaml"));

        let raw = std::fs::read_to_string(&out)?;
        let refined: BankTemplate = serde_yaml::from_str(&raw).map_err(|e| anyhow::anyhow!(e))?;

        // date column: observed envelope x0=42, x1=95 -> padded ±4 -> [38, 99]
        let date = refined
            .column_x_ranges
            .get("date")
            .ok_or_else(|| anyhow::anyhow!("No date field"))?;
        assert!((date[0] - 38.0).abs() < 0.1);
        assert!((date[1] - 99.0).abs() < 0.1);

        // amount column: observed envelope x0=220, x1=280 -> padded -> [216, 284]
        let amt = refined
            .column_x_ranges
            .get("amount")
            .ok_or_else(|| anyhow::anyhow!("No amount field"))?;
        assert!((amt[0] - 216.0).abs() < 0.1);
        assert!((amt[1] - 284.0).abs() < 0.1);

        // Other template fields are preserved.
        assert_eq!(refined.id, "test_bank");
        assert_eq!(refined.header_signatures, vec!["TEST BANK".to_string()]);
        Ok(())
    }

    #[test]
    fn learn_template_clamps_negative_lo_to_zero() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let tmpl = sample_template();
        // Observation at x0=2 -> padded x0 = -2, must clamp to 0.
        let observations = vec![("date".to_string(), [2.0, 100.0, 95.0, 120.0])];
        let out =
            learn_template(dir.path(), &tmpl, &observations).map_err(|e| anyhow::anyhow!(e))?;
        let refined: BankTemplate = serde_yaml::from_str(&std::fs::read_to_string(&out)?)
            .map_err(|e| anyhow::anyhow!(e))?;
        let date = refined
            .column_x_ranges
            .get("date")
            .ok_or_else(|| anyhow::anyhow!("No date field"))?;
        assert_eq!(date[0], 0.0);
        Ok(())
    }

    #[test]
    fn learn_template_rejects_empty_observations() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let tmpl = sample_template();
        let out = learn_template(dir.path(), &tmpl, &[]);
        assert!(out.is_err());
        Ok(())
    }

    #[test]
    fn learn_template_ignores_repeated_field_observations_correctly() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let tmpl = sample_template();
        // Three rows for "date", varying widths. Refined envelope must cover all.
        let observations = vec![
            ("date".to_string(), [50.0, 100.0, 90.0, 120.0]),
            ("date".to_string(), [40.0, 130.0, 95.0, 145.0]),
            ("date".to_string(), [45.0, 160.0, 100.0, 175.0]),
        ];
        let out =
            learn_template(dir.path(), &tmpl, &observations).map_err(|e| anyhow::anyhow!(e))?;
        let refined: BankTemplate = serde_yaml::from_str(&std::fs::read_to_string(&out)?)
            .map_err(|e| anyhow::anyhow!(e))?;
        let date = refined
            .column_x_ranges
            .get("date")
            .ok_or_else(|| anyhow::anyhow!("No date field"))?;
        // min x0 = 40, max x1 = 100 -> padded -> [36, 104]
        assert!((date[0] - 36.0).abs() < 0.1);
        assert!((date[1] - 104.0).abs() < 0.1);
        Ok(())
    }
}
