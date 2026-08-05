//! End-to-end validation of the 3-Page-Mode segmented pipeline on real,
//! multi-page bank statements (>3 pages), which the plain CLI `text` path
//! cannot edit because the PyMuPDF Pro 3-page guard (correctly) blocks a
//! full-document unlock.
//!
//! This exercises the pure-Rust split/merge engine (Subsystem A) the GUI uses
//! when `three_page_mode` is on: split a long statement into <=3-page
//! segments, confirm every segment is within the Pro limit, then merge and
//! assert the page count is preserved losslessly.
//!
//! These are gated on the presence of the AU sample statements; if they are
//! absent (e.g. CI), the test is skipped rather than failed.

use dual_core_pdf_pipeline::engine::pdf_split_merge::{merge_pdfs, split_pdf};
use lopdf::Document;
use std::path::{Path, PathBuf};

fn au_statements() -> Vec<PathBuf> {
    let dir = Path::new("AU Bank Statements");
    if !dir.exists() {
        return Vec::new();
    }
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("pdf"))
                .collect()
        })
        .unwrap_or_default()
}

fn page_count(p: &Path) -> usize {
    Document::load(p).map(|d| d.get_pages().len()).unwrap_or(0)
}

#[test]
fn every_statement_splits_to_within_pro_limit_and_merges_losslessly() {
    let statements = au_statements();
    if statements.is_empty() {
        eprintln!("skipping: no AU Bank Statements present");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut checked = 0;

    for pdf in statements {
        let original_pages = page_count(&pdf);
        if original_pages == 0 {
            eprintln!("skip (unreadable): {}", pdf.display());
            continue;
        }

        let seg_dir = tmp.path().join(format!(
            "seg_{}",
            pdf.file_stem().unwrap().to_string_lossy().replace(' ', "_")
        ));

        // Split into <=3-page segments (the Pro limit).
        let segments = match split_pdf(&pdf, &seg_dir, 3) {
            Ok(s) => s,
            Err(e) => panic!("split failed for {}: {e}", pdf.display()),
        };

        // Property: ceil(N/3) segments, each within the Pro 3-page limit,
        // tiling [0, N) contiguously.
        let expected_segments = original_pages.div_ceil(3);
        assert_eq!(
            segments.len(),
            expected_segments,
            "{}: expected {} segments for {} pages",
            pdf.display(),
            expected_segments,
            original_pages
        );

        let mut covered = 0;
        for (i, seg) in segments.iter().enumerate() {
            assert!(
                seg.page_count >= 1 && seg.page_count <= 3,
                "{}: segment {} has {} pages (must be 1..=3)",
                pdf.display(),
                i,
                seg.page_count
            );
            assert_eq!(
                seg.page_offset,
                covered,
                "{}: segment {} offset gap",
                pdf.display(),
                i
            );
            covered += seg.page_count;
            // Each segment file must itself be a <=3-page PDF (so a Pro unlock
            // on it is legal).
            assert!(
                page_count(&seg.path) <= 3,
                "{}: segment file {} exceeds 3 pages",
                pdf.display(),
                seg.path.display()
            );
        }
        assert_eq!(
            covered,
            original_pages,
            "{}: segments do not tile all pages",
            pdf.display()
        );

        // Merge back and assert lossless page count.
        let merged = seg_dir.join("merged.pdf");
        let ordered: Vec<PathBuf> = segments.iter().map(|s| s.path.clone()).collect();
        let merged_pages = merge_pdfs(&ordered, &merged)
            .unwrap_or_else(|e| panic!("merge failed for {}: {e}", pdf.display()));
        assert_eq!(
            merged_pages,
            original_pages,
            "{}: merged page count {} != original {}",
            pdf.display(),
            merged_pages,
            original_pages
        );
        assert_eq!(
            page_count(&merged),
            original_pages,
            "{}: reloaded merged page count mismatch",
            pdf.display()
        );

        eprintln!(
            "OK {}: {} pages -> {} segments -> merged {} pages",
            pdf.file_name().unwrap().to_string_lossy(),
            original_pages,
            segments.len(),
            merged_pages
        );
        checked += 1;
    }

    assert!(checked > 0, "no statements were actually checked");
}
