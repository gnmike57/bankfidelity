mod fixtures;

use dual_core_pdf_pipeline::engine::segments::{GlobalEdit, SegmentManager};
use lopdf::{Document, Object};
use std::path::Path;
use tempfile::tempdir;

fn edit(page: usize) -> GlobalEdit {
    GlobalEdit {
        page,
        bbox: [70.0, 700.0, 260.0, 730.0],
        old_text: format!("Page {} - synthetic test fixture", page + 1),
        new_text: format!("Page {} - edited boundary", page + 1),
        description: format!("boundary edit on global page {page}"),
        deep_font_replication: false,
    }
}

fn assert_page_order(path: &Path, pages: usize) {
    let document = Document::load(path).expect("load merged document");
    assert_eq!(document.get_pages().len(), pages);
    for page in 1..=pages as u32 {
        let text = document
            .extract_text(&[page])
            .unwrap_or_else(|error| panic!("extract page {page}: {error}"));
        assert!(
            text.contains(&format!("Page {page} - synthetic test fixture")),
            "global page {page} moved or was replaced: {text:?}"
        );
    }
}

#[test]
fn boundary_edits_keep_exact_segment_membership_and_global_order() {
    let root = tempdir().unwrap();
    let source = root.path().join("seven-pages.pdf");
    let output = root.path().join("merged.pdf");
    fixtures::generate_test_pdf(7, &source);

    let manager = SegmentManager::new().unwrap();
    let map = manager.prepare(&source, 3).unwrap();
    let mut observed = Vec::<(String, Vec<usize>)>::new();
    let report = manager
        .apply_and_merge(
            &map,
            vec![edit(2), edit(3), edit(6)],
            &output,
            |input, edited, local_edits| {
                observed.push((
                    input.file_name().unwrap().to_string_lossy().to_string(),
                    local_edits.iter().map(|item| item.local_page).collect(),
                ));
                std::fs::copy(input, edited)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
        )
        .unwrap();

    assert_eq!(
        observed,
        [
            ("segment_000.pdf".into(), vec![2]),
            ("segment_001.pdf".into(), vec![0]),
            ("segment_002.pdf".into(), vec![0]),
        ]
    );
    assert_eq!(report.merged_pages, 7);
    assert_eq!(report.segments_edited, 3);
    assert_page_order(&output, 7);
}

#[test]
fn interrupted_segment_apply_preserves_output_and_retry_succeeds() {
    let root = tempdir().unwrap();
    let source = root.path().join("four-pages.pdf");
    let output = root.path().join("existing-output.pdf");
    fixtures::generate_test_pdf(4, &source);
    std::fs::copy(&source, &output).unwrap();
    let prior = std::fs::read(&output).unwrap();

    let manager = SegmentManager::new().unwrap();
    let map = manager.prepare(&source, 3).unwrap();
    let error = manager
        .apply_and_merge(&map, vec![edit(0), edit(3)], &output, |input, edited, _| {
            if input.file_name().unwrap() == "segment_001.pdf" {
                return Err("injected interruption".into());
            }
            std::fs::copy(input, edited)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .unwrap_err();
    assert!(error.to_string().contains("injected interruption"));
    assert_eq!(std::fs::read(&output).unwrap(), prior);

    let report = manager
        .apply_and_merge(&map, vec![edit(0), edit(3)], &output, |input, edited, _| {
            std::fs::copy(input, edited)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .unwrap();
    assert_eq!(report.merged_pages, 4);
    assert_page_order(&output, 4);
}

#[test]
fn malformed_map_wrong_page_count_and_geometry_drift_never_replace_output() {
    let root = tempdir().unwrap();
    let source = root.path().join("four-pages.pdf");
    let output = root.path().join("existing-output.pdf");
    fixtures::generate_test_pdf(4, &source);
    std::fs::copy(&source, &output).unwrap();
    let prior = std::fs::read(&output).unwrap();

    let manager = SegmentManager::new().unwrap();
    let map = manager.prepare(&source, 3).unwrap();

    let mut malformed = map.clone();
    malformed.segments[1].page_offset += 1;
    let mut called = false;
    let error = manager
        .apply_and_merge(&malformed, vec![edit(0)], &output, |_, _, _| {
            called = true;
            Ok(())
        })
        .unwrap_err();
    assert!(error.to_string().contains("starts at page"));
    assert!(!called);
    assert_eq!(std::fs::read(&output).unwrap(), prior);

    let error = manager
        .apply_and_merge(&map, vec![edit(0)], &output, |_, edited, _| {
            fixtures::generate_test_pdf(1, edited);
            Ok(())
        })
        .unwrap_err();
    assert!(error.to_string().contains("edited segment has"));
    assert_eq!(std::fs::read(&output).unwrap(), prior);

    let error = manager
        .apply_and_merge(&map, vec![edit(0)], &output, |input, edited, _| {
            let mut document = Document::load(input).map_err(|error| error.to_string())?;
            let first_page = *document
                .get_pages()
                .get(&1)
                .ok_or_else(|| "missing first page".to_string())?;
            document
                .get_dictionary_mut(first_page)
                .map_err(|error| error.to_string())?
                .set("Rotate", Object::Integer(90));
            document.save(edited).map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap_err();
    assert!(error.to_string().contains("page boxes or rotations"));
    assert_eq!(std::fs::read(&output).unwrap(), prior);
}
