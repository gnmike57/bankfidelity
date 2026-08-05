use dual_core_pdf_pipeline::engine::verification::{VerificationGate, VerificationGateStatus};
use dual_core_pdf_pipeline::engine::verification_structural::verify_structural_invariants;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Dictionary, Document, Object, Stream, StringFormat};
use std::path::Path;

fn create_pdf(path: &Path, pages: &[&str], font_name: &str, title: &str) {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => font_name,
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut kids = Vec::new();
    for text in pages {
        let content = Content {
            operations: vec![
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
                        text.as_bytes().to_vec(),
                        StringFormat::Literal,
                    )],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id =
            document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.0.into(), 0.0.into(), 595.0.into(), 842.0.into()],
            "CropBox" => vec![0.0.into(), 0.0.into(), 595.0.into(), 842.0.into()],
        });
        kids.push(page_id.into());
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => pages.len() as i64,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Lang" => Object::string_literal("en-AU"),
    });
    let info_id = document.add_object(dictionary! {
        "Title" => Object::string_literal(title),
        "Author" => Object::string_literal("Verification Fixture"),
    });
    document.trailer.set("Root", catalog_id);
    document.trailer.set("Info", info_id);
    document.save(path).unwrap();
}

fn gate<'a>(gates: &'a [VerificationGate], id: &str) -> &'a VerificationGate {
    gates.iter().find(|gate| gate.id == id).unwrap()
}

#[test]
fn identical_structure_passes_every_mandatory_gate() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    let pages = [
        "Alpha Branch Opening Balance Customer Statement",
        "Zulu Merchant Closing Balance Customer Statement",
    ];
    create_pdf(&original, &pages, "Helvetica", "Monthly Statement");
    create_pdf(&edited, &pages, "Helvetica", "Monthly Statement");

    let gates = verify_structural_invariants(&original, &edited).unwrap();
    assert!(gates
        .iter()
        .all(|gate| !gate.mandatory || gate.status == VerificationGateStatus::Passed));
}

#[test]
fn page_count_and_order_negative_controls_fail() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let missing = directory.path().join("missing.pdf");
    let swapped = directory.path().join("swapped.pdf");
    let pages = [
        "Alpha Branch Opening Balance Customer Statement",
        "Zulu Merchant Closing Balance Customer Statement",
    ];
    create_pdf(&original, &pages, "Helvetica", "Monthly Statement");
    create_pdf(&missing, &pages[..1], "Helvetica", "Monthly Statement");
    create_pdf(
        &swapped,
        &[pages[1], pages[0]],
        "Helvetica",
        "Monthly Statement",
    );

    let missing_gates = verify_structural_invariants(&original, &missing).unwrap();
    assert_eq!(
        gate(&missing_gates, "structure.page_count").status,
        VerificationGateStatus::Failed
    );
    let swapped_gates = verify_structural_invariants(&original, &swapped).unwrap();
    assert_eq!(
        gate(&swapped_gates, "structure.page_identity").status,
        VerificationGateStatus::Failed
    );
}

#[test]
fn blank_page_and_geometry_drift_fail() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let blank = directory.path().join("blank.pdf");
    let drifted = directory.path().join("drifted.pdf");
    create_pdf(
        &original,
        &["Alpha Branch Opening Balance Customer Statement"],
        "Helvetica",
        "Monthly Statement",
    );

    let mut blank_document = Document::load(&original).unwrap();
    let blank_page = *blank_document.get_pages().values().next().unwrap();
    let empty_stream = blank_document.add_object(Stream::new(Dictionary::new(), Vec::new()));
    blank_document
        .get_dictionary_mut(blank_page)
        .unwrap()
        .set("Contents", empty_stream);
    blank_document.save(&blank).unwrap();

    let mut drifted_document = Document::load(&original).unwrap();
    let drifted_page = *drifted_document.get_pages().values().next().unwrap();
    drifted_document
        .get_dictionary_mut(drifted_page)
        .unwrap()
        .set(
            "MediaBox",
            vec![0.0.into(), 0.0.into(), 600.0.into(), 842.0.into()],
        );
    drifted_document.save(&drifted).unwrap();

    let blank_gates = verify_structural_invariants(&original, &blank).unwrap();
    assert_eq!(
        gate(&blank_gates, "structure.content_presence").status,
        VerificationGateStatus::Failed
    );
    let drifted_gates = verify_structural_invariants(&original, &drifted).unwrap();
    assert_eq!(
        gate(&drifted_gates, "structure.page_geometry").status,
        VerificationGateStatus::Failed
    );
}

#[test]
fn font_and_metadata_policy_negative_controls_fail() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let font_changed = directory.path().join("font-changed.pdf");
    let metadata_changed = directory.path().join("metadata-changed.pdf");
    let pages = ["Alpha Branch Opening Balance Customer Statement"];
    create_pdf(&original, &pages, "Helvetica", "Monthly Statement");
    create_pdf(&font_changed, &pages, "Courier", "Monthly Statement");
    create_pdf(
        &metadata_changed,
        &pages,
        "Helvetica",
        "Different Statement",
    );

    let font_gates = verify_structural_invariants(&original, &font_changed).unwrap();
    assert_eq!(
        gate(&font_gates, "structure.font_resources").status,
        VerificationGateStatus::Failed
    );
    let metadata_gates = verify_structural_invariants(&original, &metadata_changed).unwrap();
    assert_eq!(
        gate(&metadata_gates, "structure.metadata_policy").status,
        VerificationGateStatus::Failed
    );
}
