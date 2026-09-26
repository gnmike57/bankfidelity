//! Natural Language Command Router (Level 3 Autonomous Self-Correcting Engine)
//!
//! Maps free-form natural language instructions to concrete [`Job`] variants.
//! This is the backbone of the Command Palette (Ctrl+P), the CLI `chat` subcommand,
//! and the HTTP `/chat` endpoint.
//!
//! ## Design Architecture
//! 1. **Fast-path Deterministic Matching** — instantaneous keyword and phrase rules.
//! 2. **Fuzzy & Levenshtein Error-Correction** — tolerance for typos, transposed letters,
//!    and alternative Australian financial slang (e.g. "CBA", "CommBank", "balence", "reducto").
//! 3. **Autonomous Clarification & Auto-Repair** — synthesizes suggestions and auto-corrects
//!    partial instructions (e.g. missing bank names, ambiguous date shifts).
//! 4. **AI-Assisted Complex Intent Fallback** — forwards semantic edits to Reducto / Gemini / Local-LLM.

use serde::{Deserialize, Serialize};

/// A parsed NLP command ready to be dispatched as a [`Job`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NlpCommand {
    Undo,
    Redo,
    Balance {
        auto_apply: bool,
        target: Option<f64>,
    },
    Verify {
        mode: String,
    },
    Extract {
        provider: String,
    },
    Transfer {
        target_bank: String,
        source_bank: Option<String>,
    },
    AdjustDates {
        shift_days: i32,
    },
    Categorize {
        provider: String,
    },
    Doctor,
    ReloadConfig,
    StressTest {
        test_type: String,
    },
    TypstReconstruct,
    UfoAutomate {
        task_prompt: String,
    },
    FontAnalysis,
    AiEdit {
        instruction: String,
        provider: String,
    },
    ClarificationRequired {
        raw: String,
        reason: String,
        suggestions: Vec<String>,
    },
    Unknown {
        raw: String,
        suggestions: Vec<String>,
    },
}

/// Parse a natural language string into a self-correcting [`NlpCommand`].
pub fn parse(input: &str) -> NlpCommand {
    let s = input.trim();
    if s.is_empty() {
        return NlpCommand::ClarificationRequired {
            raw: String::new(),
            reason: "Empty prompt received".to_string(),
            suggestions: vec![
                "balance statement".to_string(),
                "extract with reducto".to_string(),
                "verify fidelity".to_string(),
                "doctor".to_string(),
            ],
        };
    }

    let lower = s.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    // ── 1. Undo / Redo ───────────────────────────────────────────────────────
    if matches_any(
        &lower,
        &["undo", "revert", "go back", "ctrl+z", "ctrl z", "undoo"],
    ) || is_fuzzy_token_match(&words, "undo", 1)
    {
        return NlpCommand::Undo;
    }
    if matches_any(
        &lower,
        &["redo", "reapply", "ctrl+y", "ctrl y", "redoo", "re-apply"],
    ) || is_fuzzy_token_match(&words, "redo", 1)
    {
        return NlpCommand::Redo;
    }

    // ── 2. Doctor / Health / Diagnostics ────────────────────────────────────
    if matches_any(
        &lower,
        &[
            "doctor",
            "health",
            "diagnose",
            "check config",
            "system check",
            "docter",
            "diagnostic",
            "check keys",
        ],
    ) || is_fuzzy_token_match(&words, "doctor", 1)
    {
        return NlpCommand::Doctor;
    }

    // ── 3. Reload Config / Keys ─────────────────────────────────────────────
    if matches_any(
        &lower,
        &[
            "reload",
            "refresh keys",
            "update keys",
            "reload config",
            "hot reload",
            "relod",
        ],
    ) {
        return NlpCommand::ReloadConfig;
    }

    // ── 4. Verify & Fidelity (X-Ray / SSIM / Pixel Check) ────────────────────
    if matches_any(
        &lower,
        &[
            "verify",
            "check fidelity",
            "xray",
            "x-ray",
            "visual check",
            "pixel check",
            "ssim",
            "verfy",
            "fidelity",
        ],
    ) || is_fuzzy_token_match(&words, "verify", 1)
    {
        let mode = if lower.contains("ssim") {
            "ssim".to_string()
        } else if lower.contains("xray") || lower.contains("x-ray") {
            "xray".to_string()
        } else if lower.contains("math") {
            "math".to_string()
        } else {
            "full".to_string()
        };
        return NlpCommand::Verify { mode };
    }

    // ── 5. Typst Layout Reconstruction ──────────────────────────────────────
    if matches_any(
        &lower,
        &[
            "typst",
            "reconstruct layout",
            "typst reflow",
            "typst reconstruct",
        ],
    ) {
        return NlpCommand::TypstReconstruct;
    }

    // ── 6. Font Analysis ────────────────────────────────────────────────────
    if matches_any(
        &lower,
        &[
            "font analysis",
            "analyze fonts",
            "font inspect",
            "inspect fonts",
            "font metrics",
        ],
    ) {
        return NlpCommand::FontAnalysis;
    }

    // ── 7. UFO UI Automation ────────────────────────────────────────────────
    if lower.starts_with("ufo")
        || lower.contains("automate ui")
        || lower.contains("windows automation")
    {
        let task_prompt = if lower.starts_with("ufo") {
            s.strip_prefix("ufo").unwrap_or(s).trim().to_string()
        } else {
            s.to_string()
        };
        return NlpCommand::UfoAutomate { task_prompt };
    }

    // ── 8. Balance / Reconcile ──────────────────────────────────────────────
    let balance_keywords = [
        "balance",
        "reconcile",
        "fix balance",
        "auto-balance",
        "smart balance",
        "balence",
        "reconsile",
    ];
    if matches_any(&lower, &balance_keywords) || is_fuzzy_token_match(&words, "balance", 2) {
        let auto_apply = lower.contains("apply") || lower.contains("auto") || lower.contains("all");
        let target = extract_dollar_amount(&lower);
        return NlpCommand::Balance { auto_apply, target };
    }

    // ── 9. Extract Transactions (OCR / Table Parsing) ────────────────────────
    let extract_keywords = [
        "extract",
        "get transactions",
        "parse",
        "read transactions",
        "extarct",
        "ocr",
        "pull transactions",
    ];
    if matches_any(&lower, &extract_keywords) || is_fuzzy_token_match(&words, "extract", 2) {
        let provider = detect_provider(&lower).unwrap_or_else(|| "reducto".to_string());
        return NlpCommand::Extract { provider };
    }

    // ── 10. Stress Test ─────────────────────────────────────────────────────
    if lower.contains("stress test")
        || lower.contains("transfer matrix")
        || lower.contains("xray suite")
        || lower.contains("run test")
        || lower.contains("benchmark")
    {
        let test_type = if lower.contains("xray") || lower.contains("fidelity") {
            "xray_fidelity"
        } else if lower.contains("provider") || lower.contains("probe") {
            "provider_probe"
        } else if lower.contains("all") || lower.contains("benchmark") {
            "all"
        } else {
            "transfer_matrix"
        };
        return NlpCommand::StressTest {
            test_type: test_type.to_string(),
        };
    }

    // ── 11. Transfer Transactions (Australian Banks) ─────────────────────────
    let transfer_keywords = [
        "transfer",
        "copy to",
        "move to",
        "convert to",
        "trasnfer",
        "transform to",
    ];
    if matches_any(&lower, &transfer_keywords) || is_fuzzy_token_match(&words, "transfer", 2) {
        if let Some(bank) = detect_bank(&lower) {
            return NlpCommand::Transfer {
                target_bank: bank,
                source_bank: None,
            };
        } else {
            // Missing bank name -> provide intelligent clarification
            return NlpCommand::ClarificationRequired {
                raw: s.to_string(),
                reason: "Transfer target bank was not recognized".to_string(),
                suggestions: vec![
                    "transfer to CommBank".to_string(),
                    "transfer to ANZ".to_string(),
                    "transfer to Westpac".to_string(),
                    "transfer to NAB".to_string(),
                    "transfer to Macquarie".to_string(),
                    "transfer to ING".to_string(),
                ],
            };
        }
    }

    // ── 12. Date Adjustment ─────────────────────────────────────────────────
    if lower.contains("shift date")
        || lower.contains("move date")
        || lower.contains("adjust date")
        || lower.contains("date forward")
        || lower.contains("date back")
        || lower.contains("change date")
        || lower.contains("dates by")
    {
        let days = extract_days_or_number(&lower).unwrap_or(0);
        let shift = if lower.contains("back")
            || lower.contains("earlier")
            || lower.contains("minus")
            || lower.contains("behind")
        {
            -(days.abs())
        } else {
            days.abs()
        };

        if shift == 0 {
            return NlpCommand::ClarificationRequired {
                raw: s.to_string(),
                reason: "Number of days to shift was not specified".to_string(),
                suggestions: vec![
                    "shift dates forward 30 days".to_string(),
                    "move dates back 14 days".to_string(),
                    "shift dates forward 7 days".to_string(),
                ],
            };
        }

        return NlpCommand::AdjustDates { shift_days: shift };
    }

    // ── 13. Categorize ──────────────────────────────────────────────────────
    if matches_any(
        &lower,
        &[
            "categorize",
            "classify",
            "label transactions",
            "tag transactions",
            "categorise",
        ],
    ) {
        let provider = detect_provider(&lower).unwrap_or_else(|| "local-llm".to_string());
        return NlpCommand::Categorize { provider };
    }

    // ── 14. AI Edit Fallback (Action Verbs) ──────────────────────────────────
    let edit_verbs = [
        "change", "replace", "set", "update", "edit", "modify", "add", "remove", "delete",
        "insert", "rename", "fix", "correct", "adjust", "rewrite", "double", "halve", "triple",
        "increase", "decrease", "reduce", "scale", "boost", "cut", "swap",
    ];
    if edit_verbs.iter().any(|v| lower.contains(v)) {
        let provider = detect_provider(&lower).unwrap_or_else(|| "reducto".to_string());
        return NlpCommand::AiEdit {
            instruction: s.to_string(),
            provider,
        };
    }

    // ── 15. Unknown Intent: Generate Auto-Repair Suggestions ────────────────
    let suggestions = generate_intent_suggestions(&lower);
    NlpCommand::Unknown {
        raw: s.to_string(),
        suggestions,
    }
}

// ---------------------------------------------------------------------------
// Entity Extraction & Fuzzy Resolvers
// ---------------------------------------------------------------------------

pub fn matches_any(s: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| s.contains(p))
}

/// Detects AI / parser provider with fuzzy tolerance. Default primary is Reducto.
pub fn detect_provider(s: &str) -> Option<String> {
    if s.contains("reducto") || s.contains("reducto ai") {
        return Some("reducto".to_string());
    }
    if s.contains("gemini") || s.contains("google") {
        return Some("gemini".to_string());
    }
    if s.contains("docai") || s.contains("document ai") || s.contains("documentai") {
        return Some("document-ai".to_string());
    }
    if s.contains("mistral") {
        return Some("mistral".to_string());
    }
    if s.contains("llama") || s.contains("llamaparse") {
        return Some("llamaparse".to_string());
    }
    if s.contains("mindee") {
        return Some("mindee".to_string());
    }
    if s.contains("pymupdf") || s.contains("fitz") {
        return Some("pymupdfpro".to_string());
    }
    if s.contains("local") || s.contains("ollama") || s.contains("qwen") {
        return Some("local-llm".to_string());
    }
    if s.contains("offline") || s.contains("heuristic") || s.contains("native") {
        return Some("offline".to_string());
    }
    None
}

/// Detects Australian bank entities with alias and fuzzy matching.
pub fn detect_bank(s: &str) -> Option<String> {
    let banks = [
        (
            &["commbank", "commonwealth", "cba", "comm bank", "combank"][..],
            "CommBank",
        ),
        (&["anz", "australia and new zealand", "anz bank"][..], "ANZ"),
        (&["westpac", "wbc", "westpac bank", "wespac"][..], "Westpac"),
        (
            &["nab", "national australia bank", "national bank"][..],
            "NAB",
        ),
        (&["ing", "ing direct", "ing bank"][..], "ING"),
        (&["macquarie", "macq", "macquarie bank"][..], "Macquarie"),
        (&["bankwest", "bank west"][..], "Bankwest"),
        (&["suncorp", "suncorp bank"][..], "Suncorp"),
        (&["st george", "stgeorge", "st. george"][..], "StGeorge"),
        (&["bendigo", "bendigo bank"][..], "Bendigo"),
    ];

    for (patterns, canonical) in &banks {
        for pat in *patterns {
            if s.contains(pat) {
                return Some(canonical.to_string());
            }
        }
    }

    // Fuzzy check for single word tokens
    for word in s.split_whitespace() {
        for (patterns, canonical) in &banks {
            for pat in *patterns {
                if levenshtein_distance(word, pat) <= 2 && pat.len() > 3 {
                    return Some(canonical.to_string());
                }
            }
        }
    }

    None
}

/// Extracts dollar amounts from string: supports `$5,432.10`, `$5000`, `5000.00`, `$5k`
pub fn extract_dollar_amount(s: &str) -> Option<f64> {
    // Check for $5k / 5k notation
    if let Some(k_pos) = s.find('k') {
        let prefix = &s[..k_pos];
        let num_part: String = prefix
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '$')
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let cleaned = num_part.replace('$', "");
        if let Ok(v) = cleaned.parse::<f64>() {
            return Some(v * 1000.0);
        }
    }

    if let Some(pos) = s.find('$') {
        let rest = &s[pos + 1..];
        let num_str: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
            .collect();
        let cleaned = num_str.replace(',', "");
        if let Ok(v) = cleaned.parse::<f64>() {
            return Some(v);
        }
    }

    for token in s.split_whitespace() {
        let cleaned = token.replace(['$', ','], "");
        if let Ok(v) = cleaned.parse::<f64>() {
            if v > 0.0 {
                return Some(v);
            }
        }
    }
    None
}

/// Extracts days count from strings like "30 days", "1 month" (30), "2 weeks" (14).
pub fn extract_days_or_number(s: &str) -> Option<i32> {
    if s.contains("month") {
        let num = extract_leading_number(s).unwrap_or(1);
        return Some(num * 30);
    }
    if s.contains("week") {
        let num = extract_leading_number(s).unwrap_or(1);
        return Some(num * 7);
    }

    extract_leading_number(s)
}

fn extract_leading_number(s: &str) -> Option<i32> {
    for token in s.split_whitespace() {
        if let Ok(v) = token.parse::<i32>() {
            return Some(v);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Levenshtein & Fuzzy Matching Utilities (Zero-Dependency)
// ---------------------------------------------------------------------------

pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let v1: Vec<char> = s1.chars().collect();
    let v2: Vec<char> = s2.chars().collect();
    let len1 = v1.len();
    let len2 = v2.len();

    let mut matrix = vec![vec![0usize; len2 + 1]; len1 + 1];

    for (i, row) in matrix.iter_mut().enumerate().take(len1 + 1) {
        row[0] = i;
    }
    for (j, cell) in matrix[0].iter_mut().enumerate().take(len2 + 1) {
        *cell = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if v1[i - 1] == v2[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + 2.min(cost));
        }
    }

    matrix[len1][len2]
}

fn is_fuzzy_token_match(words: &[&str], target: &str, max_dist: usize) -> bool {
    words
        .iter()
        .any(|w| levenshtein_distance(w, target) <= max_dist)
}

fn generate_intent_suggestions(s: &str) -> Vec<String> {
    let mut suggestions = Vec::new();

    if s.contains("bal") {
        suggestions.push("balance statement".to_string());
        suggestions.push("balance to $5,000.00".to_string());
    } else if s.contains("ext") || s.contains("get") {
        suggestions.push("extract with reducto".to_string());
        suggestions.push("extract transactions".to_string());
    } else if s.contains("trans") || s.contains("bank") {
        suggestions.push("transfer to CommBank".to_string());
        suggestions.push("transfer to ANZ".to_string());
    } else if s.contains("date") || s.contains("day") {
        suggestions.push("shift dates forward 30 days".to_string());
        suggestions.push("move dates back 14 days".to_string());
    } else {
        suggestions.push("balance statement".to_string());
        suggestions.push("extract with reducto".to_string());
        suggestions.push("verify fidelity".to_string());
        suggestions.push("doctor".to_string());
    }

    suggestions
}

// ---------------------------------------------------------------------------
// Human-Readable Formatting & Descriptions
// ---------------------------------------------------------------------------

impl NlpCommand {
    pub fn describe(&self) -> String {
        match self {
            NlpCommand::Undo => "Undo last change".to_string(),
            NlpCommand::Redo => "Redo last undone change".to_string(),
            NlpCommand::Balance { auto_apply, target } => {
                let t = target.map(|v| format!(" to ${:.2}", v)).unwrap_or_default();
                let a = if *auto_apply {
                    " (auto-apply)"
                } else {
                    " (preview)"
                };
                format!("Balance statement{}{}", t, a)
            }
            NlpCommand::Verify { mode } => format!("Run fidelity verification (mode: {})", mode),
            NlpCommand::Extract { provider } => format!("Extract transactions using {}", provider),
            NlpCommand::Transfer {
                target_bank,
                source_bank,
            } => {
                if let Some(src) = source_bank {
                    format!("Transfer transactions from {} to {}", src, target_bank)
                } else {
                    format!("Transfer transactions to {}", target_bank)
                }
            }
            NlpCommand::AdjustDates { shift_days } => {
                if *shift_days >= 0 {
                    format!("Shift all dates forward {} days", shift_days)
                } else {
                    format!("Shift all dates back {} days", shift_days.abs())
                }
            }
            NlpCommand::Categorize { provider } => {
                format!("Categorize transactions using {}", provider)
            }
            NlpCommand::Doctor => "Run system health check and configuration audit".to_string(),
            NlpCommand::ReloadConfig => "Hot-reload configuration and API credentials".to_string(),
            NlpCommand::StressTest { test_type } => format!("Run stress test suite: {}", test_type),
            NlpCommand::TypstReconstruct => "High-Fidelity Typst layout reconstruction".to_string(),
            NlpCommand::UfoAutomate { task_prompt } => {
                format!("Microsoft UFO automation: \"{}\"", task_prompt)
            }
            NlpCommand::FontAnalysis => "Extract and analyze document font metrics".to_string(),
            NlpCommand::AiEdit {
                instruction,
                provider,
            } => {
                format!(
                    "AI edit via {} — \"{}\"",
                    provider,
                    &instruction[..instruction.len().min(60)]
                )
            }
            NlpCommand::ClarificationRequired {
                reason,
                suggestions,
                ..
            } => {
                format!(
                    "Clarification required: {}. Suggestions: {}",
                    reason,
                    suggestions.join(" | ")
                )
            }
            NlpCommand::Unknown { raw, suggestions } => {
                format!(
                    "Unknown command: \"{}\". Try: {}",
                    raw,
                    suggestions.join(" | ")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_undo_and_redo_fuzzy() {
        assert_eq!(parse("undo"), NlpCommand::Undo);
        assert_eq!(parse("undoo"), NlpCommand::Undo);
        assert_eq!(parse("redo"), NlpCommand::Redo);
        assert_eq!(parse("redoo"), NlpCommand::Redo);
        assert_eq!(parse("revert last change"), NlpCommand::Undo);
    }

    #[test]
    fn test_balance_fuzzy_and_amounts() {
        let cmd1 = parse("balence to $5,432.10");
        match cmd1 {
            NlpCommand::Balance {
                target: Some(t), ..
            } => assert!((t - 5432.10).abs() < 0.01),
            _ => panic!("Expected Balance with target amount"),
        }

        let cmd2 = parse("balance to $5k auto");
        match cmd2 {
            NlpCommand::Balance {
                auto_apply,
                target: Some(t),
            } => {
                assert!(auto_apply);
                assert!((t - 5000.0).abs() < 0.01);
            }
            _ => panic!("Expected Balance with 5k target amount"),
        }
    }

    #[test]
    fn test_extract_with_reducto_primary() {
        let cmd = parse("extarct with reducto");
        assert_eq!(
            cmd,
            NlpCommand::Extract {
                provider: "reducto".to_string()
            }
        );

        let cmd_default = parse("extract transactions");
        assert_eq!(
            cmd_default,
            NlpCommand::Extract {
                provider: "reducto".to_string()
            }
        );
    }

    #[test]
    fn test_transfer_fuzzy_banks() {
        let cmd = parse("trasnfer to commonwealth");
        assert_eq!(
            cmd,
            NlpCommand::Transfer {
                target_bank: "CommBank".to_string(),
                source_bank: None
            }
        );

        let cmd_cba = parse("transfer to cba");
        assert_eq!(
            cmd_cba,
            NlpCommand::Transfer {
                target_bank: "CommBank".to_string(),
                source_bank: None
            }
        );

        let cmd_anz = parse("transfer to anz bank");
        assert_eq!(
            cmd_anz,
            NlpCommand::Transfer {
                target_bank: "ANZ".to_string(),
                source_bank: None
            }
        );
    }

    #[test]
    fn test_transfer_missing_bank_clarification() {
        let cmd = parse("transfer");
        match cmd {
            NlpCommand::ClarificationRequired { suggestions, .. } => {
                assert!(!suggestions.is_empty());
            }
            _ => panic!("Expected ClarificationRequired for missing transfer bank"),
        }
    }

    #[test]
    fn test_date_adjustment_variations() {
        assert_eq!(
            parse("shift dates forward 30 days"),
            NlpCommand::AdjustDates { shift_days: 30 }
        );
        assert_eq!(
            parse("move dates back 2 weeks"),
            NlpCommand::AdjustDates { shift_days: -14 }
        );
        assert_eq!(
            parse("date forward 1 month"),
            NlpCommand::AdjustDates { shift_days: 30 }
        );
    }

    #[test]
    fn test_doctor_and_typst() {
        assert_eq!(parse("docter"), NlpCommand::Doctor);
        assert_eq!(parse("health check"), NlpCommand::Doctor);
        assert_eq!(parse("typst reconstruct"), NlpCommand::TypstReconstruct);
        assert_eq!(parse("reload config"), NlpCommand::ReloadConfig);
        assert_eq!(parse("font analysis"), NlpCommand::FontAnalysis);
    }

    #[test]
    fn test_verify_modes() {
        assert_eq!(
            parse("verify fidelity"),
            NlpCommand::Verify {
                mode: "full".to_string()
            }
        );
        assert_eq!(
            parse("xray check"),
            NlpCommand::Verify {
                mode: "xray".to_string()
            }
        );
        assert_eq!(
            parse("ssim visual test"),
            NlpCommand::Verify {
                mode: "ssim".to_string()
            }
        );
    }

    #[test]
    fn test_ufo_automation() {
        let cmd = parse("ufo download bank statement from Chrome");
        assert_eq!(
            cmd,
            NlpCommand::UfoAutomate {
                task_prompt: "download bank statement from Chrome".to_string()
            }
        );
    }

    #[test]
    fn test_ai_edit_and_unknown() {
        let cmd = parse("change salary to $4500");
        match cmd {
            NlpCommand::AiEdit {
                instruction,
                provider,
            } => {
                assert_eq!(instruction, "change salary to $4500");
                assert_eq!(provider, "reducto");
            }
            _ => panic!("Expected AiEdit variant"),
        }

        let unknown = parse("xyzqwerty nonexistent command");
        match unknown {
            NlpCommand::Unknown { suggestions, .. } => {
                assert!(!suggestions.is_empty());
            }
            _ => panic!("Expected Unknown variant"),
        }
    }
}
