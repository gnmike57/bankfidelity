//! Natural Language Command Router
//!
//! Maps free-form natural language instructions to concrete [`Job`] variants.
//! This is the backbone of the Command Palette (Ctrl+P), the CLI `chat` subcommand,
//! and the HTTP `/chat` endpoint.
//!
//! ## Design
//! The router uses a two-pass strategy:
//! 1. **Pattern matching** — fast regex-based rules for common, unambiguous commands
//!    (e.g. "undo", "save", "balance", "verify").
//! 2. **AI fallback** — for ambiguous or complex instructions, the prompt is sent to
//!    the configured AI provider (Gemini/Mistral/local-llm) which returns a structured
//!    `NlpJobSpec` JSON that is then deserialized into a `Job`.
//!
//! ## Supported Commands (Pattern Layer)
//! | Intent | Example phrases |
//! |--------|-----------------|
//! | Undo | "undo", "revert last change", "go back" |
//! | Redo | "redo", "redo last", "reapply" |
//! | Balance | "balance", "fix balance", "reconcile", "auto-balance" |
//! | Verify | "verify", "check fidelity", "run xray", "visual check" |
//! | Extract | "extract transactions", "get transactions", "parse pdf" |
//! | Transfer | "transfer to [bank]", "copy to [bank]" |
//! | Adjust dates | "shift dates by N days", "move dates forward N days" |
//! | Categorize | "categorize", "classify transactions" |
//! | Doctor | "doctor", "health check", "check config", "diagnose" |
//! | Reload keys | "reload keys", "refresh api keys", "update credentials" |
//! | Stress test | "run stress test", "run transfer matrix", "run xray suite" |



/// A parsed NLP command ready to be dispatched as a [`Job`].
#[derive(Debug, Clone)]
pub enum NlpCommand {
    Undo,
    Redo,
    Balance { auto_apply: bool, target: Option<f64> },
    Verify,
    Extract { provider: String },
    Transfer { target_bank: String },
    AdjustDates { shift_days: i32 },
    Categorize { provider: String },
    Doctor,
    ReloadConfig,
    StressTest { test_type: String },
    AiEdit { instruction: String, provider: String },
    Unknown { raw: String },
}

/// Parse a natural language string into an [`NlpCommand`].
///
/// This is the fast pattern-matching layer. Complex instructions fall through
/// to `NlpCommand::AiEdit` for AI-assisted interpretation.
pub fn parse(input: &str) -> NlpCommand {
    let s = input.trim().to_lowercase();

    // ── Undo / Redo ──────────────────────────────────────────────────────────
    if matches_any(&s, &["undo", "revert last", "go back", "ctrl+z", "ctrl z"]) {
        return NlpCommand::Undo;
    }
    if matches_any(&s, &["redo", "reapply", "ctrl+y", "ctrl y", "redo last"]) {
        return NlpCommand::Redo;
    }

    // ── Balance ───────────────────────────────────────────────────────────────
    if matches_any(&s, &["balance", "reconcile", "fix balance", "auto balance", "auto-balance", "smart balance"]) {
        let auto_apply = s.contains("apply") || s.contains("auto") || s.contains("all");
        let target = extract_dollar_amount(&s);
        return NlpCommand::Balance { auto_apply, target };
    }

    // ── Verify ────────────────────────────────────────────────────────────────
    if matches_any(&s, &["verify", "check fidelity", "xray", "x-ray", "visual check", "pixel check", "ssim"]) {
        return NlpCommand::Verify;
    }

    // ── Extract ───────────────────────────────────────────────────────────────
    if matches_any(&s, &["extract", "get transactions", "parse", "read transactions"]) {
        let provider = detect_provider(&s).unwrap_or_else(|| "offline".to_string());
        return NlpCommand::Extract { provider };
    }

    // ── Transfer ──────────────────────────────────────────────────────────────
    if s.contains("transfer") || s.contains("copy to") || s.contains("move to") {
        let bank = detect_bank(&s).unwrap_or_else(|| "unknown".to_string());
        return NlpCommand::Transfer { target_bank: bank };
    }

    // ── Date adjustment ───────────────────────────────────────────────────────
    if s.contains("shift date") || s.contains("move date") || s.contains("adjust date") || s.contains("date forward") || s.contains("date back") {
        let days = extract_number(&s).unwrap_or(0) as i32;
        let shift = if s.contains("back") || s.contains("earlier") || s.contains("minus") { -days } else { days };
        return NlpCommand::AdjustDates { shift_days: shift };
    }

    // ── Categorize ────────────────────────────────────────────────────────────
    if matches_any(&s, &["categorize", "classify", "label transactions", "tag transactions"]) {
        let provider = detect_provider(&s).unwrap_or_else(|| "gemini".to_string());
        return NlpCommand::Categorize { provider };
    }

    // ── Doctor / health ───────────────────────────────────────────────────────
    if matches_any(&s, &["doctor", "health check", "diagnose", "check config", "system check"]) {
        return NlpCommand::Doctor;
    }

    // ── Reload config / keys ─────────────────────────────────────────────────
    if matches_any(&s, &["reload", "refresh keys", "update keys", "reload config", "hot reload"]) {
        return NlpCommand::ReloadConfig;
    }

    // ── Stress test ───────────────────────────────────────────────────────────
    if s.contains("stress test") || s.contains("transfer matrix") || s.contains("xray suite") || s.contains("run test") {
        let test_type = if s.contains("xray") || s.contains("fidelity") {
            "xray_fidelity"
        } else if s.contains("provider") || s.contains("probe") {
            "provider_probe"
        } else if s.contains("all") {
            "all"
        } else {
            "transfer_matrix"
        };
        return NlpCommand::StressTest { test_type: test_type.to_string() };
    }

    // ── AI edit fallback ─────────────────────────────────────────────────────
    // Any instruction that looks like an edit (contains action verbs) goes to AI
    let edit_verbs = ["change", "replace", "set", "update", "edit", "modify", "add", "remove",
                      "delete", "insert", "rename", "fix", "correct", "adjust", "rewrite"];
    if edit_verbs.iter().any(|v| s.contains(v)) {
        let provider = detect_provider(&s).unwrap_or_else(|| "gemini".to_string());
        return NlpCommand::AiEdit {
            instruction: input.trim().to_string(),
            provider,
        };
    }

    NlpCommand::Unknown { raw: input.trim().to_string() }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn matches_any(s: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| s.contains(p))
}

fn detect_provider(s: &str) -> Option<String> {
    if s.contains("gemini") { return Some("gemini".to_string()); }
    if s.contains("mistral") { return Some("mistral".to_string()); }
    if s.contains("llama") || s.contains("llamaparse") { return Some("llamaparse".to_string()); }
    if s.contains("local") || s.contains("ollama") { return Some("local-llm".to_string()); }
    if s.contains("offline") { return Some("offline".to_string()); }
    None
}

fn detect_bank(s: &str) -> Option<String> {
    let banks = [
        ("anz", "ANZ"),
        ("bankwest", "Bankwest"),
        ("commbank", "CommBank"),
        ("commonwealth", "CommBank"),
        ("ing", "ING"),
        ("macquarie", "Macquarie"),
        ("nab", "NAB"),
        ("westpac", "Westpac"),
    ];
    for (pattern, name) in &banks {
        if s.contains(pattern) {
            return Some(name.to_string());
        }
    }
    None
}

fn extract_dollar_amount(s: &str) -> Option<f64> {
    // Match patterns like "$5,432.10" or "5432.10" or "5000"
    let re_patterns = [
        r"\$([0-9,]+(?:\.[0-9]{2})?)",
        r"([0-9,]+\.[0-9]{2})",
        r"([0-9]{4,})",
    ];
    for pattern in &re_patterns {
        if let Some(cap) = regex_find(s, pattern) {
            let cleaned = cap.replace(',', "");
            if let Ok(v) = cleaned.parse::<f64>() {
                return Some(v);
            }
        }
    }
    None
}

fn extract_number(s: &str) -> Option<i64> {
    // Extract the first number from a string
    let mut num_str = String::new();
    let mut found = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            num_str.push(ch);
            found = true;
        } else if found {
            break;
        }
    }
    if found { num_str.parse().ok() } else { None }
}

/// Minimal regex-like find without pulling in the `regex` crate.
/// Only supports simple capture patterns for our use case.
fn regex_find(s: &str, pattern: &str) -> Option<String> {
    // Very simplified: just find dollar amounts
    if pattern.contains("\\$") {
        if let Some(pos) = s.find('$') {
            let rest = &s[pos + 1..];
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != ',' && c != '.').unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Format a human-readable description of a parsed command
// ---------------------------------------------------------------------------

impl NlpCommand {
    pub fn describe(&self) -> String {
        match self {
            NlpCommand::Undo => "Undo last change".to_string(),
            NlpCommand::Redo => "Redo last undone change".to_string(),
            NlpCommand::Balance { auto_apply, target } => {
                let t = target.map(|v| format!(" to ${:.2}", v)).unwrap_or_default();
                let a = if *auto_apply { " (auto-apply)" } else { " (preview)" };
                format!("Balance statement{}{}", t, a)
            }
            NlpCommand::Verify => "Run pixel-perfect fidelity verification".to_string(),
            NlpCommand::Extract { provider } => format!("Extract transactions using {}", provider),
            NlpCommand::Transfer { target_bank } => format!("Transfer transactions to {}", target_bank),
            NlpCommand::AdjustDates { shift_days } => {
                if *shift_days >= 0 {
                    format!("Shift all dates forward {} days", shift_days)
                } else {
                    format!("Shift all dates back {} days", shift_days.abs())
                }
            }
            NlpCommand::Categorize { provider } => format!("Categorize transactions using {}", provider),
            NlpCommand::Doctor => "Run system health check".to_string(),
            NlpCommand::ReloadConfig => "Hot-reload configuration and API keys".to_string(),
            NlpCommand::StressTest { test_type } => format!("Run stress test: {}", test_type),
            NlpCommand::AiEdit { instruction, provider } => {
                format!("AI edit via {} — \"{}\"", provider, &instruction[..instruction.len().min(60)])
            }
            NlpCommand::Unknown { raw } => format!("Unknown command: \"{}\"", raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undo_variants() {
        assert!(matches!(parse("undo"), NlpCommand::Undo));
        assert!(matches!(parse("Revert last change"), NlpCommand::Undo));
        assert!(matches!(parse("go back"), NlpCommand::Undo));
    }

    #[test]
    fn test_balance_with_target() {
        let cmd = parse("balance to $5,432.10");
        assert!(matches!(cmd, NlpCommand::Balance { target: Some(_), .. }));
        if let NlpCommand::Balance { target: Some(t), .. } = cmd {
            assert!((t - 5432.10).abs() < 0.01);
        }
    }

    #[test]
    fn test_date_shift_forward() {
        let cmd = parse("shift dates forward 30 days");
        assert!(matches!(cmd, NlpCommand::AdjustDates { shift_days: 30 }));
    }

    #[test]
    fn test_date_shift_backward() {
        let cmd = parse("move dates back 14 days");
        assert!(matches!(cmd, NlpCommand::AdjustDates { shift_days: -14 }));
    }

    #[test]
    fn test_transfer_bank_detection() {
        let cmd = parse("transfer to ANZ");
        assert!(matches!(cmd, NlpCommand::Transfer { .. }));
        if let NlpCommand::Transfer { target_bank } = cmd {
            assert_eq!(target_bank, "ANZ");
        }
    }

    #[test]
    fn test_ai_edit_fallback() {
        let cmd = parse("Change the account holder name to John Smith");
        assert!(matches!(cmd, NlpCommand::AiEdit { .. }));
    }

    #[test]
    fn test_verify_variants() {
        assert!(matches!(parse("verify"), NlpCommand::Verify));
        assert!(matches!(parse("check fidelity"), NlpCommand::Verify));
        assert!(matches!(parse("run xray"), NlpCommand::Verify));
    }

    #[test]
    fn test_stress_test_variants() {
        assert!(matches!(parse("run stress test"), NlpCommand::StressTest { .. }));
        assert!(matches!(parse("run transfer matrix"), NlpCommand::StressTest { .. }));
    }

    #[test]
    fn test_doctor() {
        assert!(matches!(parse("doctor"), NlpCommand::Doctor));
        assert!(matches!(parse("health check"), NlpCommand::Doctor));
        assert!(matches!(parse("diagnose"), NlpCommand::Doctor));
    }
}
