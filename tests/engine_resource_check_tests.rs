use dual_core_pdf_pipeline::engine::resource_check::{check_merged_resources, ResourceCheckError};
use lopdf::{
    content::Content, content::Operation, dictionary, Document, Object, Stream, StringFormat,
};

#[test]
fn test_resource_check_healthy_document() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("healthy.pdf");

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
        Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.0.into()]),
                Operation::new("Td", vec![100.0.into(), 100.0.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::String(
                        "Hello Geometry".into(),
                        StringFormat::Literal,
                    )],
                ),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .unwrap(),
    ));

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => vec![content_id.into()],
        "Resources" => resources_id,
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
    doc.save(&pdf_path).unwrap();

    let warnings = check_merged_resources(&pdf_path).expect("Expected healthy document");
    assert!(warnings.is_empty(), "Healthy doc should have no warnings");
}

#[test]
fn test_resource_check_dangling_content() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("dangling_content.pdf");

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    // Create a page pointing to a content stream that doesn't exist!
    let missing_content_id = (999, 0);

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => vec![Object::Reference(missing_content_id)],
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
    doc.save(&pdf_path).unwrap();

    let err = check_merged_resources(&pdf_path).unwrap_err();
    match err {
        ResourceCheckError::UnrenderablePage {
            global_page,
            category,
        } => {
            assert_eq!(global_page, 0);
            assert_eq!(category, "Contents");
        }
        _ => panic!("Expected UnrenderablePage with Contents category"),
    }
}

#[test]
fn test_resource_check_dangling_resources() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("dangling_resources.pdf");

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let missing_resources_id = (998, 0);

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => Object::Reference(missing_resources_id),
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
    doc.save(&pdf_path).unwrap();

    let err = check_merged_resources(&pdf_path).unwrap_err();
    match err {
        ResourceCheckError::UnrenderablePage {
            global_page,
            category,
        } => {
            assert_eq!(global_page, 0);
            assert_eq!(category, "Resources");
        }
        _ => panic!("Expected UnrenderablePage with Resources category"),
    }
}

#[test]
fn test_resource_check_warning_missing_font() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("missing_font.pdf");

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let missing_font_id = (997, 0);

    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => Object::Reference(missing_font_id),
        },
    });

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => resources_id,
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
    doc.save(&pdf_path).unwrap();

    let warnings = check_merged_resources(&pdf_path).expect("Should not fail, only warn");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("missing /Font resource"));
}
