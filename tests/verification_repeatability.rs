use dual_core_pdf_pipeline::engine::verification::{
    verify_edit_pages, MathInputs, VerificationDisposition, VerificationEvidencePackage,
    VerificationGateStatus,
};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use rust_decimal::Decimal;
use std::path::Path;

fn create_pdf(path: &Path) {
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
                    b"Repeatable Verification Control".to_vec(),
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

#[derive(Debug, PartialEq)]
struct StableRun {
    disposition: VerificationDisposition,
    original_sha256: String,
    edited_sha256: String,
    policy_id: String,
    policy_sha256: String,
    thresholds: (u64, u64, u64, u32, u64),
    gates: Vec<(String, bool, VerificationGateStatus)>,
    artifacts: Vec<(String, String, u64)>,
}

fn stable_run(evidence: VerificationEvidencePackage) -> StableRun {
    let mut gates = evidence
        .report
        .gates
        .into_iter()
        .map(|gate| (gate.id, gate.mandatory, gate.status))
        .collect::<Vec<_>>();
    gates.sort_by(|left, right| left.0.cmp(&right.0));
    let mut artifacts = evidence
        .artifacts
        .into_iter()
        .map(|artifact| {
            let name = Path::new(&artifact.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (name, artifact.sha256, artifact.bytes)
        })
        .collect::<Vec<_>>();
    artifacts.sort();
    StableRun {
        disposition: evidence.disposition,
        original_sha256: evidence.original_sha256,
        edited_sha256: evidence.edited_sha256,
        policy_id: evidence.config.policy_id,
        policy_sha256: evidence.config.calibration_manifest_sha256,
        thresholds: (
            evidence.config.visual_diff_threshold.to_bits(),
            evidence.config.ssim_failure_floor.to_bits(),
            evidence.config.edit_region_failure_threshold.to_bits(),
            evidence.config.tile_px,
            evidence.config.mask_padding_pts.to_bits() as u64,
        ),
        gates,
        artifacts,
    }
}

#[tokio::test]
async fn identical_control_is_bitwise_repeatable_across_three_runs() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.pdf");
    let edited = directory.path().join("edited.pdf");
    create_pdf(&original);
    std::fs::copy(&original, &edited).unwrap();

    let mut runs = Vec::new();
    for index in 0..3 {
        let output = directory.path().join(format!("run-{index}"));
        let report = verify_edit_pages(
            &original,
            &edited,
            &output,
            &[],
            MathInputs {
                transactions: Vec::new(),
                expected_transactions: None,
                opening_balance: Decimal::ZERO,
                expected_final_balance: None,
                required: false,
            },
            None,
            false,
            None,
        )
        .await
        .unwrap();
        assert!(report.mandatory_local_pass());
        let evidence: VerificationEvidencePackage = serde_json::from_slice(
            &std::fs::read(output.join("verification_evidence.json")).unwrap(),
        )
        .unwrap();
        runs.push(stable_run(evidence));
    }

    assert_eq!(runs[0], runs[1]);
    assert_eq!(runs[1], runs[2]);
}
