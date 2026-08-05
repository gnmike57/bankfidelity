mod fixtures;

use dual_core_pdf_pipeline::engine::pdf_split_merge::{merge_pdfs, split_pdf};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::PathBuf;
use tempfile::tempdir;

/// Returns the path to a real or synthetic test PDF.
/// Prefers examples/sample.pdf for high-fidelity testing; falls back to a
/// generated synthetic 5-page PDF so this test always exercises real code
/// instead of silently self-skipping.
fn get_test_pdf() -> (PathBuf, bool) {
    let sample = PathBuf::from("examples/sample.pdf");
    if sample.exists() {
        return (sample, false);
    }

    eprintln!("[info] examples/sample.pdf not found; using synthetic 5-page PDF");
    let dir = std::env::temp_dir().join("split_merge_test_fixtures");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("synthetic_5page.pdf");
    fixtures::generate_test_pdf(5, &path);
    (path, true)
}

#[test]
fn test_split_merge_cycle() {
    let (src_path, is_synthetic) = get_test_pdf();

    let original_doc = Document::load(&src_path).expect("Failed to load original");
    let original_pages = original_doc.get_pages().len();
    assert!(original_pages > 0);

    let out_dir = tempdir().expect("Failed to create temp dir");

    // Split into 1-page segments
    let segments = split_pdf(&src_path, out_dir.path(), 1).expect("Split failed");
    assert_eq!(segments.len(), original_pages);

    let output_path = out_dir.path().join("merged.pdf");
    let segment_paths: Vec<PathBuf> = segments.into_iter().map(|s| s.path).collect();

    // Merge back
    let merged_pages = merge_pdfs(&segment_paths, &output_path).expect("Merge failed");
    assert_eq!(merged_pages, original_pages);

    // Verify structural integrity of merged PDF
    let merged_doc = Document::load(&output_path).expect("Failed to load merged PDF");
    assert_eq!(merged_doc.get_pages().len(), original_pages);

    // Only check Font references for real PDFs — synthetic PDFs use simple
    // inline font refs that lopdf's page dict may not expose identically.
    if !is_synthetic {
        let page_id = merged_doc
            .get_pages()
            .get(&1)
            .cloned()
            .expect("Page 1 missing");
        let page_dict = merged_doc
            .get_object(page_id)
            .and_then(|obj| obj.as_dict())
            .expect("Page 1 not a dict");

        if let Ok(resources) = page_dict.get(b"Resources").and_then(|obj| obj.as_dict()) {
            if let Ok(fonts) = resources.get(b"Font").and_then(|obj| obj.as_dict()) {
                assert!(
                    !fonts.is_empty(),
                    "Merged PDF lost font references on Page 1"
                );
            }
        }
    }

    eprintln!(
        "✅ split_merge_cycle: {} pages → {} segments → merged {} pages ({})",
        original_pages,
        original_pages,
        merged_pages,
        if is_synthetic {
            "synthetic"
        } else {
            "real PDF"
        }
    );
}

#[test]
fn test_split_merge_fidelity_multi_page() {
    let (src_path, is_synthetic) = get_test_pdf();

    let out_dir = tempdir().expect("Failed to create temp dir");

    // Split into 2-page segments (if original has enough pages)
    let segments = split_pdf(&src_path, out_dir.path(), 2).expect("Split failed");
    let num_segments = segments.len();

    let output_path = out_dir.path().join("merged_2.pdf");
    let segment_paths: Vec<PathBuf> = segments.into_iter().map(|s| s.path).collect();

    let merged_pages = merge_pdfs(&segment_paths, &output_path).expect("Merge failed");

    let original_doc = Document::load(&src_path).unwrap();
    assert_eq!(merged_pages, original_doc.get_pages().len());

    eprintln!(
        "✅ split_merge_fidelity_multi_page: {} pages → {} segments → merged {} pages ({})",
        original_doc.get_pages().len(),
        num_segments,
        merged_pages,
        if is_synthetic {
            "synthetic"
        } else {
            "real PDF"
        }
    );
}

#[test]
fn split_merge_preserves_document_metadata_and_per_page_boxes() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("metadata-source.pdf");
    fixtures::generate_test_pdf(5, &source);

    let mut document = Document::load(&source).unwrap();
    let info_id = document.add_object(dictionary! {
        "Title" => Object::string_literal("Segmented Statement"),
        "Author" => Object::string_literal("Fidelity Test"),
    });
    document.trailer.set("Info", info_id);
    let xmp = b"<x:xmpmeta>segment-metadata</x:xmpmeta>".to_vec();
    let metadata_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "Metadata",
            "Subtype" => "XML",
        },
        xmp.clone(),
    ));
    let catalog = document.catalog_mut().unwrap();
    catalog.set("Metadata", metadata_id);
    catalog.set("Lang", Object::string_literal("en-AU"));
    catalog.set("PageMode", Object::Name(b"UseNone".to_vec()));
    catalog.set("PageLayout", Object::Name(b"SinglePage".to_vec()));

    for (index, page_id) in document.get_pages().values().copied().enumerate() {
        let inset = index as i64 + 1;
        let page = document.get_dictionary_mut(page_id).unwrap();
        page.set(
            "CropBox",
            vec![
                Object::Integer(inset),
                Object::Integer(inset + 1),
                Object::Integer(595 - inset),
                Object::Integer(842 - inset),
            ],
        );
        page.set("Rotate", Object::Integer((index as i64 * 90) % 360));
    }
    document.save(&source).unwrap();

    let segment_dir = directory.path().join("segments");
    let segments = split_pdf(&source, &segment_dir, 2).unwrap();
    let paths: Vec<PathBuf> = segments
        .iter()
        .map(|segment| segment.path.clone())
        .collect();
    let output = directory.path().join("metadata-merged.pdf");
    assert_eq!(merge_pdfs(&paths, &output).unwrap(), 5);

    let merged = Document::load(&output).unwrap();
    let (_, info_object) = merged
        .dereference(merged.trailer.get(b"Info").unwrap())
        .unwrap();
    let info = info_object.as_dict().unwrap();
    assert_eq!(
        info.get(b"Title").unwrap().as_str().unwrap(),
        b"Segmented Statement"
    );
    assert_eq!(
        info.get(b"Author").unwrap().as_str().unwrap(),
        b"Fidelity Test"
    );

    let catalog = merged.catalog().unwrap();
    assert_eq!(catalog.get(b"Lang").unwrap().as_str().unwrap(), b"en-AU");
    assert_eq!(
        catalog.get(b"PageMode").unwrap().as_name().unwrap(),
        b"UseNone"
    );
    assert_eq!(
        catalog.get(b"PageLayout").unwrap().as_name().unwrap(),
        b"SinglePage"
    );
    let (_, metadata_object) = merged
        .dereference(catalog.get(b"Metadata").unwrap())
        .unwrap();
    assert_eq!(metadata_object.as_stream().unwrap().content, xmp);

    assert_eq!(merged.get_pages().len(), 5);
    for (index, page_id) in merged.get_pages().values().copied().enumerate() {
        let page = merged.get_dictionary(page_id).unwrap();
        let crop = page.get(b"CropBox").unwrap().as_array().unwrap();
        let inset = index as f32 + 1.0;
        let observed: Vec<f32> = crop.iter().map(|value| value.as_float().unwrap()).collect();
        assert_eq!(observed, [inset, inset + 1.0, 595.0 - inset, 842.0 - inset]);
        assert_eq!(
            page.get(b"Rotate").unwrap().as_i64().unwrap(),
            (index as i64 * 90) % 360
        );
        let text = merged.extract_text(&[(index + 1) as u32]).unwrap();
        assert!(text.contains(&format!("Page {} - synthetic test fixture", index + 1)));
    }
}

#[test]
fn merge_failures_preserve_existing_destination_bytes() {
    let directory = tempdir().unwrap();
    let destination = directory.path().join("existing.pdf");
    fixtures::generate_test_pdf(2, &destination);
    let prior = std::fs::read(&destination).unwrap();

    let empty_error = merge_pdfs(&[], &destination).unwrap_err();
    assert!(empty_error.to_string().contains("at least one input"));
    assert_eq!(std::fs::read(&destination).unwrap(), prior);

    let missing = directory.path().join("missing-segment.pdf");
    let missing_error = merge_pdfs(&[missing], &destination).unwrap_err();
    assert!(missing_error.to_string().contains("failed to load"));
    assert_eq!(std::fs::read(&destination).unwrap(), prior);

    let corrupt = directory.path().join("corrupt-segment.pdf");
    std::fs::write(&corrupt, b"not a PDF").unwrap();
    let corrupt_error = merge_pdfs(&[corrupt], &destination).unwrap_err();
    assert!(corrupt_error.to_string().contains("failed to load"));
    assert_eq!(std::fs::read(&destination).unwrap(), prior);
}
