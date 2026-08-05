use dual_core_pdf_pipeline::engine::font_replication::{
    extract_font_bytes_from_pdf, DeepFontReplicationResult,
};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;

fn create_pdf_with_font_file(path: &Path) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    // Create a mock FontFile2 stream
    let font_bytes = b"fake TTF data".to_vec();
    let font_file_id = doc.add_object(Stream::new(dictionary! {}, font_bytes));

    // Create a FontDescriptor
    let descriptor_id = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(b"TestFont".to_vec()),
        "FontFile2" => Object::Reference(font_file_id),
    });

    // Create the Font dictionary
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "TrueType",
        "BaseFont" => Object::Name(b"TestFont".to_vec()),
        "FontDescriptor" => Object::Reference(descriptor_id),
    });

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F1" => Object::Reference(font_id),
            },
        },
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
fn test_extract_font_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("font_test.pdf");

    create_pdf_with_font_file(&pdf_path);

    let _doc = lopdf::Document::load(&pdf_path).unwrap();

    // Should extract successfully
    let extracted = extract_font_bytes_from_pdf(&pdf_path, "TestFont").expect("Failed to extract");
    assert_eq!(extracted, b"fake TTF data");

    // Shouldn't find wrong font
    let missing = extract_font_bytes_from_pdf(&pdf_path, "OtherFont");
    assert!(missing.is_err());
}

#[test]
fn test_replicated_font_from_json() {
    let json = r#"{
        "success": true,
        "metrics": {
            "font_path": "/tmp/font.ttf",
            "upm": 1000,
            "ascender": 800,
            "descender": -200
        },
        "images": []
    }"#;

    let rep = DeepFontReplicationResult::from_json(json).expect("Parse failed");
    let path = rep.font_path().unwrap();
    assert_eq!(path.to_string_lossy(), "/tmp/font.ttf");
}
