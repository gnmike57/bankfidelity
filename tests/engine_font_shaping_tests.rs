use dual_core_pdf_pipeline::engine::font_shaping::calculate_exact_width;
use std::path::PathBuf;

fn test_font_path() -> PathBuf {
    PathBuf::from("assets/Inter-Regular.ttf")
}

#[test]
fn calculates_exact_width_deterministically() {
    let font_path = test_font_path();
    if !font_path.exists() {
        eprintln!("fixture unavailable: {}", font_path.display());
        return;
    }

    let width = calculate_exact_width(&font_path, "Hello, World!", 12.0)
        .expect("valid fixture font must shape");
    let repeated = calculate_exact_width(&font_path, "Hello, World!", 12.0)
        .expect("repeated shaping must succeed");

    assert!(width > 0.0);
    assert!(width < 200.0);
    assert_eq!(width, repeated);
    assert_eq!(
        calculate_exact_width(&font_path, "", 12.0).expect("empty text must shape"),
        0.0
    );
}
