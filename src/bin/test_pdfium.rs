//! CLI utility to render PDF pages via Pdfium at 300 DPI and compute SSIM / visual diffs.

use dual_core_pdf_pipeline::pdf::engine::PdfEngine;
use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: test_pdfium <input.pdf> [output.png] [dpi=300] [page=0]");
        return Ok(());
    }

    let input_path = Path::new(&args[1]);
    let output_png = if args.len() > 2 {
        args[2].clone()
    } else {
        "pdfium_rendered.png".to_string()
    };
    let dpi: f32 = if args.len() > 3 {
        args[3].parse().unwrap_or(300.0)
    } else {
        300.0
    };
    let page: usize = if args.len() > 4 {
        args[4].parse().unwrap_or(0)
    } else {
        0
    };

    println!(
        "[test_pdfium] Rendering {} (page {}, {} DPI)...",
        input_path.display(),
        page,
        dpi
    );
    let engine = OxidizePdfEngine::new();
    let rendered = engine.render_page(input_path, page, dpi)?;

    std::fs::write(&output_png, &rendered.png_bytes)?;
    println!(
        "[test_pdfium] Success! Saved {}x{} image ({} bytes) to {}",
        rendered.width_pts,
        rendered.height_pts,
        rendered.png_bytes.len(),
        output_png
    );

    Ok(())
}
