//! Property-based tests for `SegmentMap` page mapping.
//!
//! **Property 2 (CP-2): Round-trip page mapping**
//!
//! These tests exercise the bidirectional translation between global page
//! indices and `(segment index, local page)` pairs exposed by
//! `dual_core_pdf_pipeline::engine::segments::SegmentMap`.
//!
//! **Validates: Requirements 6.3, 6.4**

use std::path::PathBuf;

use dual_core_pdf_pipeline::engine::segments::{SegmentInfo, SegmentMap};
use proptest::prelude::*;

/// The PyMuPDF Pro per-segment page limit. Every segment holds 1..=3 pages,
/// so every resolved local page must lie in `0..=2`.
const MAX_PAGES_PER_SEGMENT: usize = 3;

/// Build a `SegmentMap` from a list of per-segment page counts.
///
/// Segments are laid out contiguously: segment `i` starts at the running
/// page offset and spans `page_counts[i]` pages. Paths are dummy values
/// since the mapping logic under test never touches the filesystem.
fn build_map(page_counts: &[usize]) -> SegmentMap {
    let mut segments = Vec::with_capacity(page_counts.len());
    let mut page_offset = 0usize;
    for (i, &count) in page_counts.iter().enumerate() {
        segments.push(SegmentInfo {
            index: i,
            path: PathBuf::from(format!("segment_{i:03}.pdf")),
            page_offset,
            page_count: count,
            edited: false,
            edited_path: None,
        });
        page_offset += count;
    }
    SegmentMap::new(
        segments,
        PathBuf::from("original.pdf"),
        PathBuf::from("temp_dir"),
        MAX_PAGES_PER_SEGMENT,
    )
}

/// Random segment layout: each segment holds 1..=3 pages (the Pro limit) and
/// the document has between 1 and 40 segments.
fn page_counts_strategy() -> impl Strategy<Value = Vec<usize>> {
    prop::collection::vec(1usize..=MAX_PAGES_PER_SEGMENT, 1..=40)
}

proptest! {
    /// Property 2 (CP-2), direction A: for every in-range global page `p`,
    /// resolving then translating back recovers `p` exactly, and the resolved
    /// local page is always within the Pro 3-page limit (`0..=2`).
    ///
    /// **Validates: Requirements 6.3, 6.4**
    #[test]
    fn prop_round_trip_global_to_local_to_global(page_counts in page_counts_strategy()) {
        let map = build_map(&page_counts);

        // total_pages must equal the sum of the per-segment page counts.
        let expected_total: usize = page_counts.iter().sum();
        prop_assert_eq!(map.total_pages, expected_total);

        for p in 0..map.total_pages {
            let resolved = map.resolve(p);
            prop_assert!(
                resolved.is_some(),
                "resolve({}) returned None for an in-range global page (total_pages = {})",
                p,
                map.total_pages
            );
            let (seg_idx, local_page) = resolved.unwrap();

            // Every resolved local page must be in 0..=2 (Pro 3-page limit).
            prop_assert!(
                local_page <= 2,
                "resolved local_page {} for global page {} is outside 0..=2",
                local_page,
                p
            );

            // to_global(resolve(p)) == Some(p)
            prop_assert_eq!(map.to_global(seg_idx, local_page), Some(p));
        }
    }

    /// Property 2 (CP-2), direction B: for every valid `(i, l)` pair (segment
    /// index in range, local page below that segment's page count), translating
    /// to a global page then resolving recovers the same `(i, l)` pair.
    ///
    /// **Validates: Requirements 6.3, 6.4**
    #[test]
    fn prop_round_trip_local_to_global_to_local(page_counts in page_counts_strategy()) {
        let map = build_map(&page_counts);

        for (i, &count) in page_counts.iter().enumerate() {
            for l in 0..count {
                // Valid local pages always fall within the Pro 3-page limit.
                prop_assert!(
                    l <= 2,
                    "constructed local_page {} (segment {}) is outside 0..=2",
                    l,
                    i
                );

                let global = map.to_global(i, l);
                prop_assert!(
                    global.is_some(),
                    "to_global({}, {}) returned None for a valid (segment, local) pair",
                    i,
                    l
                );
                let global = global.unwrap();

                // resolve(to_global(i, l)) == Some((i, l))
                prop_assert_eq!(map.resolve(global), Some((i, l)));
            }
        }
    }
}
