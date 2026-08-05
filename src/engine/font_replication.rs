//! Font Replication
//!
//! High-level orchestration for the deep font replication pipeline.
//!
//! Font extraction and glyph measurement are handled natively with `lopdf`
//! and `skrifa`. This module does not synthesize missing glyph programs: any
//! uncovered replacement character is an explicit unsupported disposition.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontMetrics {
    pub upm: u16,
    pub ascender: i16,
    pub descender: i16,
    pub font_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphImage {
    pub char: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepFontReplicationResult {
    pub success: bool,
    pub metrics: Option<FontMetrics>,
    pub images: Vec<GlyphImage>,
    pub baseline_y: Option<i32>,
    pub error: Option<String>,
}

impl DeepFontReplicationResult {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn font_path(&self) -> Option<PathBuf> {
        self.metrics.as_ref().map(|m| PathBuf::from(&m.font_path))
    }
}

/// Phase 3: Extract raw font data (TTF/OTF bytes) from an embedded PDF font.
///
/// Walks the font descriptor chain: Font -> FontDescriptor -> FontFile2 (TrueType)
/// or FontFile3 (CFF/OpenType). Returns the raw byte stream suitable for
/// loading into `skrifa::FontRef` or `write_fonts::FontBuilder`.
pub fn extract_font_bytes_from_pdf(pdf_path: &Path, font_name: &str) -> Result<Vec<u8>, String> {
    let doc = lopdf::Document::load(pdf_path).map_err(|e| format!("Failed to load PDF: {e}"))?;

    let pages = doc.get_pages();

    for &page_id in pages.values() {
        let font_dict = doc
            .get_page_resources(page_id)
            .ok()
            .and_then(|(res_opt, _)| res_opt)
            .and_then(|res| res.get(b"Font").ok())
            .and_then(|f| doc.dereference(f).ok())
            .and_then(|(_, obj)| obj.as_dict().ok().cloned());

        if let Some(fdict) = font_dict {
            for (_, font_ref) in fdict.iter() {
                let font_obj = match doc.dereference(font_ref) {
                    Ok((_, obj)) => match obj.as_dict() {
                        Ok(d) => d.clone(),
                        Err(_) => continue,
                    },
                    Err(_) => continue,
                };

                // Check if this is the font we're looking for
                let base = font_obj
                    .get(b"BaseFont")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .unwrap_or_default();

                if !base.contains(font_name) {
                    continue;
                }

                // Get FontDescriptor -> FontFile2/FontFile3
                let descriptor = font_obj
                    .get(b"FontDescriptor")
                    .ok()
                    .and_then(|d| doc.dereference(d).ok())
                    .and_then(|(_, obj)| obj.as_dict().ok().cloned());

                if let Some(desc) = descriptor {
                    // Try FontFile2 (TrueType) first, then FontFile3 (CFF/OTF)
                    for key in &[
                        b"FontFile2".as_ref(),
                        b"FontFile3".as_ref(),
                        b"FontFile".as_ref(),
                    ] {
                        if let Ok(stream_ref) = desc.get(key) {
                            if let Ok((_, stream_obj)) = doc.dereference(stream_ref) {
                                if let Ok(stream) = stream_obj.as_stream() {
                                    let bytes = stream
                                        .decompressed_content()
                                        .unwrap_or_else(|_| stream.content.clone());
                                    if !bytes.is_empty() {
                                        return Ok(bytes);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err(format!(
        "Font '{font_name}' not found or has no embedded data"
    ))
}

/// Phase 3: Measure glyph advance widths using skrifa for a given font.
///
/// Returns a map of character -> advance width (in font design units).
/// Used by the text editor to calculate pixel-perfect text placement.
pub fn measure_advances(font_data: &[u8], text: &str) -> Result<Vec<(char, f32)>, String> {
    use skrifa::raw::TableProvider;
    use skrifa::FontRef;
    use skrifa::MetadataProvider;

    let font = FontRef::new(font_data).map_err(|e| format!("Failed to parse font: {e}"))?;

    let upem = font.head().map(|h| h.units_per_em()).unwrap_or(1000) as f32;
    let charmap = font.charmap();

    let mut advances = Vec::new();
    let glyph_metrics = font.glyph_metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );

    for ch in text.chars() {
        let gid = charmap.map(ch);
        let advance = gid
            .and_then(|g| glyph_metrics.advance_width(g))
            .unwrap_or(upem * 0.5);
        advances.push((ch, advance / upem));
    }

    Ok(advances)
}

/// Validate that an extracted font program covers every required character.
///
/// The original bytes are returned unchanged only when coverage is complete.
/// Missing glyphs are never fabricated from donor outlines or metrics because
/// doing so would change the typeface while masquerading as fidelity.
pub fn require_font_subset_coverage(
    original_bytes: &[u8],
    required_text: &str,
) -> Result<(Vec<u8>, Vec<char>), String> {
    let face = ttf_parser::Face::parse(original_bytes, 0)
        .map_err(|e| format!("Failed to parse font: {e}"))?;

    // Check which characters are missing from the cmap
    let mut missing: Vec<char> = Vec::new();
    let mut present: Vec<char> = Vec::new();
    for ch in required_text.chars() {
        if face.glyph_index(ch).is_some() {
            present.push(ch);
        } else {
            missing.push(ch);
        }
    }

    if missing.is_empty() {
        // All characters covered - return original bytes unchanged
        return Ok((original_bytes.to_vec(), missing));
    }

    Err(format!(
        "FONT_COVERAGE_INSUFFICIENT: embedded font lacks required characters {missing:?}"
    ))
}

/// Check which characters from `required_text` are covered by the font's cmap.
/// Returns (covered, missing) character lists.
pub fn check_glyph_coverage(
    font_bytes: &[u8],
    required_text: &str,
) -> Result<(Vec<char>, Vec<char>), String> {
    let face =
        ttf_parser::Face::parse(font_bytes, 0).map_err(|e| format!("Failed to parse font: {e}"))?;

    let mut covered = Vec::new();
    let mut missing = Vec::new();

    for ch in required_text.chars() {
        if face.glyph_index(ch).is_some() {
            covered.push(ch);
        } else {
            missing.push(ch);
        }
    }

    Ok((covered, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_successful_payload_with_metrics() -> anyhow::Result<()> {
        let json = r#"{
            "success": true,
            "metrics": {
                "upm": 1000,
                "ascender": 800,
                "descender": -200,
                "font_path": "out/extracted_subset.ttf"
            },
            "images": [{"char": "A", "path": "out/glyph_65.png"}],
            "baseline_y": 200
        }"#;
        let parsed = DeepFontReplicationResult::from_json(json).map_err(|e| anyhow::anyhow!(e))?;
        assert!(parsed.success);
        assert_eq!(
            parsed
                .font_path()
                .ok_or_else(|| anyhow::anyhow!("No font path"))?
                .to_string_lossy(),
            "out/extracted_subset.ttf"
        );
        assert_eq!(parsed.images.len(), 1);
        Ok(())
    }

    #[test]
    fn parses_a_failure_payload() -> anyhow::Result<()> {
        let json = r#"{"success": false, "error": "Font not found", "images": []}"#;
        let parsed = DeepFontReplicationResult::from_json(json).map_err(|e| anyhow::anyhow!(e))?;
        assert!(!parsed.success);
        assert_eq!(parsed.error.as_deref(), Some("Font not found"));
        Ok(())
    }
}
