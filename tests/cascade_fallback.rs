use dual_core_pdf_pipeline::app::runtime::{Job, JobResult};
use dual_core_pdf_pipeline::engine::model::ProposedChange;
use dual_core_pdf_pipeline::pdf::engine::PdfEngine;
use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use std::path::Path;
use std::sync::Arc;

fn create_four_page_pdf(path: &Path) {
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
    for i in 0..4 {
        let text = format!("Page {}", i);
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
                vec![Object::String(
                    text.as_bytes().to_vec(),
                    StringFormat::Literal,
                )],
            ),
            Operation::new("ET", vec![]),
        ];
        let content = Content { operations };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
            "Resources" => resources_id,
        });
        page_ids.push(page_id.into());
    }

    doc.objects.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Count" => 4,
            "Kids" => page_ids,
        }
        .into(),
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

#[test]
fn test_cascade_fallback_with_downed_actor() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("cascade_in.pdf");
    let output = dir.path().join("cascade_out.pdf");

    create_four_page_pdf(&input);

    // Force Python initialization to fail, simulating a downed actor
    std::env::set_var("TEST_CRASH_PYTHON_ACTOR", "1");

    let config = Arc::new(dual_core_pdf_pipeline::app::config::AppConfig::default());
    let audit_log = dual_core_pdf_pipeline::app::audit::AuditLog::open(dir.path()).unwrap();

    let (_runtime, job_tx, res_rx) =
        dual_core_pdf_pipeline::app::runtime::Runtime::start(audit_log, config);
    std::env::remove_var("TEST_CRASH_PYTHON_ACTOR");

    let geometry_engine = OxidizePdfEngine::new();
    let page_0_bbox = geometry_engine.get_text_blocks(&input, 0).unwrap()[0].bbox;
    let page_3_bbox = geometry_engine.get_text_blocks(&input, 3).unwrap()[0].bbox;
    let changes = vec![
        ProposedChange {
            page: 0,
            bbox: Some(page_0_bbox),
            old_text: "Page 0".into(),
            new_text: "Modified 0".into(),
            reason: "test".into(),
            confidence: 1.0,
            affects_subsequent_balances: false,
        },
        ProposedChange {
            page: 3,
            bbox: Some(page_3_bbox),
            old_text: "Page 3".into(),
            new_text: "Modified 3".into(),
            reason: "test".into(),
            confidence: 1.0,
            affects_subsequent_balances: false,
        },
    ];

    job_tx
        .send(Job::ApplyProposedChanges {
            input: input.clone(),
            output: output.clone(),
            changes,
        })
        .unwrap();

    let mut applied_success = false;

    while let Ok(res) = res_rx.recv() {
        if let JobResult::ProposedChangesApplied {
            changes_applied,
            failures,
        } = res
        {
            assert_eq!(changes_applied, 2);
            assert!(
                failures.is_empty(),
                "Expected no failures due to native fallback, got {:?}",
                failures
            );
            applied_success = true;
            break;
        }
        if let JobResult::Error { message, .. } = res {
            panic!("Job failed: {}", message);
        }
    }

    assert!(applied_success, "Did not receive ProposedChangesApplied");

    let engine = OxidizePdfEngine::new();
    let blocks_0 = engine.get_text_blocks(&output, 0).unwrap();
    println!("BLOCKS 0: {:#?}", blocks_0);
    assert_eq!(blocks_0[0].text, "Modified 0");

    let blocks_3 = engine.get_text_blocks(&output, 3).unwrap();
    assert_eq!(blocks_3[0].text, "Modified 3");
}
