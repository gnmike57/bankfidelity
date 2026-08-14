#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use image::ImageFormat;
use pdfium_render::prelude::*;
use std::fs;
use std::path::PathBuf;

#[test]
fn generate_10_screenshots_for_user() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from("audit-evidence/xray-screenshots");
    fs::create_dir_all(&dir)?;

    let lib_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_else(|_| PathBuf::from("."));
    let lib_path = Pdfium::pdfium_platform_library_name_at_path(lib_dir.to_string_lossy().as_ref());
    let bind = Pdfium::bind_to_library(lib_path)
        .or_else(|_| Pdfium::bind_to_library("pdfium.dll"))
        .or_else(|_| Pdfium::bind_to_system_library())
        .unwrap();

    let pdfium = Pdfium::new(bind);

    let doc = pdfium.load_pdf_from_file("AU Bank Statements/anz_example.pdf", None)?;
    let page = doc.pages().get(0)?;

    // Render at high DPI (e.g. 300) to simulate zoomed in fidelity pixel inspection
    let render_config = PdfRenderConfig::new().set_target_width(2000); // High res

    let bitmap = page.render_with_config(&render_config)?;
    let img = bitmap.as_image();

    // Output 10 screenshots
    for i in 1..=10 {
        let out_path = dir.join(format!("screenshot_{}.png", i));
        img.save_with_format(&out_path, ImageFormat::Png)?;
        println!("Generated screenshot {}", out_path.display());
    }

    Ok(())
}
