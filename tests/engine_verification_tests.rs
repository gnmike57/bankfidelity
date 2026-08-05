use dual_core_pdf_pipeline::engine::verification::{
    verify_edit_pages, MathInputs, VerificationDisposition, VerificationEvidencePackage,
    VERIFICATION_EVIDENCE_SCHEMA,
};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use rust_decimal_macros::dec;
use std::path::Path;

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

#[tokio::test]
async fn test_verify_edit_pages_identical() {
    let dir = tempfile::tempdir().unwrap();
    let orig = dir.path().join("orig.pdf");
    let edited = dir.path().join("edited.pdf");

    create_simple_pdf(&orig, &[("100.00", 50.0, 700.0)]);
    create_simple_pdf(&edited, &[("100.00", 50.0, 700.0)]);

    let math = MathInputs {
        transactions: vec![],
        expected_transactions: None,
        opening_balance: dec!(0.0),
        expected_final_balance: None,
        required: false,
    };

    let report = verify_edit_pages(
        &orig,
        &edited,
        dir.path(), // output_dir
        &[],        // no intended edits
        math,
        None,  // only_pages
        false, // auto_match_dpi
        None,  // vision_api_key
    )
    .await
    .expect("Verification failed");

    assert!(report.math_valid);
    assert!(report.only_intended_changes);
    assert!(report.mandatory_local_pass());
    // Since images are identical, scores should be very good.
    assert!(report.visual_diff_score < 0.01);
    let evidence: VerificationEvidencePackage = serde_json::from_slice(
        &std::fs::read(dir.path().join("verification_evidence.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(evidence.disposition, VerificationDisposition::Passed);
    assert_eq!(evidence.schema_version, VERIFICATION_EVIDENCE_SCHEMA);
    assert!(!evidence.artifacts.is_empty());
    assert!(evidence
        .artifacts
        .iter()
        .all(|artifact| artifact.bytes > 0 && artifact.sha256.len() == 64));
    assert_eq!(
        evidence.report.only_intended_changes,
        report.only_intended_changes
    );
    assert!(dir.path().join("verification_report.json").is_file());
}

#[tokio::test]
async fn test_verify_edit_pages_different() {
    let dir = tempfile::tempdir().unwrap();
    let orig = dir.path().join("orig2.pdf");
    let edited = dir.path().join("edited2.pdf");

    create_simple_pdf(&orig, &[("100.00", 50.0, 700.0)]);
    create_simple_pdf(&edited, &[("200.00", 50.0, 700.0)]);

    let math = MathInputs {
        transactions: vec![],
        expected_transactions: None,
        opening_balance: dec!(0.0),
        expected_final_balance: None,
        required: false,
    };

    // Provide a bounding box that does NOT cover the edit,
    // so it will be caught by the visual diff!
    let wrong_bbox = [0.0, 0.0, 10.0, 10.0];

    let report = verify_edit_pages(
        &orig,
        &edited,
        dir.path(),
        &[(0, wrong_bbox)],
        math,
        None,
        false,
        None,
    )
    .await
    .expect("Verification failed");

    // The visual difference is outside the intended bounding box.
    // So only_intended_changes should be FALSE.
    assert!(!report.only_intended_changes);
    assert!(!report.mandatory_local_pass());
    assert!(report.visual_diff_score > 0.0);
    let evidence: VerificationEvidencePackage = serde_json::from_slice(
        &std::fs::read(dir.path().join("verification_evidence.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(evidence.disposition, VerificationDisposition::Failed);
}

#[tokio::test]
async fn evidence_persistence_failure_blocks_verification_result() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.pdf");
    let edited = dir.path().join("edited.pdf");
    create_simple_pdf(&original, &[("100.00", 50.0, 700.0)]);
    create_simple_pdf(&edited, &[("100.00", 50.0, 700.0)]);
    let not_a_directory = dir.path().join("evidence-blocker");
    std::fs::write(&not_a_directory, b"existing file").unwrap();

    let result = verify_edit_pages(
        &original,
        &edited,
        &not_a_directory,
        &[],
        MathInputs {
            transactions: vec![],
            expected_transactions: None,
            opening_balance: dec!(0.0),
            expected_final_balance: None,
            required: false,
        },
        None,
        false,
        None,
    )
    .await;

    assert!(
        result.is_err(),
        "evidence failure must never return a report"
    );
    assert_eq!(std::fs::read(&not_a_directory).unwrap(), b"existing file");
}
