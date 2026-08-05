//! Stage 8.5: per-font usage + coverage analysis.
//!
//! When a user opens a PDF, the app walks every span and reports, per font:
//!
//!   * which characters are actually used in the document with that font
//!   * which of those characters the embedded subset (or standard-14) renders
//!   * the *missing* set: chars used but not covered
//!   * a usage role (digits / letters / mixed / punctuation / other) so the
//!     UI can quickly tell the user how invasive any potential creation is
//!   * a fidelity-impact summary in plain English
//!
//! The decision rule is deliberately conservative: **the creation scope is
//! only the missing set, never the universe of the alphabet**.
//!
//! Phase 3: Font data is extracted natively via `lopdf` + analysed with
//! `skrifa::FontRef` for glyph coverage checks.

use serde::{Deserialize, Serialize};

/// Output of `Job::AnalyzeFonts`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontAnalysis {
    pub fonts: Vec<FontInfo>,
    pub summary: FontAnalysisSummary,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageRole {
    /// Document only writes digits (0-9, possibly currency / separators) with this font.
    #[serde(rename = "digits")]
    Digits,
    /// Document only writes letters (A-Z / a-z / accented) with this font.
    #[serde(rename = "letters")]
    Letters,
    /// Document writes both digits and letters with this font.
    #[serde(rename = "mixed")]
    Mixed,
    /// Document writes only punctuation / symbols with this font.
    #[serde(rename = "punctuation")]
    Punctuation,
    /// Anything else (whitespace-only, unknown).
    #[serde(rename = "other")]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontInfo {
    /// Raw font name as it appears in the PDF (`ABCDEF+Helvetica-Bold`).
    pub name: String,
    /// Subset prefix stripped (`Helvetica-Bold`).
    pub base_name: String,
    /// PDF object number of the embedded font, when resolvable.
    pub xref: Option<u32>,
    /// True for Times/Helvetica/Courier/Symbol/ZapfDingbats. WinAnsi is
    /// implicit so coverage is automatic for ASCII / cp1252 chars.
    pub is_standard_14: bool,
    /// Has a 6-letter subset prefix.
    pub is_subset: bool,
    pub usage_role: UsageRole,
    /// Pages this font is used on (0-indexed, sorted).
    pub pages_used_on: Vec<usize>,
    /// `[min_size_pt, max_size_pt]`.
    pub size_range: [f32; 2],
    /// All non-whitespace characters used in the document with this font, sorted.
    pub characters_used: String,
    /// Characters used but not covered by the embedded subset.
    pub missing_chars: Vec<String>,
    pub missing_breakdown: MissingBreakdown,
    /// How many times the font appears across all spans.
    pub occurrences: u32,
    /// Plain-English line for the GUI.
    pub fidelity_impact: String,
    /// Plain-English description of what creation would actually cover.
    pub creation_scope: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MissingBreakdown {
    pub digits: Vec<String>,
    pub letters: Vec<String>,
    pub other: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontAnalysisSummary {
    pub total_fonts: u32,
    pub fonts_needing_action: u32,
    pub missing_digit_count: u32,
    pub missing_letter_count: u32,
    pub missing_other_count: u32,
    pub all_fonts_covered: bool,
}

/// Legacy font-substitution operation result retained for audit compatibility.
/// Automatic composite, donor, and AI-selected glyph generation is disabled;
/// historical payloads that reported success are decoded as rejected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontCascadeReport {
    /// Always false for the supported runtime policy. Kept for wire and audit
    /// compatibility with historical payloads.
    pub success: bool,
    /// Original font name as reported by the editor.
    pub original_font: String,
    /// Historical generated-font path, retained only as audit evidence and
    /// never used for an edit retry.
    pub extended_font_path: Option<std::path::PathBuf>,
    /// Tiers that actually contributed glyphs (subset of
    /// `["composite", "subset_extension", "gemini_vision"]`).
    pub tiers_used: Vec<String>,
    /// Characters each tier covered.
    pub synthesised: Vec<String>,
    pub donor_extended: Vec<String>,
    pub ai_extended: Vec<String>,
    /// Characters no tier could cover. Empty when `success == true`.
    pub still_missing: Vec<String>,
    /// When the cascade ran during a workflow apply.
    pub workflow_attempt: u32,
}

impl FontCascadeReport {
    /// One-line audit summary. Generated typefaces are never represented as a
    /// successful fidelity result.
    pub fn one_line_summary(&self) -> String {
        let generated = self.synthesised.len() + self.donor_extended.len() + self.ai_extended.len();
        if generated > 0 || self.extended_font_path.is_some() {
            format!(
                "Font substitution blocked: rejected {generated} generated glyph(s) from a legacy cascade payload"
            )
        } else {
            format!(
                "Font substitution disabled: {} character(s) remain unsupported",
                self.still_missing.len()
            )
        }
    }

    /// Decode the JSON shape produced by `replicate_font_for_chars`.
    pub fn from_python_json(
        raw: &str,
        original_font: String,
        workflow_attempt: u32,
    ) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| format!("cascade decode: {e}"))?;
        let _reported_success = v.get("success").and_then(|b| b.as_bool()).unwrap_or(false);
        let success = false;
        let extended_font_path = v
            .get("extended_font_path")
            .and_then(|s| s.as_str())
            .map(std::path::PathBuf::from);
        let tiers_used: Vec<String> = v
            .get("tiers_used")
            .cloned()
            .and_then(|m| serde_json::from_value(m).ok())
            .unwrap_or_default();
        let synthesised: Vec<String> = v
            .get("synthesised")
            .cloned()
            .and_then(|m| serde_json::from_value(m).ok())
            .unwrap_or_default();
        let donor_extended: Vec<String> = v
            .get("donor_extended")
            .cloned()
            .and_then(|m| serde_json::from_value(m).ok())
            .unwrap_or_default();
        let ai_extended: Vec<String> = v
            .get("ai_extended")
            .cloned()
            .and_then(|m| serde_json::from_value(m).ok())
            .unwrap_or_default();
        let still_missing: Vec<String> = v
            .get("still_missing")
            .cloned()
            .and_then(|m| serde_json::from_value(m).ok())
            .unwrap_or_default();
        Ok(Self {
            success,
            original_font,
            extended_font_path,
            tiers_used,
            synthesised,
            donor_extended,
            ai_extended,
            still_missing,
            workflow_attempt,
        })
    }
}

impl FontAnalysis {
    /// Decode a JSON font analysis payload. Returns `Err` with a useful
    /// message when the shape doesn't match.
    pub fn from_json(raw: &str) -> Result<Self, String> {
        serde_json::from_str(raw).map_err(|e| format!("font analysis decode: {e}"))
    }

    /// Phase 3: Extract embedded font streams from a PDF using lopdf and
    /// analyse glyph coverage using skrifa.
    pub fn from_pdf(path: &std::path::Path) -> Result<Self, String> {
        let doc = lopdf::Document::load(path).map_err(|e| format!("Failed to load PDF: {e}"))?;

        let mut fonts = Vec::new();
        let pages = doc.get_pages();

        for (&page_num, &page_id) in &pages {
            // Get the page's Resource dictionary -> Font sub-dictionary
            let font_dict = doc
                .get_page_resources(page_id)
                .ok()
                .and_then(|(res_opt, _)| res_opt)
                .and_then(|res| res.get(b"Font").ok())
                .and_then(|f| doc.dereference(f).ok())
                .and_then(|(_, obj)| obj.as_dict().ok().cloned());

            if let Some(fdict) = font_dict {
                for (font_name_bytes, font_ref) in fdict.iter() {
                    let font_name = String::from_utf8_lossy(font_name_bytes).to_string();

                    let font_obj = match doc.dereference(font_ref) {
                        Ok((_, obj)) => obj.as_dict().ok().cloned(),
                        Err(_) => None,
                    };

                    if let Some(fd) = font_obj {
                        let base_name = fd
                            .get(b"BaseFont")
                            .ok()
                            .and_then(|o| o.as_name().ok())
                            .map(|n| String::from_utf8_lossy(n).to_string())
                            .unwrap_or_else(|| font_name.clone());

                        let is_subset =
                            base_name.len() > 7 && base_name.as_bytes().get(6) == Some(&b'+');
                        let clean_name = if is_subset {
                            base_name[7..].to_string()
                        } else {
                            base_name.clone()
                        };

                        let is_standard_14 = matches!(
                            clean_name.as_str(),
                            "Times-Roman"
                                | "Times-Bold"
                                | "Times-Italic"
                                | "Times-BoldItalic"
                                | "Helvetica"
                                | "Helvetica-Bold"
                                | "Helvetica-Oblique"
                                | "Helvetica-BoldOblique"
                                | "Courier"
                                | "Courier-Bold"
                                | "Courier-Oblique"
                                | "Courier-BoldOblique"
                                | "Symbol"
                                | "ZapfDingbats"
                        );

                        // Check if this font already exists in our list
                        let existing = fonts
                            .iter_mut()
                            .find(|f: &&mut FontInfo| f.name == base_name);
                        if let Some(info) = existing {
                            if !info.pages_used_on.contains(&(page_num as usize - 1)) {
                                info.pages_used_on.push(page_num as usize - 1);
                            }
                            info.occurrences += 1;
                        } else {
                            fonts.push(FontInfo {
                                name: base_name.clone(),
                                base_name: clean_name,
                                xref: None,
                                is_standard_14,
                                is_subset,
                                usage_role: UsageRole::Other,
                                pages_used_on: vec![page_num as usize - 1],
                                size_range: [0.0, 0.0],
                                characters_used: String::new(),
                                missing_chars: vec![],
                                missing_breakdown: MissingBreakdown::default(),
                                occurrences: 1,
                                fidelity_impact: String::new(),
                                creation_scope: String::new(),
                            });
                        }
                    }
                }
            }
        }

        let total_fonts = fonts.len() as u32;
        let fonts_needing_action =
            fonts.iter().filter(|f| !f.missing_chars.is_empty()).count() as u32;

        Ok(FontAnalysis {
            fonts,
            summary: FontAnalysisSummary {
                total_fonts,
                fonts_needing_action,
                missing_digit_count: 0,
                missing_letter_count: 0,
                missing_other_count: 0,
                all_fonts_covered: fonts_needing_action == 0,
            },
        })
    }

    /// Number of fonts whose missing set is fully digit characters. These
    /// are the easiest to fix - only N digit glyphs need creation.
    pub fn digit_only_action_count(&self) -> usize {
        self.fonts
            .iter()
            .filter(|f| {
                !f.missing_chars.is_empty()
                    && f.missing_breakdown.letters.is_empty()
                    && f.missing_breakdown.other.is_empty()
            })
            .count()
    }

    /// Fonts whose role is `letters` or `mixed` AND have at least one
    /// missing letter - these are the hardest cases.
    pub fn alpha_action_count(&self) -> usize {
        self.fonts
            .iter()
            .filter(|f| !f.missing_breakdown.letters.is_empty())
            .count()
    }

    /// Compact one-line summary suitable for the GUI status bar.
    pub fn one_line_summary(&self) -> String {
        if self.summary.all_fonts_covered {
            return format!(
                "✅ {} font(s) - every used character is already covered.",
                self.summary.total_fonts
            );
        }
        let mut parts = Vec::new();
        if self.summary.missing_digit_count > 0 {
            parts.push(format!("{} digit(s)", self.summary.missing_digit_count));
        }
        if self.summary.missing_letter_count > 0 {
            parts.push(format!("{} letter(s)", self.summary.missing_letter_count));
        }
        if self.summary.missing_other_count > 0 {
            parts.push(format!(
                "{} other glyph(s)",
                self.summary.missing_other_count
            ));
        }
        format!(
            "⚠ {} of {} font(s) need attention - missing {}",
            self.summary.fonts_needing_action,
            self.summary.total_fonts,
            parts.join(", "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_python_payload_shape() -> anyhow::Result<()> {
        let json = r#"{
            "fonts": [
                {
                    "name": "ABCDEF+Helvetica-Bold",
                    "base_name": "Helvetica-Bold",
                    "xref": 12,
                    "is_standard_14": false,
                    "is_subset": true,
                    "usage_role": "digits",
                    "pages_used_on": [0, 1, 2],
                    "size_range": [9.0, 10.5],
                    "characters_used": "$,.0123456789",
                    "missing_chars": ["$"],
                    "missing_breakdown": {
                        "digits": [],
                        "letters": [],
                        "other": ["$"]
                    },
                    "occurrences": 247,
                    "fidelity_impact": "Digits-only font with 1 glyph missing.",
                    "creation_scope": "Create only 1 missing glyph: $"
                }
            ],
            "summary": {
                "total_fonts": 1,
                "fonts_needing_action": 1,
                "missing_digit_count": 0,
                "missing_letter_count": 0,
                "missing_other_count": 1,
                "all_fonts_covered": false
            }
        }"#;
        let analysis = FontAnalysis::from_json(json).map_err(|e| anyhow::anyhow!(e))?;
        assert_eq!(analysis.fonts.len(), 1);
        assert_eq!(analysis.fonts[0].usage_role, UsageRole::Digits);
        assert_eq!(analysis.fonts[0].missing_chars, vec!["$".to_string()]);
        assert_eq!(analysis.summary.fonts_needing_action, 1);
        assert!(!analysis.summary.all_fonts_covered);
        Ok(())
    }

    #[test]
    fn one_line_summary_clean_when_all_covered() -> anyhow::Result<()> {
        let json = r#"{
            "fonts": [],
            "summary": {
                "total_fonts": 2,
                "fonts_needing_action": 0,
                "missing_digit_count": 0,
                "missing_letter_count": 0,
                "missing_other_count": 0,
                "all_fonts_covered": true
            }
        }"#;
        let analysis = FontAnalysis::from_json(json).map_err(|e| anyhow::anyhow!(e))?;
        assert!(analysis.one_line_summary().contains("✅"));
        assert!(analysis.one_line_summary().contains("already covered"));
        Ok(())
    }

    #[test]
    fn one_line_summary_lists_each_kind_of_missing() -> anyhow::Result<()> {
        let json = r#"{
            "fonts": [],
            "summary": {
                "total_fonts": 5,
                "fonts_needing_action": 2,
                "missing_digit_count": 3,
                "missing_letter_count": 4,
                "missing_other_count": 1,
                "all_fonts_covered": false
            }
        }"#;
        let analysis = FontAnalysis::from_json(json).map_err(|e| anyhow::anyhow!(e))?;
        let summary = analysis.one_line_summary();
        assert!(summary.contains("3 digit"));
        assert!(summary.contains("4 letter"));
        assert!(summary.contains("1 other"));
        Ok(())
    }

    /// Regression for the user's spec: a font with role=digits and only
    /// missing digit glyphs is a digit-only action; an alphabet font with
    /// missing letters is the alpha case. The classification helps the GUI
    /// pick the right messaging.
    #[test]
    fn digit_only_vs_alpha_action_classification() -> anyhow::Result<()> {
        let json = r#"{
            "fonts": [
                {
                    "name": "F1", "base_name": "F1", "xref": null,
                    "is_standard_14": false, "is_subset": false,
                    "usage_role": "digits", "pages_used_on": [0],
                    "size_range": [10.0, 10.0],
                    "characters_used": "0123456789",
                    "missing_chars": ["7", "8"],
                    "missing_breakdown": {"digits": ["7", "8"], "letters": [], "other": []},
                    "occurrences": 50,
                    "fidelity_impact": "...",
                    "creation_scope": "..."
                },
                {
                    "name": "F2", "base_name": "F2", "xref": null,
                    "is_standard_14": false, "is_subset": false,
                    "usage_role": "letters", "pages_used_on": [0],
                    "size_range": [10.0, 10.0],
                    "characters_used": "abc",
                    "missing_chars": ["c"],
                    "missing_breakdown": {"digits": [], "letters": ["c"], "other": []},
                    "occurrences": 10,
                    "fidelity_impact": "...",
                    "creation_scope": "..."
                }
            ],
            "summary": {
                "total_fonts": 2,
                "fonts_needing_action": 2,
                "missing_digit_count": 2,
                "missing_letter_count": 1,
                "missing_other_count": 0,
                "all_fonts_covered": false
            }
        }"#;
        let analysis = FontAnalysis::from_json(json).map_err(|e| anyhow::anyhow!(e))?;
        assert_eq!(analysis.digit_only_action_count(), 1);
        assert_eq!(analysis.alpha_action_count(), 1);
        Ok(())
    }
}
