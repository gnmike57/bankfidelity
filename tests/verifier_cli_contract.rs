use dual_core_pdf_pipeline::engine::verification::{
    VerificationDisposition, VerificationEvidencePackage,
};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use std::path::Path;
use std::process::Command;

fn create_pdf(path: &Path, text: &str) {
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

fn verifier_command(working_directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dual-core-pdf-pipeline"));
    command
        .current_dir(working_directory)
        .env(
            "DUAL_CORE_PASSPHRASE",
            "phase07-verifier-cli-contract-passphrase-1234",
        )
        .env_remove("GEMINI_API_KEY")
        .env_remove("VISION_API_KEY")
        .env_remove("PYMUPDF_PRO_KEY")
        .env_remove("PDFREST_API_KEY");
    command
}

fn read_evidence(path: &Path) -> VerificationEvidencePackage {
    serde_json::from_slice(&std::fs::read(path.join("verification_evidence.json")).unwrap())
        .unwrap()
}

#[test]
fn identical_pair_exits_success_and_persists_machine_evidence_without_api_chatter() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    let evidence = directory.path().join("evidence");
    create_pdf(&original, "100.00");
    create_pdf(&edited, "100.00");

    let output = verifier_command(directory.path())
        .args([
            "verify",
            "--original",
            original.to_str().unwrap(),
            "--edited",
            edited.to_str().unwrap(),
            "--output-dir",
            evidence.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("Evidence saved to:"));
    assert!(!combined.contains("API Key Verification Report"));
    assert_eq!(
        read_evidence(&evidence).disposition,
        VerificationDisposition::Passed
    );
    assert!(evidence.join("verification_report.json").is_file());
}

#[test]
fn unrequested_visible_mutation_exits_validation_failure_with_failed_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    let evidence = directory.path().join("evidence");
    create_pdf(&original, "100.00");
    create_pdf(&edited, "200.00");

    let output = verifier_command(directory.path())
        .args([
            "verify",
            "--original",
            original.to_str().unwrap(),
            "--edited",
            edited.to_str().unwrap(),
            "--output-dir",
            evidence.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains("API Key Verification Report"));
    assert_eq!(
        read_evidence(&evidence).disposition,
        VerificationDisposition::Failed
    );
}
