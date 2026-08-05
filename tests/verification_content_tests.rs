use dual_core_pdf_pipeline::engine::verification::{VerificationGateStatus, VerificationIntent};
use dual_core_pdf_pipeline::engine::verification_content::verify_intended_edit_membership;
use dual_core_pdf_pipeline::pdf::engine::PdfEngine;
use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use std::path::Path;

fn create_pdf(path: &Path, texts: &[&str]) {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut operations = vec![Operation::new("BT", vec![])];
    for text in texts {
        operations.extend([
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
                    text.as_bytes().to_vec(),
                    StringFormat::Literal,
                )],
            ),
        ]);
    }
    operations.push(Operation::new("ET", vec![]));
    let content = Content { operations };
    let content_id = document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.0.into(), 0.0.into(), 595.0.into(), 842.0.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    document.save(path).unwrap();
}

fn intent_for(original: &Path, old_text: &str, new_text: &str) -> VerificationIntent {
    let block = OxidizePdfEngine::new()
        .get_text_blocks(original, 0)
        .unwrap()
        .into_iter()
        .find(|block| block.text == old_text)
        .unwrap();
    VerificationIntent {
        page: 0,
        bbox: block.bbox,
        old_text: old_text.into(),
        new_text: new_text.into(),
    }
}

#[test]
fn exact_old_and_new_membership_passes() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    create_pdf(&original, &["OLD VALUE"]);
    create_pdf(&edited, &["NEW VALUE"]);
    let intent = intent_for(&original, "OLD VALUE", "NEW VALUE");

    let gate = verify_intended_edit_membership(&original, &edited, &[intent]).unwrap();
    assert_eq!(gate.status, VerificationGateStatus::Passed);
    assert!(gate.mandatory);
}

#[test]
fn stale_or_wrong_replacement_text_fails() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let stale = directory.path().join("stale.pdf");
    let wrong = directory.path().join("wrong.pdf");
    create_pdf(&original, &["OLD VALUE"]);
    create_pdf(&stale, &["OLD VALUE"]);
    create_pdf(&wrong, &["OTHER VALUE"]);
    let intent = intent_for(&original, "OLD VALUE", "NEW VALUE");

    assert_eq!(
        verify_intended_edit_membership(&original, &stale, std::slice::from_ref(&intent))
            .unwrap()
            .status,
        VerificationGateStatus::Failed
    );
    assert_eq!(
        verify_intended_edit_membership(&original, &wrong, &[intent])
            .unwrap()
            .status,
        VerificationGateStatus::Failed
    );
}

#[test]
fn duplicate_source_identity_is_ambiguous_and_fails() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    create_pdf(&original, &["OLD VALUE", "OLD VALUE"]);
    create_pdf(&edited, &["NEW VALUE"]);
    let intent = intent_for(&original, "OLD VALUE", "NEW VALUE");

    let gate = verify_intended_edit_membership(&original, &edited, &[intent]).unwrap();
    assert_eq!(gate.status, VerificationGateStatus::Failed);
    assert!(gate.message.contains("matched 2 targets"));
}

#[test]
fn duplicate_replacement_is_over_applied_and_fails() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    create_pdf(&original, &["OLD VALUE"]);
    create_pdf(&edited, &["NEW VALUE", "NEW VALUE"]);
    let intent = intent_for(&original, "OLD VALUE", "NEW VALUE");

    let gate = verify_intended_edit_membership(&original, &edited, &[intent]).unwrap();
    assert_eq!(gate.status, VerificationGateStatus::Failed);
    assert!(gate
        .message
        .contains("replacement identity matched 2 targets"));
}

#[test]
fn blanked_target_without_replacement_fails() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    create_pdf(&original, &["OLD VALUE"]);
    create_pdf(&edited, &[]);
    let intent = intent_for(&original, "OLD VALUE", "NEW VALUE");

    let gate = verify_intended_edit_membership(&original, &edited, &[intent]).unwrap();
    assert_eq!(gate.status, VerificationGateStatus::Failed);
    assert!(gate
        .message
        .contains("replacement identity matched 0 targets"));
}
