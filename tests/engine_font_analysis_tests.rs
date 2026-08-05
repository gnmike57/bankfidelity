use dual_core_pdf_pipeline::engine::font_analysis::{
    FontAnalysis, FontAnalysisSummary, FontCascadeReport, FontInfo, MissingBreakdown, UsageRole,
};

#[test]
fn test_font_analysis_from_json() {
    let json_payload = r#"{
        "fonts": [
            {
                "name": "ABCDEF+Helvetica",
                "base_name": "Helvetica",
                "xref": 15,
                "is_standard_14": true,
                "is_subset": true,
                "usage_role": "mixed",
                "pages_used_on": [0, 1],
                "size_range": [10.0, 12.0],
                "characters_used": "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                "missing_chars": ["1", "5", "X"],
                "missing_breakdown": {
                    "digits": ["1", "5"],
                    "letters": ["X"],
                    "other": []
                },
                "occurrences": 150,
                "fidelity_impact": "Will look like Helvetica",
                "creation_scope": "Replace missing chars"
            }
        ],
        "summary": {
            "total_fonts": 1,
            "fonts_needing_action": 1,
            "missing_digit_count": 2,
            "missing_letter_count": 1,
            "missing_other_count": 0,
            "all_fonts_covered": false
        }
    }"#;

    let analysis = FontAnalysis::from_json(json_payload).expect("Failed to parse JSON");
    assert_eq!(analysis.fonts.len(), 1);

    let font = &analysis.fonts[0];
    assert_eq!(font.name, "ABCDEF+Helvetica");
    assert_eq!(font.usage_role, UsageRole::Mixed);
    assert_eq!(font.missing_breakdown.digits, vec!["1", "5"]);
    assert_eq!(font.missing_breakdown.letters, vec!["X"]);

    assert_eq!(analysis.summary.fonts_needing_action, 1);
    assert!(!analysis.summary.all_fonts_covered);
}

#[test]
fn test_font_analysis_action_counts() {
    let mut analysis = FontAnalysis {
        fonts: vec![],
        summary: FontAnalysisSummary {
            total_fonts: 0,
            fonts_needing_action: 0,
            missing_digit_count: 0,
            missing_letter_count: 0,
            missing_other_count: 0,
            all_fonts_covered: true,
        },
    };

    let base_font = FontInfo {
        name: "TestFont".into(),
        base_name: "TestFont".into(),
        xref: None,
        is_standard_14: false,
        is_subset: true,
        usage_role: UsageRole::Mixed,
        pages_used_on: vec![0],
        size_range: [10.0, 12.0],
        characters_used: "A1".into(),
        missing_chars: vec!["A".into(), "1".into()],
        missing_breakdown: MissingBreakdown {
            digits: vec!["1".into()],
            letters: vec!["A".into()],
            other: vec![],
        },
        occurrences: 5,
        fidelity_impact: "".into(),
        creation_scope: "".into(),
    };

    // Font 1: Only missing digits
    let mut f1 = base_font.clone();
    f1.missing_breakdown.digits = vec!["1".into()];
    f1.missing_breakdown.letters = vec![];
    analysis.fonts.push(f1);

    // Font 2: Missing digits and letters
    let mut f2 = base_font.clone();
    f2.missing_breakdown.digits = vec!["2".into()];
    f2.missing_breakdown.letters = vec!["B".into()];
    analysis.fonts.push(f2);

    // Font 3: Missing letters only
    let mut f3 = base_font.clone();
    f3.missing_breakdown.digits = vec![];
    f3.missing_breakdown.letters = vec!["C".into()];
    analysis.fonts.push(f3);

    // Font 4: Missing nothing
    let mut f4 = base_font.clone();
    f4.missing_chars = vec![];
    f4.missing_breakdown.digits = vec![];
    f4.missing_breakdown.letters = vec![];
    analysis.fonts.push(f4);

    assert_eq!(analysis.digit_only_action_count(), 1); // f1
    assert_eq!(analysis.alpha_action_count(), 2); // f2 and f3
}

#[test]
fn test_font_analysis_one_line_summary() {
    let analysis = FontAnalysis {
        fonts: vec![],
        summary: FontAnalysisSummary {
            total_fonts: 5,
            fonts_needing_action: 2,
            missing_digit_count: 3,
            missing_letter_count: 1,
            missing_other_count: 0,
            all_fonts_covered: false,
        },
    };

    assert_eq!(
        analysis.one_line_summary(),
        "⚠ 2 of 5 font(s) need attention - missing 3 digit(s), 1 letter(s)"
    );

    let analysis_clean = FontAnalysis {
        fonts: vec![],
        summary: FontAnalysisSummary {
            total_fonts: 5,
            fonts_needing_action: 0,
            missing_digit_count: 0,
            missing_letter_count: 0,
            missing_other_count: 0,
            all_fonts_covered: true,
        },
    };

    assert_eq!(
        analysis_clean.one_line_summary(),
        "✅ 5 font(s) - every used character is already covered."
    );
}

#[test]
fn test_font_cascade_report() {
    let json_payload = r#"{
        "success": true,
        "original_font": "Helvetica",
        "extended_font_path": "/tmp/helvetica_ext.ttf",
        "tiers_used": ["composite", "gemini_vision"],
        "synthesised": ["1", "2"],
        "donor_extended": [],
        "ai_extended": ["A"],
        "still_missing": [],
        "workflow_attempt": 1
    }"#;

    let report = FontCascadeReport::from_python_json(json_payload, "Helvetica".to_string(), 1)
        .expect("Parse failed");
    assert!(!report.success, "legacy synthesis success must be rejected");
    assert_eq!(report.original_font, "Helvetica");
    assert_eq!(report.synthesised.len(), 2);

    let summary = report.one_line_summary();
    assert_eq!(
        summary,
        "Font substitution blocked: rejected 3 generated glyph(s) from a legacy cascade payload"
    );
}

#[test]
fn test_font_cascade_report_failure() {
    let json_payload = r#"{
        "success": false,
        "original_font": "Helvetica",
        "extended_font_path": null,
        "tiers_used": [],
        "synthesised": [],
        "donor_extended": [],
        "ai_extended": [],
        "still_missing": ["X"],
        "workflow_attempt": 1
    }"#;

    let report = FontCascadeReport::from_python_json(json_payload, "Helvetica".to_string(), 1)
        .expect("Parse failed");
    assert!(!report.success);

    let summary = report.one_line_summary();
    assert_eq!(
        summary,
        "Font substitution disabled: 1 character(s) remain unsupported"
    );
}
