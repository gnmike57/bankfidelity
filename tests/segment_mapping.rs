use dual_core_pdf_pipeline::engine::segments::{SegmentInfo, SegmentMap};
use proptest::prelude::*;
use std::path::PathBuf;

fn make_map(page_counts: &[usize]) -> SegmentMap {
    let mut segments = Vec::new();
    let mut offset = 0;
    for (i, &count) in page_counts.iter().enumerate() {
        segments.push(SegmentInfo {
            index: i,
            path: PathBuf::from("dummy.pdf"),
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
        PathBuf::from("temp"),
        3,
    )
}

proptest! {
    #[test]
    fn test_mapping_roundtrip(page_counts in prop::collection::vec(1..10usize, 1..20)) {
        let map = make_map(&page_counts);
        let total_pages = map.total_pages;

        for global_page in 0..total_pages {
            let (seg_idx, local_page) = map.resolve(global_page).expect("Must resolve");
            let roundtrip = map.to_global(seg_idx, local_page).expect("Must roundtrip");
            prop_assert_eq!(global_page, roundtrip);
        }
    }

    #[test]
    fn test_resolve_out_of_bounds(
        page_counts in prop::collection::vec(1..10usize, 1..10),
        global_page in 100..200usize
    ) {
        let map = make_map(&page_counts);
        if global_page >= map.total_pages {
            prop_assert!(map.resolve(global_page).is_none());
        }
    }
}
