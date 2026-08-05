use dual_core_pdf_pipeline::app::config::{AppConfig, ConfigManager};
use dual_core_pdf_pipeline::pdf::engine::{EngineError, PdfEngine};
use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
use dual_core_pdf_pipeline::pdf::selector::PdfEngineSelector;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use std::path::Path;
use std::sync::Arc;

/// Helper to generate a simple PDF with specific text elements at exact coordinates.
fn create_simple_pdf(path: &Path, strings: &[(&str, f32, f32)]) {
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

    let mut operations = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 12.0.into()]),
    ];
    for (s, x, y) in strings {
        operations.push(Operation::new(
            "Tm",
            vec![
                1.0.into(),
                0.0.into(),
                0.0.into(),
                1.0.into(),
                (*x).into(),
                (*y).into(),
            ],
        ));
        operations.push(Operation::new(
            "Tj",
            vec![Object::String(s.as_bytes().to_vec(), StringFormat::Literal)],
        ));
    }
    operations.push(Operation::new("ET", vec![]));

    let content = Content { operations };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
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
fn test_native_engine_apply_change_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("single_edit_in.pdf");
    let output = dir.path().join("single_edit_out.pdf");

    create_simple_pdf(&input, &[("100.00", 50.0, 700.0)]);

    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 1);
    let target_bbox = blocks[0].bbox;

    engine
        .apply_change(&input, &output, 0, target_bbox, "200.00", "100.00", None)
        .unwrap();

    let out_blocks = engine.get_text_blocks(&output, 0).unwrap();
    assert_eq!(out_blocks.len(), 1);
    assert_eq!(out_blocks[0].text, "200.00");
}

#[test]
fn test_native_engine_repeated_value_target() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("repeated_edit_in.pdf");
    let output = dir.path().join("repeated_edit_out.pdf");

    // Two identical strings at different Y coordinates (e.g. transaction amount and running balance)
    create_simple_pdf(&input, &[("100.00", 50.0, 700.0), ("100.00", 50.0, 680.0)]);

    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 2);

    // Target the first one specifically by its exact bbox
    let target_bbox = blocks[0].bbox;

    engine
        .apply_change(&input, &output, 0, target_bbox, "200.00", "100.00", None)
        .unwrap();

    let out_blocks = engine.get_text_blocks(&output, 0).unwrap();
    assert_eq!(out_blocks.len(), 2);
    // The first one should be changed, the second should remain unchanged
    assert_eq!(out_blocks[0].text, "200.00");
    assert_eq!(out_blocks[1].text, "100.00");
}

#[test]
fn test_selector_rejects_non_overlapping_bbox() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("guard_in.pdf");
    let output = dir.path().join("guard_out.pdf");

    create_simple_pdf(&input, &[("Target", 50.0, 700.0)]);

    let primary = Arc::new(OxidizePdfEngine::new());
    let fallback = Arc::new(OxidizePdfEngine::new());
    let config = ConfigManager::new(Arc::new(AppConfig::default()));
    let selector = PdfEngineSelector::new(primary, fallback, config);

    // Provide a bbox completely outside the bounds of the "Target" text (e.g. y=100.0)
    let bad_bbox = [50.0, 100.0, 100.0, 115.0];

    let result = selector.apply_change(&input, &output, 0, bad_bbox, "Hacked", "Target", None);

    match result {
        Err(EngineError::RowDrifted { .. }) => {} // Expected rejection
        other => panic!("Expected RowDrifted error, got: {:?}", other),
    }
}

#[test]
fn test_native_engine_apply_many_edits_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("multi_edit_in.pdf");
    let output = dir.path().join("multi_edit_out.pdf");

    create_simple_pdf(&input, &[("First", 50.0, 700.0), ("Second", 50.0, 600.0)]);

    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 2);

    let edits_json = serde_json::json!([
        {
            "page": 0,
            "rect": blocks[0].bbox,
            "old_text": "First",
            "new_text": "Alpha"
        },
        {
            "page": 0,
            "rect": blocks[1].bbox,
            "old_text": "Second",
            "new_text": "Beta"
        }
    ])
    .to_string();

    engine
        .apply_many_edits(&input, &output, &edits_json, None)
        .unwrap();

    let out_blocks = engine.get_text_blocks(&output, 0).unwrap();
    assert_eq!(out_blocks.len(), 2);
    assert_eq!(out_blocks[0].text, "Alpha");
    assert_eq!(out_blocks[1].text, "Beta");
}

fn create_pdf_with_operations(
    path: &Path,
    operations: Vec<Operation>,
    rotation: Option<i64>,
    crop_box: Option<[f32; 4]>,
) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations }.encode().unwrap(),
    ));
    let mut page = dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.0.into(), 0.0.into(), 595.0.into(), 842.0.into()],
    };
    if let Some(rotation) = rotation {
        page.set("Rotate", rotation);
    }
    if let Some([x0, y0, x1, y1]) = crop_box {
        page.set("CropBox", vec![x0.into(), y0.into(), x1.into(), y1.into()]);
    }
    let page_id = doc.add_object(page);
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

#[test]
fn native_batch_rejects_old_text_mismatch_without_publishing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("identity-input.pdf");
    let output = dir.path().join("identity-output.pdf");
    create_simple_pdf(&input, &[("ORIGINAL", 50.0, 700.0)]);
    std::fs::write(&output, b"prior-output").unwrap();
    let engine = OxidizePdfEngine::new();
    let bbox = engine.get_text_blocks(&input, 0).unwrap()[0].bbox;
    let edits = serde_json::json!([{
        "page": 0,
        "rect": bbox,
        "old_text": "WRONG",
        "new_text": "REPLACED"
    }]);

    let result = engine.apply_many_edits(&input, &output, &edits.to_string(), None);
    assert!(
        matches!(result, Err(EngineError::ApplyFailed(message)) if message.contains("stable target not found"))
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"prior-output");
}

#[test]
fn native_batch_rejects_ambiguous_duplicate_operator_without_publishing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("duplicate-input.pdf");
    let output = dir.path().join("duplicate-output.pdf");
    create_simple_pdf(
        &input,
        &[("DUPLICATE", 50.0, 700.0), ("DUPLICATE", 50.0, 700.0)],
    );
    std::fs::write(&output, b"prior-output").unwrap();
    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].bbox, blocks[1].bbox);
    let edits = serde_json::json!([{
        "page": 0,
        "rect": blocks[0].bbox,
        "old_text": "DUPLICATE",
        "new_text": "ONE"
    }]);

    let result = engine.apply_many_edits(&input, &output, &edits.to_string(), None);
    assert!(
        matches!(result, Err(EngineError::ApplyFailed(message)) if message.contains("is ambiguous"))
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"prior-output");
}

#[test]
fn native_batch_rejects_two_edits_for_one_operator() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("double-input.pdf");
    let output = dir.path().join("double-output.pdf");
    create_simple_pdf(&input, &[("ONLY", 50.0, 700.0)]);
    let engine = OxidizePdfEngine::new();
    let bbox = engine.get_text_blocks(&input, 0).unwrap()[0].bbox;
    let edits = serde_json::json!([
        {"page": 0, "rect": bbox, "old_text": "ONLY", "new_text": "FIRST"},
        {"page": 0, "rect": bbox, "old_text": "ONLY", "new_text": "SECOND"}
    ]);

    let result = engine.apply_many_edits(&input, &output, &edits.to_string(), None);
    assert!(
        matches!(result, Err(EngineError::ApplyFailed(message)) if message.contains("multiple edits select"))
    );
    assert!(!output.exists());
}

#[test]
fn native_batch_applies_one_ctm_transformed_target_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("ctm-input.pdf");
    let output = dir.path().join("ctm-output.pdf");
    create_pdf_with_operations(
        &input,
        vec![
            Operation::new("q", vec![]),
            Operation::new(
                "cm",
                vec![
                    1.0.into(),
                    0.0.into(),
                    0.0.into(),
                    1.0.into(),
                    100.0.into(),
                    40.0.into(),
                ],
            ),
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
                vec![Object::String(
                    b"TRANSFORMED".to_vec(),
                    StringFormat::Literal,
                )],
            ),
            Operation::new("ET", vec![]),
            Operation::new("Q", vec![]),
        ],
        None,
        None,
    );
    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 1);
    assert!((blocks[0].bbox[0] - 150.0).abs() < 0.01);
    let edits = serde_json::json!([{
        "page": 0,
        "rect": blocks[0].bbox,
        "old_text": "TRANSFORMED",
        "new_text": "REPLACED"
    }]);

    let applied = engine
        .apply_many_edits(&input, &output, &edits.to_string(), None)
        .unwrap();
    assert_eq!(applied, 1);
    let output_blocks = engine.get_text_blocks(&output, 0).unwrap();
    assert_eq!(output_blocks[0].text, "REPLACED");
    assert_eq!(output_blocks[0].bbox[0], blocks[0].bbox[0]);
}

#[test]
fn native_batch_applies_one_cropped_target_at_visible_geometry() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("cropped-input.pdf");
    let output = dir.path().join("cropped-output.pdf");
    create_pdf_with_operations(
        &input,
        vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.0.into()]),
            Operation::new(
                "Tm",
                vec![
                    1.0.into(),
                    0.0.into(),
                    0.0.into(),
                    1.0.into(),
                    82.0.into(),
                    740.0.into(),
                ],
            ),
            Operation::new(
                "Tj",
                vec![Object::String(b"CROPPED".to_vec(), StringFormat::Literal)],
            ),
            Operation::new("ET", vec![]),
        ],
        None,
        Some([10.0, 20.0, 585.0, 822.0]),
    );
    let engine = OxidizePdfEngine::new();
    let blocks = engine.get_text_blocks(&input, 0).unwrap();
    assert_eq!(blocks.len(), 1);
    assert!((blocks[0].bbox[0] - 72.0).abs() < 0.01);
    assert!((blocks[0].bbox[1] - 70.0).abs() < 0.01);
    let edits = serde_json::json!([{
        "page": 0,
        "rect": blocks[0].bbox,
        "old_text": "CROPPED",
        "new_text": "UPDATED"
    }]);

    let applied = engine
        .apply_many_edits(&input, &output, &edits.to_string(), None)
        .unwrap();
    assert_eq!(applied, 1);
    let output_blocks = engine.get_text_blocks(&output, 0).unwrap();
    assert_eq!(output_blocks[0].text, "UPDATED");
    assert!((output_blocks[0].bbox[0] - blocks[0].bbox[0]).abs() < 0.01);
    assert!((output_blocks[0].bbox[1] - blocks[0].bbox[1]).abs() < 0.01);
}

#[test]
fn native_batch_applies_one_rotated_target_at_visible_geometry() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("rotated-input.pdf");
    let output = dir.path().join("rotated-output.pdf");
    create_pdf_with_operations(
        &input,
        vec![
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
                vec![Object::String(b"ROTATED".to_vec(), StringFormat::Literal)],
            ),
            Operation::new("ET", vec![]),
        ],
        Some(90),
        None,
    );
    std::fs::write(&output, b"prior-output").unwrap();
    let engine = OxidizePdfEngine::new();
    let bbox = engine.get_text_blocks(&input, 0).unwrap()[0].bbox;
    let edits = serde_json::json!([{
        "page": 0,
        "rect": bbox,
        "old_text": "ROTATED",
        "new_text": "REPLACED"
    }]);

    let applied = engine
        .apply_many_edits(&input, &output, &edits.to_string(), None)
        .unwrap();
    assert_eq!(applied, 1);
    let output_blocks = engine.get_text_blocks(&output, 0).unwrap();
    assert_eq!(output_blocks.len(), 1);
    assert_eq!(output_blocks[0].text, "REPLACED");
    assert!((output_blocks[0].bbox[0] - bbox[0]).abs() < 0.01);
    assert!((output_blocks[0].bbox[1] - bbox[1]).abs() < 0.01);
    assert_ne!(std::fs::read(&output).unwrap(), b"prior-output");
}
