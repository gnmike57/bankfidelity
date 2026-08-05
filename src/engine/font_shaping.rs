//! Stage 8.6: Sub-pixel Font Shaping with rustybuzz
//!
//! Provides the `calculate_exact_width` function that takes a subsetted or extracted
//! font file and shapes a given string. It returns the exact sub-pixel width of the
//! string in design units. This allows the system to perfectly scale the text
//! injection to fit the original bank statement's bounding box without layout overflow.

use rustybuzz::{Face, UnicodeBuffer};
use std::fs;
use std::path::Path;

/// Calculates the exact width of a string in font design units using `rustybuzz`.
///
/// * `font_path` - The path to the font file (TTF/OTF) extracted via `fonttools`.
/// * `text` - The new text string to be shaped.
/// * `font_size_pt` - The target font size in points.
///
/// Returns the exact sub-pixel width in PDF points.
pub fn calculate_exact_width(
    font_path: &Path,
    text: &str,
    font_size_pt: f32,
) -> Result<f32, String> {
    let font_data = fs::read(font_path).map_err(|e| format!("Failed to read font file: {}", e))?;
    let face = Face::from_slice(&font_data, 0).ok_or("Failed to parse font face")?;

    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    // You can set direction, script, language here if needed.

    let glyph_buffer = rustybuzz::shape(&face, &[], buffer);

    // Calculate total advance width in design units
    let mut total_advance = 0;
    for pos in glyph_buffer.glyph_positions() {
        total_advance += pos.x_advance;
    }

    // Convert design units to PDF points.
    // PDF points = (design_units / units_per_em) * font_size_pt
    let upm = face.units_per_em() as f32;
    let width_pt = (total_advance as f32 / upm) * font_size_pt;

    Ok(width_pt)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A simple test to ensure the function signature works. In a real scenario,
    // you would pass a valid TTF file path.
    #[test]
    fn test_shaping_signature() {
        // Just verifying compilation and signature
        let _ = calculate_exact_width(Path::new("dummy.ttf"), "1,234.56", 10.0);
    }
}
