//! Document-Level Layout Analysis
//!
//! Analyzes the ENTIRE multi-page bank statement for structural and visual layout.
//! This helps the Smart Balance Engine make smarter decisions and preserve fidelity.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageLayout {
    pub page_number: usize,
    pub has_header: bool,
    pub has_footer: bool,
    pub has_page_number: bool,
    pub table_columns: usize,
    pub main_text_style: String,
    pub dominant_font: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentLayout {
    pub total_pages: usize,
    pub pages: Vec<PageLayout>,
    pub has_consistent_headers: bool,
    pub has_consistent_footers: bool,
    pub overall_style: String,
    pub layout_confidence: f32,
}

impl Default for DocumentLayout {
    fn default() -> Self {
        Self {
            total_pages: 0,
            pages: vec![],
            has_consistent_headers: false,
            has_consistent_footers: false,
            overall_style: "Unknown".to_string(),
            layout_confidence: 0.0,
        }
    }
}
