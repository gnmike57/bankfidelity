use crate::engine::verification::{VerificationGate, VerificationGateStatus, VerificationIntent};
use crate::pdf::engine::{bbox_overlap_fraction, PdfEngine, TextBlock};
use crate::pdf::native_engine::OxidizePdfEngine;
use std::path::Path;

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn overlapping_blocks(blocks: &[TextBlock], bbox: [f32; 4]) -> Vec<&TextBlock> {
    blocks
        .iter()
        .filter(|block| {
            bbox_overlap_fraction(bbox, block.bbox) >= 0.30
                || bbox_overlap_fraction(block.bbox, bbox) >= 0.30
        })
        .collect()
}

fn exact_matches(blocks: &[&TextBlock], expected: &str) -> usize {
    let expected = normalize_text(expected);
    blocks
        .iter()
        .filter(|block| normalize_text(&block.text) == expected)
        .count()
}

pub fn verify_intended_edit_membership(
    original_path: &Path,
    edited_path: &Path,
    intents: &[VerificationIntent],
) -> Result<VerificationGate, String> {
    if intents.is_empty() {
        return Ok(VerificationGate::optional(
            "content.intended_edit_membership",
            VerificationGateStatus::NotApplicable,
            "no old/new text intents were supplied",
        ));
    }

    let engine = OxidizePdfEngine::new();
    let mut failures = Vec::new();
    for (index, intent) in intents.iter().enumerate() {
        if intent.old_text.trim().is_empty() || intent.new_text.trim().is_empty() {
            failures.push(format!(
                "intent {index} page {} has empty old or new text identity",
                intent.page + 1
            ));
            continue;
        }
        if intent.bbox.iter().any(|coordinate| !coordinate.is_finite())
            || intent.bbox[2] <= intent.bbox[0]
            || intent.bbox[3] <= intent.bbox[1]
        {
            failures.push(format!(
                "intent {index} page {} has invalid target geometry {:?}",
                intent.page + 1,
                intent.bbox
            ));
            continue;
        }

        let source_blocks =
            engine
                .get_text_blocks(original_path, intent.page)
                .map_err(|error| {
                    format!(
                        "cannot extract original page {} text blocks: {error}",
                        intent.page + 1
                    )
                })?;
        let edited_blocks = engine
            .get_text_blocks(edited_path, intent.page)
            .map_err(|error| {
                format!(
                    "cannot extract edited page {} text blocks: {error}",
                    intent.page + 1
                )
            })?;
        let source_target = overlapping_blocks(&source_blocks, intent.bbox);
        let edited_target = overlapping_blocks(&edited_blocks, intent.bbox);
        let old_source_matches = exact_matches(&source_target, &intent.old_text);
        let new_edited_matches = exact_matches(&edited_target, &intent.new_text);
        let stale_old_matches =
            if normalize_text(&intent.old_text) == normalize_text(&intent.new_text) {
                0
            } else {
                exact_matches(&edited_target, &intent.old_text)
            };

        if old_source_matches != 1 {
            failures.push(format!(
                "intent {index} page {} source identity matched {old_source_matches} targets, expected exactly one",
                intent.page + 1
            ));
        }
        if new_edited_matches != 1 {
            failures.push(format!(
                "intent {index} page {} replacement identity matched {new_edited_matches} targets, expected exactly one",
                intent.page + 1
            ));
        }
        if stale_old_matches != 0 {
            failures.push(format!(
                "intent {index} page {} still contains the old text in the target region",
                intent.page + 1
            ));
        }
    }

    Ok(VerificationGate::mandatory(
        "content.intended_edit_membership",
        if failures.is_empty() {
            VerificationGateStatus::Passed
        } else {
            VerificationGateStatus::Failed
        },
        if failures.is_empty() {
            format!(
                "all {} intended edits have exact source and replacement membership",
                intents.len()
            )
        } else {
            failures.join("; ")
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_is_whitespace_stable() {
        assert_eq!(normalize_text("  A\n B  "), "A B");
    }
}
