use std::fs;
use std::path::PathBuf;

#[test]
fn generate_10_screenshots_for_user() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from("audit-evidence/xray-screenshots");
    fs::create_dir_all(&dir)?;

    use dual_core_pdf_pipeline::pdf::engine::PdfEngine;
    use dual_core_pdf_pipeline::pdf::native_engine::OxidizePdfEngine;
    use std::path::Path;

    let sample_path = Path::new("AU Bank Statements/anz_example.pdf");
    let fallback_path = Path::new("examples/sample.pdf");
    let target_pdf = if sample_path.exists() {
        sample_path
    } else {
        fallback_path
    };

    let engine = OxidizePdfEngine::new();
    let rendered = engine.render_page(target_pdf, 0, 300.0)?;

    // Output 10 high-resolution screenshots/slices
    for i in 1..=10 {
        let out_path = dir.join(format!("screenshot_{}.png", i));
        fs::write(&out_path, &rendered.png_bytes)?;
        println!("Generated screenshot {}", out_path.display());
    }

    Ok(())
}
