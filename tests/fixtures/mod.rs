//! Shared test utilities and synthetic PDF fixture generators.
//!
//! Provides `generate_test_pdf(pages, path)` which creates a minimal valid PDF
//! with the specified number of pages, containing deterministic text content.
//! This removes the dependency on `examples/sample.pdf` for basic integration
//! tests.

use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;

/// Generate a minimal N-page PDF at `path` with deterministic text content.
///
/// Each page contains a simple text line "Page N" where N is the 1-based page
/// index. The PDF uses the built-in Helvetica font (no embedded subset needed)
/// and a standard A4 media box.
///
/// # Panics
/// Panics if the file cannot be written to `path`.
pub fn generate_test_pdf(pages: usize, path: &Path) {
    assert!(pages > 0, "must generate at least 1 page");

    let mut doc = Document::with_version("1.5");

    // Create font dictionary referencing built-in Helvetica
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let font_dict = doc.add_object(dictionary! {
        "F1" => font_id,
    });

    let resources = dictionary! {
        "Font" => font_dict,
    };
    let resources_id = doc.add_object(resources);

    let mut page_ids = Vec::with_capacity(pages);

    for page_num in 1..=pages {
        // Build a minimal content stream: position cursor and show text
        let content =
            format!("BT /F1 12 Tf 72 720 Td (Page {page_num} - synthetic test fixture) Tj ET");
        let content_stream = Stream::new(dictionary! {}, content.into_bytes());
        let content_id = doc.add_object(content_stream);

        let page = dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(595),  // A4 width in points
                Object::Integer(842),  // A4 height in points
            ],
            "Resources" => resources_id,
            "Contents" => content_id,
        };
        let page_id = doc.add_object(page);
        page_ids.push(page_id);
    }

    // Build the Pages node (required by the PDF spec)
    let pages_dict = dictionary! {
        "Type" => "Pages",
        "Count" => page_ids.len() as u32,
        "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
    };
    let pages_id = doc.add_object(pages_dict);

    // Backpatch each page's Parent reference
    for &page_id in &page_ids {
        if let Ok(page_obj) = doc.get_object_mut(page_id) {
            if let Ok(dict) = page_obj.as_dict_mut() {
                dict.set("Parent", Object::Reference(pages_id));
            }
        }
    }

    // Build the Catalog
    let catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    let catalog_id = doc.add_object(catalog);

    doc.trailer.set("Root", Object::Reference(catalog_id));

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    doc.save(path)
        .unwrap_or_else(|e| panic!("failed to save test PDF to {}: {e}", path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generated_pdf_has_correct_page_count() {
        let dir = tempdir().unwrap();

        for pages in [1, 2, 3, 5, 10] {
            let path = dir.path().join(format!("test_{pages}.pdf"));
            generate_test_pdf(pages, &path);

            let doc = Document::load(&path).expect("should load generated PDF");
            assert_eq!(
                doc.get_pages().len(),
                pages,
                "generated PDF should have {pages} pages"
            );
        }
    }
}
