use super::geometry::*;
use crate::engine::model::Transaction;
use std::sync::Arc;

pub struct MergeReport {
    pub transactions: Vec<Transaction>,
    pub coverage_pct: f32,
    pub unmatched_count: usize,
}

pub struct HybridMerger {
    pub providers: Vec<Arc<dyn GeometryProvider>>,
}

impl HybridMerger {
    pub fn new(providers: Vec<Arc<dyn GeometryProvider>>) -> Self {
        Self { providers }
    }

    pub fn merge(&self, semantic: Vec<Transaction>, geometries: Vec<LineGeometry>) -> MergeReport {
        let mut merged = Vec::new();
        let mut unmatched_count = 0;

        for mut tx in semantic {
            let mut best_match: Option<LineGeometry> = None;
            let mut best_score = i32::MIN;

            // Prefer text/multi-line soft matches over bare line_on_page, then
            // source priority BankTemplate > TextLayer > Ocr, then confidence,
            // then leftmost bbox (Approach 1.5 D-N4).
            for geo in &geometries {
                if geo.page != tx.page {
                    continue;
                }
                let Some(text_score) = Self::text_match_score(geo, &tx) else {
                    continue;
                };
                let score = text_score + Self::score_geometry(geo);
                if best_match.is_none() || score > best_score {
                    best_match = Some(geo.clone());
                    best_score = score;
                }
            }

            // Stage 7.5: only overwrite the existing bbox when the geometry
            // provider has higher-trust source data (a bank template). For
            // anything else we prefer Document AI's bbox (which already
            // matches the entity that produced the row's content).
            if let Some(m) = best_match {
                let prefer_geo =
                    matches!(m.source, GeometrySource::BankTemplate { .. }) || tx.bbox.is_none();
                if prefer_geo {
                    tx.bbox = Some(m.bbox);
                }
                merged.push(tx);
            } else {
                if tx.bbox.is_none() {
                    unmatched_count += 1;
                }
                merged.push(tx);
            }
        }

        let coverage_pct = if merged.is_empty() {
            0.0
        } else {
            ((merged.len() - unmatched_count) as f32 / merged.len() as f32) * 100.0
        };

        MergeReport {
            transactions: merged,
            coverage_pct,
            unmatched_count,
        }
    }

    fn score_geometry(geo: &LineGeometry) -> i32 {
        let mut score = 0;
        match &geo.source {
            GeometrySource::BankTemplate { .. } => score += 3000,
            GeometrySource::TextLayer => score += 2000,
            GeometrySource::Ocr => score += 1000,
        }
        score += (geo.confidence * 100.0) as i32;
        // leftmost bbox is better
        score -= geo.bbox[0] as i32;
        score
    }

    /// Score how well geometry text aligns with a semantic transaction.
    /// Returns `None` when the geometry should not be considered.
    fn text_match_score(geo: &LineGeometry, tx: &Transaction) -> Option<i32> {
        let geo_norm = normalize_match_text(&geo.text);
        let raw_norm = normalize_match_text(&tx.raw_text);
        let date_norm = normalize_match_text(&tx.date);

        if !geo_norm.is_empty() && geo_norm == raw_norm {
            return Some(50_000);
        }

        // Multi-line: geometry is often a single line while raw_text includes
        // wrap continuations (or vice versa).
        if !geo_norm.is_empty() && !raw_norm.is_empty() {
            if raw_norm.contains(&geo_norm) || geo_norm.contains(&raw_norm) {
                return Some(40_000);
            }
            let geo_tokens = significant_tokens(&geo_norm);
            let raw_tokens = significant_tokens(&raw_norm);
            if !geo_tokens.is_empty() && !raw_tokens.is_empty() {
                let overlap = geo_tokens
                    .iter()
                    .filter(|t| raw_tokens.iter().any(|r| r == *t))
                    .count();
                let min_len = geo_tokens.len().min(raw_tokens.len());
                if overlap >= 2 && overlap * 2 >= min_len {
                    return Some(30_000 + (overlap as i32) * 100);
                }
                // Date + at least one shared content token (common for multi-line).
                if !date_norm.is_empty()
                    && (geo_norm.contains(&date_norm) || raw_norm.starts_with(&date_norm))
                    && overlap >= 1
                {
                    return Some(25_000 + (overlap as i32) * 50);
                }
            }
        }

        // Weak fallback: same line index only when no stronger text match.
        if geo.line_on_page == tx.line_on_page {
            return Some(5_000);
        }
        None
    }
}

fn normalize_match_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn significant_tokens(normalized: &str) -> Vec<String> {
    normalized
        .split_whitespace()
        .filter(|token| {
            let t = *token;
            t.len() >= 2
                && !matches!(
                    t,
                    "cr" | "dr" | "aud" | "usd" | "the" | "and" | "to" | "for" | "of"
                )
                && !t
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '/' || c == '-')
        })
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::engine::model::Provenance;

    #[test]
    fn test_merge_and_tiebreak() -> anyhow::Result<()> {
        let merger = HybridMerger::new(vec![]);

        let tx1 = Transaction {
            page: 0,
            line_on_page: 0,
            date: "2026-05-25".into(),
            raw_text: "Match me".into(),
            debit: None,
            credit: None,
            running_balance: None,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::DocumentAI { confidence: 0.9 },
            category: None,
            canonical: Default::default(),
        };

        let geo1 = LineGeometry {
            page: 0,
            line_on_page: 0,
            text: "Match me".into(),
            bbox: [10.0, 10.0, 100.0, 20.0],
            confidence: 0.9,
            source: GeometrySource::TextLayer,
        };

        let geo2 = LineGeometry {
            page: 0,
            line_on_page: 0,
            text: "Match me".into(),
            bbox: [12.0, 10.0, 100.0, 20.0], // Slightly more right
            confidence: 0.9,
            source: GeometrySource::BankTemplate {
                template_id: "chase".into(),
            },
        };

        let semantic = vec![tx1];
        let geometries = vec![geo1, geo2]; // geo2 should win due to BankTemplate source priority

        let report = merger.merge(semantic, geometries);
        assert_eq!(report.unmatched_count, 0);
        assert_eq!(report.coverage_pct, 100.0);
        assert_eq!(
            report.transactions[0]
                .bbox
                .ok_or_else(|| anyhow::anyhow!("No bbox"))?[0],
            12.0
        ); // geo2 won
        Ok(())
    }

    #[test]
    fn merge_matches_multiline_raw_text_to_single_line_geometry() -> anyhow::Result<()> {
        let merger = HybridMerger::new(vec![]);
        let tx = Transaction {
            page: 0,
            line_on_page: 9, // deliberately does not match geometry line
            date: "15/01/2024".into(),
            raw_text: "15/01/2024 Payment to Merchant Ref 1394711 Osko".into(),
            debit: None,
            credit: Some(rust_decimal_macros::dec!(50.00)),
            running_balance: Some(rust_decimal_macros::dec!(1050.00)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::DocumentAI { confidence: 0.85 },
            category: None,
            canonical: Default::default(),
        };
        let geo = LineGeometry {
            page: 0,
            line_on_page: 3,
            text: "15/01/2024 Payment to Merchant 50.00 1050.00".into(),
            bbox: [20.0, 100.0, 400.0, 112.0],
            confidence: 0.95,
            source: GeometrySource::TextLayer,
        };
        let report = merger.merge(vec![tx], vec![geo]);
        assert_eq!(report.unmatched_count, 0);
        assert_eq!(
            report.transactions[0]
                .bbox
                .ok_or_else(|| anyhow::anyhow!("No bbox"))?,
            [20.0, 100.0, 400.0, 112.0]
        );
        Ok(())
    }

    #[test]
    fn merge_prefers_text_over_conflicting_line_index() -> anyhow::Result<()> {
        let merger = HybridMerger::new(vec![]);
        let tx = Transaction {
            page: 0,
            line_on_page: 1,
            date: "01/02/2024".into(),
            raw_text: "01/02/2024 Target Row Alpha".into(),
            debit: Some(rust_decimal_macros::dec!(1.00)),
            credit: None,
            running_balance: None,
            bbox: None,
            field_bboxes: Default::default(),
            provenance: Provenance::DocumentAI { confidence: 0.9 },
            category: None,
            canonical: Default::default(),
        };
        let wrong_line = LineGeometry {
            page: 0,
            line_on_page: 1,
            text: "01/02/2024 Unrelated Other Merchant".into(),
            bbox: [10.0, 50.0, 100.0, 60.0],
            confidence: 0.99,
            source: GeometrySource::TextLayer,
        };
        let right_text = LineGeometry {
            page: 0,
            line_on_page: 7,
            text: "01/02/2024 Target Row Alpha 1.00".into(),
            bbox: [10.0, 200.0, 100.0, 210.0],
            confidence: 0.80,
            source: GeometrySource::TextLayer,
        };
        let report = merger.merge(vec![tx], vec![wrong_line, right_text]);
        assert_eq!(
            report.transactions[0]
                .bbox
                .ok_or_else(|| anyhow::anyhow!("No bbox"))?[1],
            200.0,
            "must prefer overlapping text over bare line_on_page"
        );
        Ok(())
    }
}
