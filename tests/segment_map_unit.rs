//! Unit tests for the pure page-mapping layer in `src/engine/segments.rs`.
//!
//! These cover hand-built `SegmentMap`s for documents of 1, 2, 3, 4, 7, and 10
//! pages (segments of at most 3 pages), exercising:
//!   * `resolve` / `to_global` segment-edge boundaries, first/last page, and
//!     out-of-range inputs (which must return `None` without mutating state);
//!   * `group_edits_by_segment` bucketing, empty input, and the out-of-range
//!     error path (which must identify the offending global page rather than
//!     silently drop the edit);
//!   * `ordered_merge_paths` substituting `edited_path` while preserving
//!     ascending segment-index order.
//!
//! _Requirements: 6.1, 6.2, 6.5, 6.6, 6.7, 8.3, 8.4, 8.5_

use dual_core_pdf_pipeline::engine::segments::{
    GlobalEdit, GroupEditsError, SegmentInfo, SegmentMap,
};
use std::path::PathBuf;

/// Build a `SegmentMap` from an explicit list of per-segment page counts,
/// chaining `page_offset` contiguously from 0. Each segment gets a distinct,
/// deterministic original path so merge-path ordering is observable.
fn build_map(page_counts: &[usize]) -> SegmentMap {
    let mut segments = Vec::new();
    let mut offset = 0usize;
    for (i, &count) in page_counts.iter().enumerate() {
        segments.push(SegmentInfo {
            index: i,
            path: PathBuf::from(format!("segment_{i:03}.pdf")),
            page_offset: offset,
            page_count: count,
            edited: false,
            edited_path: None,
        });
        offset += count;
    }
    SegmentMap::new(
        segments,
        PathBuf::from("source.pdf"),
        PathBuf::from("temp_dir"),
        3,
    )
}

/// Chunk `total` pages into ≤3-page segments (3,3,...,remainder), matching the
/// split engine's contiguous tiling, and build a map for it.
fn map_for_pages(total: usize) -> SegmentMap {
    let mut counts = Vec::new();
    let mut remaining = total;
    while remaining > 0 {
        let c = remaining.min(3);
        counts.push(c);
        remaining -= c;
    }
    build_map(&counts)
}

/// Construct a `GlobalEdit` targeting `page` with placeholder edit content.
fn edit_on(page: usize) -> GlobalEdit {
    GlobalEdit {
        page,
        bbox: [0.0, 0.0, 10.0, 10.0],
        old_text: format!("old-{page}"),
        new_text: format!("new-{page}"),
        description: format!("edit page {page}"),
        deep_font_replication: false,
    }
}

// ---------------------------------------------------------------------------
// resolve / to_global: per-document-size mappings (Req 6.1, 6.2, 6.7)
// ---------------------------------------------------------------------------

#[test]
fn one_page_doc_maps_single_segment() {
    let map = map_for_pages(1);
    assert_eq!(map.total_pages, 1);
    assert_eq!(map.segments.len(), 1);
    // First == last == only page.
    assert_eq!(map.resolve(0), Some((0, 0)));
    assert_eq!(map.to_global(0, 0), Some(0));
}

#[test]
fn two_page_doc_maps_single_segment() {
    let map = map_for_pages(2);
    assert_eq!(map.total_pages, 2);
    assert_eq!(map.segments.len(), 1);
    assert_eq!(map.resolve(0), Some((0, 0))); // first
    assert_eq!(map.resolve(1), Some((0, 1))); // last (total-1)
    assert_eq!(map.to_global(0, 1), Some(1));
}

#[test]
fn three_page_doc_fills_one_segment_exactly() {
    let map = map_for_pages(3);
    assert_eq!(map.total_pages, 3);
    assert_eq!(map.segments.len(), 1);
    assert_eq!(map.resolve(0), Some((0, 0))); // first
    assert_eq!(map.resolve(1), Some((0, 1)));
    assert_eq!(map.resolve(2), Some((0, 2))); // last, local_page == 2 (Req 6.7)
    assert_eq!(map.to_global(0, 2), Some(2));
}

#[test]
fn four_page_doc_spans_two_segments_with_edge_boundary() {
    // Segments: [3, 1] -> offsets 0, 3.
    let map = map_for_pages(4);
    assert_eq!(map.total_pages, 4);
    assert_eq!(map.segments.len(), 2);
    assert_eq!(map.resolve(0), Some((0, 0))); // first page
    assert_eq!(map.resolve(2), Some((0, 2))); // last page of seg 0
    assert_eq!(map.resolve(3), Some((1, 0))); // first page of seg 1 == last page (total-1)
                                              // Round-trip the boundary both directions.
    assert_eq!(map.to_global(0, 2), Some(2));
    assert_eq!(map.to_global(1, 0), Some(3));
}

#[test]
fn seven_page_doc_segment_edge_boundaries() {
    // Segments: [3, 3, 1] -> offsets 0, 3, 6.
    let map = map_for_pages(7);
    assert_eq!(map.total_pages, 7);
    assert_eq!(map.segments.len(), 3);

    // First and last page.
    assert_eq!(map.resolve(0), Some((0, 0)));
    assert_eq!(map.resolve(6), Some((2, 0))); // total-1

    // Segment-edge boundaries called out in the task: 2->(0,2), 3->(1,0),
    // 5->(1,2), 6->(2,0).
    assert_eq!(map.resolve(2), Some((0, 2)));
    assert_eq!(map.resolve(3), Some((1, 0)));
    assert_eq!(map.resolve(5), Some((1, 2)));
    assert_eq!(map.resolve(6), Some((2, 0)));

    // Every local_page stays within 0..=2 (Req 6.7).
    for g in 0..map.total_pages {
        let (_, local) = map.resolve(g).expect("in-range page must resolve");
        assert!(local <= 2, "local_page {local} out of 0..=2 for global {g}");
    }
}

#[test]
fn ten_page_doc_full_roundtrip_and_boundaries() {
    // Segments: [3, 3, 3, 1] -> offsets 0, 3, 6, 9.
    let map = map_for_pages(10);
    assert_eq!(map.total_pages, 10);
    assert_eq!(map.segments.len(), 4);

    // First and last.
    assert_eq!(map.resolve(0), Some((0, 0)));
    assert_eq!(map.resolve(9), Some((3, 0))); // total-1

    // Boundaries.
    assert_eq!(map.resolve(2), Some((0, 2)));
    assert_eq!(map.resolve(3), Some((1, 0)));
    assert_eq!(map.resolve(5), Some((1, 2)));
    assert_eq!(map.resolve(6), Some((2, 0)));
    assert_eq!(map.resolve(8), Some((2, 2)));

    // Full round-trip resolve -> to_global -> identity (Req 6.3-style check
    // expressed over every concrete page).
    for g in 0..map.total_pages {
        let (seg, local) = map.resolve(g).expect("in-range page must resolve");
        assert_eq!(map.to_global(seg, local), Some(g));
        assert!(local <= 2);
    }
}

// ---------------------------------------------------------------------------
// Out-of-range resolve / to_global return None without mutation (Req 6.5, 6.6)
// ---------------------------------------------------------------------------

#[test]
fn resolve_out_of_range_returns_none_without_mutation() {
    let map = map_for_pages(7); // total_pages == 7
    let total = map.total_pages;

    // resolve(total_pages) and resolve(total_pages + k) -> None.
    assert_eq!(map.resolve(total), None);
    for k in 1..=5 {
        assert_eq!(map.resolve(total + k), None);
    }

    // No mutation: a known mapping still holds afterward, and total is intact.
    assert_eq!(map.total_pages, total);
    assert_eq!(map.resolve(0), Some((0, 0)));
    assert_eq!(map.resolve(6), Some((2, 0)));
}

#[test]
fn to_global_out_of_range_returns_none_without_mutation() {
    let map = map_for_pages(7); // 3 segments: [3,3,1]
    let seg_count = map.segments.len();

    // Out-of-range segment index.
    assert_eq!(map.to_global(seg_count, 0), None);
    assert_eq!(map.to_global(seg_count + 3, 0), None);

    // Valid segment, out-of-range local page.
    // Segment 2 has page_count 1, so local_page 1 is out of range.
    assert_eq!(map.to_global(2, 1), None);
    // Segment 0 has page_count 3, so local_page 3 is out of range.
    assert_eq!(map.to_global(0, 3), None);

    // No mutation: known mappings still hold afterward.
    assert_eq!(map.segments.len(), seg_count);
    assert_eq!(map.to_global(0, 0), Some(0));
    assert_eq!(map.to_global(2, 0), Some(6));
    assert_eq!(map.resolve(3), Some((1, 0)));
}

// ---------------------------------------------------------------------------
// group_edits_by_segment (Req 8.3, 8.4, 8.5)
// ---------------------------------------------------------------------------

#[test]
fn group_edits_lands_in_correct_buckets_with_local_indices() {
    // 10-page doc: segments [3,3,3,1] -> offsets 0,3,6,9.
    let map = map_for_pages(10);

    // Edits on globals 0, 4, 5, 9:
    //   0 -> seg 0, local 0
    //   4 -> seg 1, local 1
    //   5 -> seg 1, local 2
    //   9 -> seg 3, local 0
    let edits = vec![edit_on(0), edit_on(4), edit_on(5), edit_on(9)];
    let groups = map
        .group_edits_by_segment(&edits)
        .expect("all edits are in range");

    // Buckets keyed by segment index; segment 2 has no edits.
    assert_eq!(groups.keys().copied().collect::<Vec<_>>(), vec![0, 1, 3]);

    // Segment 0: one edit at local 0.
    let seg0 = &groups[&0];
    assert_eq!(seg0.len(), 1);
    assert_eq!(seg0[0].local_page, 0);
    assert_eq!(seg0[0].new_text, "new-0");

    // Segment 1: two edits at locals 1 and 2, preserving submission order.
    let seg1 = &groups[&1];
    assert_eq!(seg1.len(), 2);
    assert_eq!(seg1[0].local_page, 1);
    assert_eq!(seg1[0].new_text, "new-4");
    assert_eq!(seg1[1].local_page, 2);
    assert_eq!(seg1[1].new_text, "new-5");

    // Segment 3: one edit at local 0.
    let seg3 = &groups[&3];
    assert_eq!(seg3.len(), 1);
    assert_eq!(seg3[0].local_page, 0);
    assert_eq!(seg3[0].new_text, "new-9");

    // Each grouped local page round-trips back to the submitted global page.
    assert_eq!(map.to_global(0, seg0[0].local_page), Some(0));
    assert_eq!(map.to_global(1, seg1[0].local_page), Some(4));
    assert_eq!(map.to_global(1, seg1[1].local_page), Some(5));
    assert_eq!(map.to_global(3, seg3[0].local_page), Some(9));
}

#[test]
fn group_edits_empty_input_yields_empty_buckets() {
    let map = map_for_pages(7);
    let groups = map
        .group_edits_by_segment(&[])
        .expect("empty edit list is trivially in range");
    assert!(groups.is_empty(), "no edits should produce zero buckets");
}

#[test]
fn group_edits_out_of_range_errors_and_identifies_offending_page() {
    let map = map_for_pages(7); // total_pages == 7
    let total = map.total_pages;

    // Edit exactly at total_pages is out of range.
    let edits = vec![edit_on(0), edit_on(total)];
    let err = map
        .group_edits_by_segment(&edits)
        .expect_err("an out-of-range global page must error, not be dropped");
    assert_eq!(
        err,
        GroupEditsError::OutOfRange {
            global_page: total,
            total_pages: total,
        }
    );
    // The error message identifies the offending page.
    let msg = err.to_string();
    assert!(
        msg.contains(&total.to_string()),
        "error message should identify offending page: {msg}"
    );

    // Well beyond the end also errors and identifies that exact page.
    let far = total + 5;
    let err2 = map
        .group_edits_by_segment(&[edit_on(far)])
        .expect_err("page far beyond end must error");
    assert_eq!(
        err2,
        GroupEditsError::OutOfRange {
            global_page: far,
            total_pages: total,
        }
    );
}

// ---------------------------------------------------------------------------
// ordered_merge_paths (Req 8.3, 8.4)
// ---------------------------------------------------------------------------

#[test]
fn ordered_merge_paths_uses_original_paths_when_unedited() {
    let map = map_for_pages(10); // 4 segments, none edited
    let paths = map.ordered_merge_paths();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("segment_000.pdf"),
            PathBuf::from("segment_001.pdf"),
            PathBuf::from("segment_002.pdf"),
            PathBuf::from("segment_003.pdf"),
        ]
    );
}

#[test]
fn ordered_merge_paths_substitutes_edited_path_in_segment_order() {
    let mut map = map_for_pages(10); // 4 segments [3,3,3,1]

    // Mark segment 1 as edited with a distinct edited path.
    map.segments[1].edited = true;
    map.segments[1].edited_path = Some(PathBuf::from("segment_001_edited.pdf"));

    let paths = map.ordered_merge_paths();

    // Edited segment uses its edited_path; all others use original paths,
    // in ascending segment-index order.
    assert_eq!(
        paths,
        vec![
            PathBuf::from("segment_000.pdf"),
            PathBuf::from("segment_001_edited.pdf"),
            PathBuf::from("segment_002.pdf"),
            PathBuf::from("segment_003.pdf"),
        ]
    );
}

#[test]
fn ordered_merge_paths_multiple_edited_segments_preserve_order() {
    let mut map = map_for_pages(7); // 3 segments [3,3,1]

    // Edit segments 0 and 2; leave segment 1 untouched.
    map.segments[0].edited = true;
    map.segments[0].edited_path = Some(PathBuf::from("segment_000_edited.pdf"));
    map.segments[2].edited = true;
    map.segments[2].edited_path = Some(PathBuf::from("segment_002_edited.pdf"));

    let paths = map.ordered_merge_paths();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("segment_000_edited.pdf"),
            PathBuf::from("segment_001.pdf"),
            PathBuf::from("segment_002_edited.pdf"),
        ]
    );
}
