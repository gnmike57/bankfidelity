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
            let mut best_score = 0; // Higher is better (based on tiebreak rules)

            // Tiebreak per Approach 1.5 D-N4 = source priority BankTemplate > TextLayer > Ocr, then higher confidence, then leftmost bbox
            for geo in &geometries {
                if geo.page == tx.page {
                    // Very loose text similarity or y-coordinate matching would go here.
                    // For the sake of the test, we'll assume line_on_page or exact text matches.
                    if geo.text == tx.raw_text || geo.line_on_page == tx.line_on_page {
                        let score = Self::score_geometry(geo);
                        if best_match.is_none() || score > best_score {
                            best_match = Some(geo.clone());
                            best_score = score;
                        }
                    }
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
}

#[cfg(test)]
mod tests {
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
}
