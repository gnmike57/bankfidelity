use pdfium_render::prelude::*;
use std::fs;
use std::path::PathBuf;
use image::ImageFormat;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from("audit-evidence/xray-screenshots");
    fs::create_dir_all(&dir)?;

    let pdfium_bytes = std::fs::read("target/debug/pdfium.dll").unwrap_or_else(|_| {
        std::fs::read(r"C:\bankfidelity\bankfidelity\target\debug\pdfium.dll").unwrap_or_default()
    });
    
    // We can just rely on the system pdfium or bundled pdfium.
    let bind = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./")).or_else(|_| {
        Pdfium::bind_to_system_library()
    })?;
    
    let pdfium = Pdfium::new(bind);

    let doc = pdfium.load_pdf_from_file("AU Bank Statements/anz_example.pdf", None)?;
    let page = doc.pages().get(0)?;
    
    // Render at high DPI (e.g. 300) to simulate zoomed in fidelity pixel inspection
    let mut render_config = PdfRenderConfig::new();
    render_config.set_target_width(2000); // High res

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
