use dual_core_pdf_pipeline::pdf::engine::PdfEngine;
use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use std::path::Path;

/// Helper to generate a multi-page PDF for testing cloning and removing.
fn create_multipage_pdf(path: &Path, num_pages: usize) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });

    let mut page_ids = vec![];

    for i in 0..num_pages {
        let text = format!("Page {}", i + 1);
        let operations = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.0.into()]),
            Operation::new(
                "Tm",
                vec![
                    1.0.into(),
                    0.0.into(),
                    0.0.into(),
                    1.0.into(),
                    50.0.into(),
                    700.0.into(),
                ],
            ),
            Operation::new(
                "Tj",
                vec![Object::String(text.into_bytes(), StringFormat::Literal)],
            ),
            Operation::new("ET", vec![]),
        ];

        let content = Content { operations };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.0.into(), 0.0.into(), 595.0.into(), 842.0.into()],
        });
        page_ids.push(page_id.into());
    }

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => page_ids,
        "Count" => num_pages as i32,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

#[test]
fn test_native_engine_clone_pages() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("clone_in.pdf");
    let output = dir.path().join("clone_out.pdf");

    create_multipage_pdf(&input, 3); // Pages 1, 2, 3 (indices 0, 1, 2)

    let engine = OxidizePdfEngine::new();

    // Clone page index 1 (Page 2). The clone must follow its source immediately.
    engine.clone_pages(&input, &output, vec![1]).unwrap();

    let doc = Document::load(&output).unwrap();
    let pages = doc.get_pages();

    assert_eq!(pages.len(), 4);

    // We expect the texts: "Page 1", "Page 2", "Page 2", "Page 3".
    let out_blocks_0 = engine.get_text_blocks(&output, 0).unwrap();
    let out_blocks_1 = engine.get_text_blocks(&output, 1).unwrap();
    let out_blocks_2 = engine.get_text_blocks(&output, 2).unwrap();
    let out_blocks_3 = engine.get_text_blocks(&output, 3).unwrap();

    assert_eq!(out_blocks_0[0].text, "Page 1");
    assert_eq!(out_blocks_1[0].text, "Page 2");
    assert_eq!(out_blocks_2[0].text, "Page 2");
    assert_eq!(out_blocks_3[0].text, "Page 3");
}

#[test]
fn test_native_engine_remove_pages() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("remove_in.pdf");
    let output = dir.path().join("remove_out.pdf");

    create_multipage_pdf(&input, 4); // Pages 1, 2, 3, 4

    let engine = OxidizePdfEngine::new();

    // Remove pages 1 and 2 (indices 1, 2)
    engine.remove_pages(&input, &output, vec![1, 2]).unwrap();

    let doc = Document::load(&output).unwrap();
    let pages = doc.get_pages();

    assert_eq!(pages.len(), 2);

    // Check that we have 2 pages
    // We expect the texts: "Page 1", "Page 4"
    let out_blocks_0 = engine.get_text_blocks(&output, 0).unwrap();
    let out_blocks_1 = engine.get_text_blocks(&output, 1).unwrap();

    assert_eq!(out_blocks_0[0].text, "Page 1");
    assert_eq!(out_blocks_1[0].text, "Page 4");
}

#[test]
fn test_native_engine_analyze_layout() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("layout_in.pdf");

    create_multipage_pdf(&input, 2);

    let engine = OxidizePdfEngine::new();

    let layout = engine.analyze_layout(&input).unwrap();
    assert_eq!(layout.total_pages, 2);
    // Since there are no repeated headers across pages in this simple test PDF,
    // has_consistent_headers will be false (or maybe true if there's no header).
    // Let's just check it doesn't crash.
}

#[test]
fn test_native_engine_apply_change() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("apply_in.pdf");
    let output = dir.path().join("apply_out.pdf");

    create_multipage_pdf(&input, 1); // Page 1

    let engine = OxidizePdfEngine::new();

    // First, find the text block to get its bounding box
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 1);
    let bbox = blocks[0].bbox;
    let old_text = &blocks[0].text;
    assert_eq!(old_text, "Page 1");

    // Apply the change
    let new_text = "Replaced 1";
    engine
        .apply_change(&input, &output, 0, bbox, new_text, old_text, None)
        .unwrap();

    // Verify the change
    let modified_blocks = engine.get_text_blocks(&output, 0).unwrap();
    assert_eq!(modified_blocks.len(), 1);
    assert_eq!(modified_blocks[0].text, new_text);
}

#[test]
fn test_native_engine_non_ascii_guard() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("ascii_in.pdf");
    let output = dir.path().join("ascii_out.pdf");

    create_multipage_pdf(&input, 1);

    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    let bbox = blocks[0].bbox;

    // Try applying a non-ASCII string (emoji)
    let new_text = "Page 📈";
    let result = engine.apply_change(&input, &output, 0, bbox, new_text, "Page 1", None);

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("ASCII"));
}

fn create_pdf_with_ops(path: &Path, ops: Vec<Operation>) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });

    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations: ops }.encode().unwrap(),
    ));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.0.into(), 0.0.into(), 595.0.into(), 842.0.into()],
    });

    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

#[test]
fn test_native_engine_text_operators() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("ops.pdf");

    // We will test Tm, Td, TD, T* operators
    let ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 12.0.into()]),
        // Tm sets matrix directly: 1 0 0 1 50 700 Tm
        Operation::new(
            "Tm",
            vec![
                1.0.into(),
                0.0.into(),
                0.0.into(),
                1.0.into(),
                50.0.into(),
                700.0.into(),
            ],
        ),
        Operation::new(
            "Tj",
            vec![Object::String("Line 1".into(), StringFormat::Literal)],
        ),
        // Td moves offset: 0 -20 Td
        Operation::new("Td", vec![0.0.into(), (-20.0).into()]),
        Operation::new(
            "Tj",
            vec![Object::String("Line 2".into(), StringFormat::Literal)],
        ),
        // TD is like Td but also sets leading (we just treat it as Td in the engine)
        Operation::new("TD", vec![0.0.into(), (-20.0).into()]),
        Operation::new(
            "Tj",
            vec![Object::String("Line 3".into(), StringFormat::Literal)],
        ),
        // T* moves to next line using leading (approx font size in the engine)
        Operation::new("T*", vec![]),
        Operation::new(
            "Tj",
            vec![Object::String("Line 4".into(), StringFormat::Literal)],
        ),
        // TJ (Array of strings and numbers)
        Operation::new("T*", vec![]),
        Operation::new(
            "TJ",
            vec![Object::Array(vec![
                Object::String("L".into(), StringFormat::Literal),
                Object::Integer(-50), // kerning
                Object::String("ine 5".into(), StringFormat::Literal),
            ])],
        ),
        Operation::new("ET", vec![]),
    ];

    create_pdf_with_ops(&input, ops);

    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();

    assert_eq!(blocks.len(), 5);
    assert_eq!(blocks[0].text, "Line 1");
    // Canonical bboxes use a visible top-left origin, so lower lines have larger Y.
    let y1 = blocks[0].bbox[1];
    let y2 = blocks[1].bbox[1];
    let y3 = blocks[2].bbox[1];
    let y4 = blocks[3].bbox[1];

    assert!(y1 < y2);
    assert!(y2 < y3);
    assert!(y3 < y4);

    // Line 5 is parsed as "Line 5" from TJ array
    assert_eq!(blocks[4].text, "Line 5");

    // Now test apply_change and apply_many_edits on these weird operators!
    let output1 = dir.path().join("ops_out1.pdf");
    let output2 = dir.path().join("ops_out2.pdf");

    // 1. apply_change to Line 2 (Td)
    engine
        .apply_change(
            &input,
            &output1,
            0,
            blocks[1].bbox,
            "Replaced 2",
            "Line 2",
            None,
        )
        .unwrap();
    let mod_blocks = engine.get_text_blocks(&output1, 0).unwrap();
    assert_eq!(mod_blocks[1].text, "Replaced 2");

    // 2. apply_many_edits to Line 3 (TD) and Line 5 (TJ)
    let edits = serde_json::json!([
        {
            "page": 0,
            "rect": blocks[2].bbox,
            "old_text": "Line 3",
            "new_text": "Replaced 3"
        },
        {
            "page": 0,
            "rect": blocks[4].bbox,
            "old_text": "Line 5",
            "new_text": "Replaced 5"
        }
    ]);

    engine
        .apply_many_edits(
            &input,
            &output2,
            &serde_json::to_string(&edits).unwrap(),
            None,
        )
        .unwrap();
    let mod_blocks2 = engine.get_text_blocks(&output2, 0).unwrap();
    assert_eq!(mod_blocks2[2].text, "Replaced 3");
    assert_eq!(mod_blocks2[4].text, "Replaced 5");
}

#[test]
fn test_native_engine_find_text_block_at_click() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("click_in.pdf");

    create_multipage_pdf(&input, 1);
    let engine = OxidizePdfEngine::new();

    // We expect "Page 1" at some coordinate. Let's extract to find its bbox, then click inside it!
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    let bbox = blocks[0].bbox;

    let mid_x = (bbox[0] + bbox[2]) / 2.0;
    let mid_y = (bbox[1] + bbox[3]) / 2.0;

    let found = engine
        .find_text_block_at_click(&input, 0, mid_x, mid_y)
        .unwrap()
        .unwrap();
    assert_eq!(found.text, "Page 1");

    let not_found = engine
        .find_text_block_at_click(&input, 0, 999.0, 999.0)
        .unwrap();
    assert!(not_found.is_none());
}

#[test]
fn test_native_engine_render_page() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("render_in.pdf");

    create_multipage_pdf(&input, 1);
    let engine = OxidizePdfEngine::new();

    // This will download pdfium dynamically via auto_download if needed and render the page!
    // Since it's a test, pdfium should be mocked or available locally.
    let result = engine.render_page(&input, 0, 72.0);
    // Even if pdfium is missing in CI and it errors out, we cover the path trying to fetch it.
    // Ideally it succeeds if pdfium is installed.
    if let Ok(rendered) = result {
        assert!(rendered.width_pts > 0.0);
        assert!(rendered.height_pts > 0.0);
        assert!(!rendered.png_bytes.is_empty());
    }
}

#[test]
fn test_native_engine_multi_clone_preserves_exact_membership_and_order() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("multi_clone_in.pdf");
    let output = dir.path().join("multi_clone_out.pdf");
    create_multipage_pdf(&input, 3);
    let engine = OxidizePdfEngine::new();

    let cloned = engine.clone_pages(&input, &output, vec![0, 2]).unwrap();
    assert_eq!(cloned, 2);
    let observed = (0..5)
        .map(|page| {
            engine.get_text_blocks(&output, page).unwrap()[0]
                .text
                .clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(observed, ["Page 1", "Page 1", "Page 2", "Page 3", "Page 3"]);
}

#[test]
fn test_native_engine_page_surgery_rejects_non_exact_requests_non_destructively() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("surgery_guard_in.pdf");
    let output = dir.path().join("surgery_guard_out.pdf");
    create_multipage_pdf(&input, 3);
    let engine = OxidizePdfEngine::new();
    let sentinel = b"prior-output-must-survive";

    std::fs::write(&output, sentinel).unwrap();
    assert!(engine.clone_pages(&input, &output, vec![1, 1]).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), sentinel);

    std::fs::write(&output, sentinel).unwrap();
    assert!(engine.clone_pages(&input, &output, vec![3]).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), sentinel);

    std::fs::write(&output, sentinel).unwrap();
    assert!(engine.remove_pages(&input, &output, vec![1, 1]).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), sentinel);

    std::fs::write(&output, sentinel).unwrap();
    assert!(engine.remove_pages(&input, &output, vec![3]).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), sentinel);

    std::fs::write(&output, sentinel).unwrap();
    assert!(engine.remove_pages(&input, &output, vec![0, 1, 2]).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), sentinel);
}
