#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use dual_core_pdf_pipeline::engine::workflow::*;
use rust_decimal_macros::dec;

#[test]
fn test_is_borderline() {
    // is_borderline = score >= threshold && score <= threshold * 2.5
    assert!(!is_borderline(0.015, 0.02)); // 0.015 < 0.02 (not >= threshold)
    assert!(is_borderline(0.025, 0.02)); // 0.025 >= 0.02 && <= 0.05
    assert!(is_borderline(0.05, 0.02)); // 0.05 == 2.5 * 0.02
    assert!(!is_borderline(0.06, 0.02)); // 0.06 > 0.05
}

#[test]
fn test_mask_padding_for_attempt() {
    // Verification masks are immutable: widening after a failure can hide
    // collateral mutations and makes replay evidence non-deterministic.
    for attempt in [1, 2, 3, 5, 10] {
        assert_eq!(mask_padding_for_attempt(attempt), 0.0);
    }
}

#[test]
fn test_should_accept_near_perfect() {
    // A failed mandatory gate never becomes acceptable merely because a
    // later attempt is numerically close to the threshold.
    for (attempt, score) in [(1, 0.005), (3, 0.005), (3, 0.015), (99, 0.0)] {
        assert!(!should_accept_near_perfect(attempt, score, 0.02));
    }
}

#[test]
fn test_edit_set_hash() {
    let mut edits = vec![UserEdit {
        page: 0,
        line_on_page: 0,
        bbox: [0.0, 0.0, 10.0, 10.0],
        old_text: "2023-01-01".to_string(),
        new_text: "2023-01-02".to_string(),
        field: EditField::Date,
    }];
    let hash1 = edit_set_hash("abc", &edits);

    // Exact same edit should yield same hash
    let hash2 = edit_set_hash("abc", &edits);
    assert_eq!(hash1, hash2);

    // Different edit yields different hash
    edits[0].page = 1;
    let hash3 = edit_set_hash("abc", &edits);
    assert_ne!(hash1, hash3);
}

#[test]
fn test_detect_edit_conflicts() {
    let edits = vec![
        UserEdit {
            page: 0,
            line_on_page: 0,
            bbox: [0.0, 0.0, 10.0, 10.0],
            old_text: "100.0".to_string(),
            new_text: "150.0".to_string(),
            field: EditField::Debit,
        },
        UserEdit {
            page: 0,
            line_on_page: 0,
            bbox: [2.0, 2.0, 8.0, 8.0], // >50% overlap with edit 0
            old_text: "100.0".to_string(),
            new_text: "200.0".to_string(),
            field: EditField::Credit,
        },
        UserEdit {
            page: 1, // different page
            line_on_page: 0,
            bbox: [0.0, 0.0, 10.0, 10.0],
            old_text: "200.0".to_string(),
            new_text: "300.0".to_string(),
            field: EditField::Debit,
        },
    ];

    let conflicts = detect_edit_conflicts(&edits);
    assert_eq!(conflicts.len(), 1);
    // Edit 0 and Edit 1 conflict
    assert_eq!(conflicts[0], (0, 1));
}

#[test]
fn test_prune_redundant_edits() {
    let preview = BalancePreview {
        rows: vec![PreviewRow {
            page: 0,
            line_on_page: 0,
            date: "2023-01-01".to_string(),
            description: "Test".to_string(),
            debit: Some(dec!(100.0)),
            credit: None,
            old_running_balance: Some(dec!(500.0)),
            new_running_balance: Some(dec!(600.0)),
            will_change: true,
        }],
        final_imbalance: dec!(0.0),
        balanced: true,
        auto_correction_message: None,
    };

    let edits = vec![
        UserEdit {
            page: 0,
            line_on_page: 0,
            bbox: [0.0, 0.0, 10.0, 10.0],
            old_text: "500.00".to_string(),
            new_text: "600.00".to_string(), // redundant, matches new_running_balance
            field: EditField::RunningBalance,
        },
        UserEdit {
            page: 0,
            line_on_page: 0,
            bbox: [0.0, 0.0, 10.0, 10.0],
            old_text: "500.00".to_string(),
            new_text: "650.00".to_string(), // not redundant
            field: EditField::RunningBalance,
        },
        UserEdit {
            page: 0,
            line_on_page: 0,
            bbox: [0.0, 0.0, 10.0, 10.0],
            old_text: "100.00".to_string(),
            new_text: "200.00".to_string(), // not RunningBalance field, always kept
            field: EditField::Debit,
        },
    ];

    let (kept, dropped) = prune_redundant_edits(&edits, &preview);
    assert_eq!(kept.len(), 2);
    assert_eq!(dropped.len(), 1);
    assert_eq!(kept[0].new_text, "650.00");
    assert_eq!(kept[1].new_text, "200.00");
    assert_eq!(dropped[0].new_text, "600.00");
}
