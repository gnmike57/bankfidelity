use dual_core_pdf_pipeline::extractors::geometry::{
    detect_table_grid, GeometryProvider, NativeTextLayerProvider,
};
use dual_core_pdf_pipeline::pdf::OxidizePdfEngine;
use image::{DynamicImage, RgbaImage};
use std::sync::Arc;

#[test]
fn test_detect_table_grid_empty_image() {
    // Create an empty white image (100x100)
    let img = RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
    let dynamic_img = DynamicImage::ImageRgba8(img);

    let boxes = detect_table_grid(&dynamic_img);
    assert!(boxes.is_empty(), "Expected no boxes in a blank image");
}

#[test]
fn test_detect_table_grid_with_lines() {
    // Create an image and draw a black rectangle to simulate table borders
    let mut img = RgbaImage::from_pixel(200, 200, image::Rgba([255, 255, 255, 255]));
    let black = image::Rgba([0, 0, 0, 255]);

    // Draw horizontal lines
    for x in 20..180 {
        img.put_pixel(x, 50, black);
        img.put_pixel(x, 100, black);
    }

    // Draw vertical lines
    for y in 20..130 {
        img.put_pixel(50, y, black);
        img.put_pixel(150, y, black);
    }

    let dynamic_img = DynamicImage::ImageRgba8(img);
    let boxes = detect_table_grid(&dynamic_img);

    // Should detect at least one box around (50, 50) to (150, 100)
    let found = boxes.iter().any(|b| {
        (b.x0 - 50.0).abs() < 5.0
            && (b.x1 - 150.0).abs() < 5.0
            && (b.y0 - 50.0).abs() < 5.0
            && (b.y1 - 100.0).abs() < 5.0
    });

    assert!(
        found,
        "Should have found the drawn table cell, got boxes: {:?}",
        boxes
    );
}

#[tokio::test]
async fn test_native_text_layer_provider() {
    let engine = Arc::new(OxidizePdfEngine::new());
    let provider = NativeTextLayerProvider::new(engine);

    // Create a dummy PDF
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("dummy.pdf");
    use lopdf::{
        content::Content, content::Operation, dictionary, Document, Object, Stream, StringFormat,
    };

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
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
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

    let geometries = provider
        .extract_line_geometry(&pdf_path)
        .expect("Extraction failed");

    assert!(
        !geometries.is_empty(),
        "Should extract text geometries from PDF"
    );
    let text_found = geometries.iter().any(|g| g.text.contains("Hello"));
    assert!(text_found, "Extracted text should match PDF content");
}
