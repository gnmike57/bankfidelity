//! # Financial NLP Engine
//!
//! A deterministic, domain-aware semantic parser for natural language edits to
//! Australian bank statements. This module intercepts financial intents *before*
//! they hit an LLM, resolves entities (payees, amounts, pay cycles) against the
//! actual transaction data, applies the math using `rust_decimal::Decimal`, and
//! cascades the running balance from the first edited row onwards.
//!
//! ## Why deterministic first, LLM second?
//!
//! LLMs are excellent at understanding *what* the user wants ("double Maree's pay")
//! but are unreliable at arithmetic and balance cascading across hundreds of rows.
//! This engine uses the LLM only for entity resolution when needed (e.g. "which
//! transactions belong to Maree?"), then applies all math and balance updates in
//! Rust with `Decimal` precision.
//!
//! ## Supported intents
//!
//! | Intent | Example |
//! |---|---|
//! | `ScaleIncome` | "double Maree's pay", "increase salary by 50%" |
//! | `ScaleExpense` | "halve my rent", "reduce Woolworths by $50" |
//! | `SetAmount` | "set Maree's pay to $3000", "change rent to $1500" |
//! | `RenamePayee` | "change Woolworths to Coles", "rename salary to wages" |
//! | `RemoveTransactions` | "remove all Uber transactions" |
//! | `DateShiftPayee` | "move all rent payments to the 1st" |
//! | `AddTransaction` | "add a $500 deposit from Maree on 15 Jan" |
//! | `ScaleAll` | "increase all income by 10%", "reduce all expenses by 5%" |

use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::engine::model::Transaction;

// ─────────────────────────────────────────────────────────────────────────────
// Intent taxonomy
// ─────────────────────────────────────────────────────────────────────────────

/// The parsed financial intent from a natural language instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum FinancialIntent {
    /// Scale a specific payee's income transactions by a factor.
    /// "double Maree's pay" → ScaleIncome { payee: "Maree", factor: 2.0, field: IncomeField::Debit }
    ScaleIncome {
        payee: Option<String>,
        factor: Decimal,
    },
    /// Scale a specific payee's expense transactions by a factor.
    ScaleExpense {
        payee: Option<String>,
        factor: Decimal,
    },
    /// Set a specific payee's transactions to a fixed amount.
    SetAmount {
        payee: Option<String>,
        amount: Decimal,
        is_income: bool,
    },
    /// Rename a payee/description across all matching transactions.
    RenamePayee {
        from: String,
        to: String,
    },
    /// Remove all transactions matching a payee.
    RemoveTransactions {
        payee: String,
    },
    /// Scale all income transactions by a factor.
    ScaleAllIncome {
        factor: Decimal,
    },
    /// Scale all expense transactions by a factor.
    ScaleAllExpenses {
        factor: Decimal,
    },
    /// Add a new transaction.
    AddTransaction {
        date: String,
        description: String,
        amount: Decimal,
        is_income: bool,
    },
    /// Not a financial intent — pass to the LLM.
    Unknown,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pay cycle detection
// ─────────────────────────────────────────────────────────────────────────────

/// Detected pay cycle for a payee.
#[derive(Debug, Clone)]
pub struct PayCycle {
    pub payee: String,
    pub frequency_days: Option<u32>,
    pub typical_amount: Option<Decimal>,
    pub transaction_count: usize,
    pub is_income: bool,
}

/// Detect recurring pay cycles for all payees in a transaction list.
pub fn detect_pay_cycles(transactions: &[Transaction]) -> Vec<PayCycle> {
    let mut payee_groups: HashMap<String, Vec<&Transaction>> = HashMap::new();

    for tx in transactions {
        let key = normalise_payee(&tx.raw_text);
        if !key.is_empty() {
            payee_groups.entry(key).or_default().push(tx);
        }
    }

    let mut cycles = Vec::new();
    for (payee, txns) in &payee_groups {
        if txns.len() < 2 {
            continue;
        }
        let is_income = txns.iter().filter(|t| t.debit.is_some()).count()
            > txns.iter().filter(|t| t.credit.is_some()).count();

        // Compute typical amount (median of the relevant field)
        let mut amounts: Vec<Decimal> = txns
            .iter()
            .filter_map(|t| if is_income { t.debit } else { t.credit })
            .collect();
        amounts.sort();
        let typical_amount = if amounts.is_empty() {
            None
        } else {
            Some(amounts[amounts.len() / 2])
        };

        cycles.push(PayCycle {
            payee: payee.clone(),
            frequency_days: None, // TODO: parse dates and compute gaps
            typical_amount,
            transaction_count: txns.len(),
            is_income,
        });
    }
    cycles
}

// ─────────────────────────────────────────────────────────────────────────────
// Intent parser
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a natural language instruction into a `FinancialIntent`.
///
/// This is a deterministic parser — no LLM is called here. It uses pattern
/// matching against a comprehensive set of financial vocabulary.
pub fn parse_financial_intent(instruction: &str) -> FinancialIntent {
    let lower = instruction.to_lowercase();
    let lower = lower.trim();

    // ── Scale all income/expenses ─────────────────────────────────────────────
    // "increase all income by 10%" / "reduce all expenses by 5%"
    if let Some(intent) = try_parse_scale_all(lower, instruction) {
        return intent;
    }

    // ── Scale income ──────────────────────────────────────────────────────────
    // "double Maree's pay" / "triple my salary" / "increase pay by 50%"
    if let Some(intent) = try_parse_scale_income(lower, instruction) {
        return intent;
    }

    // ── Scale expense ─────────────────────────────────────────────────────────
    // "halve my rent" / "reduce Woolworths by $50" / "cut expenses by 20%"
    if let Some(intent) = try_parse_scale_expense(lower, instruction) {
        return intent;
    }

    // ── Set amount ────────────────────────────────────────────────────────────
    // "set Maree's pay to $3000" / "change rent to $1500"
    if let Some(intent) = try_parse_set_amount(lower, instruction) {
        return intent;
    }

    // ── Rename payee ──────────────────────────────────────────────────────────
    // "change Woolworths to Coles" / "rename salary to wages"
    if let Some(intent) = try_parse_rename(lower, instruction) {
        return intent;
    }

    // ── Remove transactions ───────────────────────────────────────────────────
    // "remove all Uber transactions" / "delete Netflix payments"
    if let Some(intent) = try_parse_remove(lower, instruction) {
        return intent;
    }

    FinancialIntent::Unknown
}

// ─────────────────────────────────────────────────────────────────────────────
// Sub-parsers
// ─────────────────────────────────────────────────────────────────────────────

fn try_parse_scale_income(lower: &str, original: &str) -> Option<FinancialIntent> {
    // Income vocabulary: pay, salary, wage, income, earnings, deposit, credit
    let income_words = [
        "pay", "salary", "wage", "wages", "income", "earnings", "deposit",
        "payroll", "remuneration", "stipend", "allowance",
    ];
    let has_income_word = income_words.iter().any(|w| lower.contains(w));
    if !has_income_word {
        return None;
    }

    let factor = extract_scale_factor(lower)?;
    let payee = extract_payee_name(lower, original);

    Some(FinancialIntent::ScaleIncome { payee, factor })
}

fn try_parse_scale_expense(lower: &str, original: &str) -> Option<FinancialIntent> {
    // Expense vocabulary: rent, mortgage, bill, expense, payment, fee, cost
    let expense_words = [
        "rent", "mortgage", "bill", "expense", "expenses", "payment",
        "fee", "cost", "subscription", "insurance", "utilities",
    ];
    // Must also have a scaling word
    let has_expense_word = expense_words.iter().any(|w| lower.contains(w));
    if !has_expense_word {
        return None;
    }

    // Check for scaling intent (not just any sentence mentioning rent)
    let scaling_words = [
        "double", "triple", "halve", "half", "increase", "decrease",
        "reduce", "cut", "multiply", "scale", "boost", "lower", "raise",
    ];
    let has_scale = scaling_words.iter().any(|w| lower.contains(w))
        || lower.contains('%')
        || lower.contains("x2") || lower.contains("x3") || lower.contains("x0.");

    if !has_scale {
        return None;
    }

    let factor = extract_scale_factor(lower)?;
    let payee = extract_payee_name(lower, original);

    Some(FinancialIntent::ScaleExpense { payee, factor })
}

fn try_parse_set_amount(lower: &str, original: &str) -> Option<FinancialIntent> {
    // "set X to $Y" / "change X to $Y" / "make X $Y"
    let set_words = ["set", "change", "make", "update", "fix"];
    let has_set = set_words.iter().any(|w| lower.starts_with(w));
    if !has_set {
        return None;
    }
    let amount = extract_dollar_amount(lower)?;
    let payee = extract_payee_name(lower, original);

    // Determine if income or expense from context
    let income_words = ["pay", "salary", "wage", "income", "deposit", "earnings"];
    let is_income = income_words.iter().any(|w| lower.contains(w));

    Some(FinancialIntent::SetAmount { payee, amount, is_income })
}

fn try_parse_rename(lower: &str, original: &str) -> Option<FinancialIntent> {
    // "change X to Y" / "rename X to Y" / "replace X with Y"
    let rename_words = ["rename", "replace"];
    let has_rename = rename_words.iter().any(|w| lower.starts_with(w));

    // Also handle "change X to Y" but only when NOT a set-amount pattern
    let has_change_to = lower.starts_with("change") && lower.contains(" to ")
        && !lower.contains('$') && !lower.contains('%');

    if !has_rename && !has_change_to {
        return None;
    }

    // Extract "from" and "to" names
    let separator = if lower.contains(" to ") { " to " } else { " with " };
    let verb_end = ["rename ", "replace ", "change "]
        .iter()
        .find_map(|v| lower.find(v).map(|p| p + v.len()))?;

    let rest = &original[verb_end..];
    let sep_pos = lower[verb_end..].find(separator)?;
    let from = rest[..sep_pos].trim().trim_matches('\'').trim_matches('"').to_string();
    let to_start = sep_pos + separator.len();
    let to = rest[to_start..].trim().trim_matches('\'').trim_matches('"').to_string();

    if from.is_empty() || to.is_empty() {
        return None;
    }

    Some(FinancialIntent::RenamePayee { from, to })
}

fn try_parse_remove(lower: &str, _original: &str) -> Option<FinancialIntent> {
    let remove_words = ["remove", "delete", "drop", "exclude", "strip"];
    let has_remove = remove_words.iter().any(|w| lower.starts_with(w));
    if !has_remove {
        return None;
    }

    // Extract what to remove — everything after "all", "the", "every"
    let noise = ["all ", "the ", "every ", "any "];
    let mut rest = lower;
    for verb in &remove_words {
        if rest.starts_with(verb) {
            rest = rest[verb.len()..].trim();
            break;
        }
    }
    for n in &noise {
        if rest.starts_with(n) {
            rest = rest[n.len()..].trim();
            break;
        }
    }
    // Strip trailing "transactions", "payments", "entries"
    let strip = [" transactions", " payments", " entries", " transaction", " payment", " entry"];
    for s in &strip {
        if rest.ends_with(s) {
            rest = &rest[..rest.len() - s.len()];
            break;
        }
    }
    let payee = rest.trim().to_string();
    if payee.is_empty() {
        return None;
    }

    Some(FinancialIntent::RemoveTransactions { payee })
}

fn try_parse_scale_all(lower: &str, _original: &str) -> Option<FinancialIntent> {
    // "increase all income by 10%" / "reduce all expenses by 5%"
    let has_all = lower.contains("all income") || lower.contains("all salary")
        || lower.contains("all wages") || lower.contains("all deposits");
    let has_all_exp = lower.contains("all expense") || lower.contains("all payments")
        || lower.contains("all spending") || lower.contains("all costs");

    if !has_all && !has_all_exp {
        return None;
    }

    let factor = extract_scale_factor(lower)?;

    if has_all {
        Some(FinancialIntent::ScaleAllIncome { factor })
    } else {
        Some(FinancialIntent::ScaleAllExpenses { factor })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scale factor extraction
// ─────────────────────────────────────────────────────────────────────────────

/// Extract a multiplicative scale factor from a natural language string.
///
/// Examples:
/// - "double" → 2.0
/// - "triple" → 3.0
/// - "halve" / "half" → 0.5
/// - "increase by 50%" → 1.5
/// - "decrease by 20%" → 0.8
/// - "reduce by $500" → None (use SetAmount instead)
/// - "x2" → 2.0
/// - "2x" → 2.0
/// - "multiply by 3" → 3.0
pub fn extract_scale_factor(lower: &str) -> Option<Decimal> {
    // Named multipliers
    if lower.contains("double") || lower.contains("2x") || lower.contains("x2") {
        return Some(Decimal::from(2));
    }
    if lower.contains("triple") || lower.contains("3x") || lower.contains("x3") {
        return Some(Decimal::from(3));
    }
    if lower.contains("quadruple") || lower.contains("4x") || lower.contains("x4") {
        return Some(Decimal::from(4));
    }
    if lower.contains("halve") || lower.contains("half") {
        return Some(Decimal::new(5, 1)); // 0.5
    }

    // Percentage-based: "increase by 50%" → 1.5, "decrease by 20%" → 0.8
    if let Some(pct) = extract_percentage(lower) {
        let pct_dec = Decimal::from_f64(pct)?;
        let factor = if lower.contains("increase")
            || lower.contains("raise")
            || lower.contains("boost")
            || lower.contains("more")
        {
            Decimal::ONE + pct_dec / Decimal::from(100)
        } else if lower.contains("decrease")
            || lower.contains("reduce")
            || lower.contains("cut")
            || lower.contains("lower")
            || lower.contains("less")
        {
            Decimal::ONE - pct_dec / Decimal::from(100)
        } else {
            // Just a percentage without direction → treat as absolute factor
            pct_dec / Decimal::from(100)
        };
        return Some(factor);
    }

    // "multiply by N"
    if let Some(pos) = lower.find("multiply by ") {
        let rest = &lower[pos + "multiply by ".len()..];
        if let Some(n) = parse_leading_number(rest) {
            return Decimal::from_f64(n);
        }
    }

    // "by a factor of N"
    if let Some(pos) = lower.find("factor of ") {
        let rest = &lower[pos + "factor of ".len()..];
        if let Some(n) = parse_leading_number(rest) {
            return Decimal::from_f64(n);
        }
    }

    None
}

fn extract_percentage(s: &str) -> Option<f64> {
    if let Some(pos) = s.find('%') {
        // Walk backwards to find the number
        let before = &s[..pos];
        let num_start = before.rfind(|c: char| !c.is_ascii_digit() && c != '.' && c != ' ')
            .map(|p| p + 1)
            .unwrap_or(0);
        let num_str = before[num_start..].trim();
        return num_str.parse::<f64>().ok();
    }
    None
}

fn parse_leading_number(s: &str) -> Option<f64> {
    let s = s.trim();
    let end = s.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(s.len());
    s[..end].parse::<f64>().ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// Payee name extraction
// ─────────────────────────────────────────────────────────────────────────────

/// Extract a payee name from a natural language instruction.
///
/// Handles possessive forms ("Maree's"), "from X", "for X", "to X".
pub fn extract_payee_name(lower: &str, original: &str) -> Option<String> {
    // Possessive: "Maree's pay" → "Maree"
    if let Some(pos) = lower.find("'s ") {
        // Walk backwards to find the start of the name
        let before = &lower[..pos];
        let name_start = before.rfind(|c: char| [' ', '\t'].contains(&c))
            .map(|p| p + 1)
            .unwrap_or(0);
        let name_lower = &lower[name_start..pos];
        // Preserve original casing
        let name_orig = &original[name_start..pos];
        if !name_lower.is_empty() && !is_stop_word(name_lower) {
            return Some(name_orig.trim().to_string());
        }
    }

    // "from X" / "for X" / "by X"
    for prep in &["from ", "for ", "by "] {
        if let Some(pos) = lower.find(prep) {
            let rest = &lower[pos + prep.len()..];
            let orig_rest = &original[pos + prep.len()..];
            let end = rest.find(|c: char| [' ', '\'', ','].contains(&c)).unwrap_or(rest.len());
            let name = orig_rest[..end].trim().to_string();
            if !name.is_empty() && !is_stop_word(&name.to_lowercase()) {
                return Some(name);
            }
        }
    }

    None
}

fn is_stop_word(s: &str) -> bool {
    matches!(
        s,
        "my" | "the" | "a" | "an" | "all" | "this" | "that" | "their"
            | "her" | "his" | "its" | "our" | "your" | "every" | "each"
    )
}

fn extract_dollar_amount(s: &str) -> Option<Decimal> {
    if let Some(pos) = s.find('$') {
        let rest = &s[pos + 1..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != ',' && c != '.')
            .unwrap_or(rest.len());
        let num_str = rest[..end].replace(',', "");
        return Decimal::from_str(&num_str).ok();
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Payee matching
// ─────────────────────────────────────────────────────────────────────────────

/// Normalise a transaction description to a canonical payee key.
/// Strips transaction IDs, dates, amounts, and common bank noise.
pub fn normalise_payee(raw_text: &str) -> String {
    let s = raw_text.to_lowercase();
    // Strip common AU bank noise patterns
    let noise_patterns = [
        // BSB/account numbers
        r"bsb\s*\d+",
        // Transaction reference numbers
        r"ref\s*[\w\d]+",
        // Date stamps embedded in description
        r"\d{2}/\d{2}",
        // Card numbers
        r"card\s*\d+",
        // "value date" suffix
        r"value date.*",
    ];
    let mut result = s.clone();
    for pattern in &noise_patterns {
        // Simple substring removal for common patterns
        if let Some(pos) = result.find(pattern.split('\\').next().unwrap_or("")) {
            result = result[..pos].trim().to_string();
        }
    }
    // Trim common prefixes
    let prefixes = ["direct credit ", "direct debit ", "osko payment ", "bpay ", "eftpos "];
    for prefix in &prefixes {
        if result.starts_with(prefix) {
            result = result[prefix.len()..].trim().to_string();
        }
    }
    result.trim().to_string()
}

/// Find all transactions that match a payee name using fuzzy matching.
///
/// Matching strategy (in order of priority):
/// 1. Exact match (case-insensitive)
/// 2. Contains match
/// 3. First-name match (for personal names like "Maree")
pub fn find_matching_transactions(
    transactions: &[Transaction],
    payee: &str,
) -> Vec<usize> {
    let payee_lower = payee.to_lowercase();
    let mut matches = Vec::new();

    for (idx, tx) in transactions.iter().enumerate() {
        let desc_lower = tx.raw_text.to_lowercase();
        let normalised = normalise_payee(&tx.raw_text);

        // Exact match on normalised payee
        if normalised == payee_lower {
            matches.push(idx);
            continue;
        }

        // Contains match
        if desc_lower.contains(&payee_lower) {
            matches.push(idx);
            continue;
        }

        // Word boundary match (for first names)
        let words: Vec<&str> = desc_lower.split_whitespace().collect();
        if words.contains(&payee_lower.as_str()) {
            matches.push(idx);
        }
    }

    matches
}

// ─────────────────────────────────────────────────────────────────────────────
// Balance cascade
// ─────────────────────────────────────────────────────────────────────────────

/// Recalculate all running balances from a given index onwards.
///
/// Uses the running balance of the row *before* `from_index` as the opening
/// balance for the cascade. If `from_index == 0`, uses `Decimal::ZERO`.
pub fn cascade_running_balances(transactions: &mut [Transaction], from_index: usize) {
    if transactions.is_empty() || from_index >= transactions.len() {
        return;
    }

    // Determine the opening balance for the cascade
    let opening = if from_index == 0 {
        // Try to infer from the first transaction's balance minus its delta
        transactions[0]
            .running_balance
            .map(|b| {
                let delta = transactions[0].net_delta();
                b - delta
            })
            .unwrap_or(Decimal::ZERO)
    } else {
        transactions[from_index - 1]
            .running_balance
            .unwrap_or(Decimal::ZERO)
    };

    let mut balance = opening;
    for tx in transactions[from_index..].iter_mut() {
        balance = (balance + tx.net_delta()).round_dp(2);
        tx.running_balance = Some(balance);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Main engine: apply a FinancialIntent to a transaction list
// ─────────────────────────────────────────────────────────────────────────────

/// Result of applying a `FinancialIntent` to a transaction list.
#[derive(Debug)]
pub struct FinancialEditResult {
    /// The updated transaction list with all changes applied and balances cascaded.
    pub transactions: Vec<Transaction>,
    /// Human-readable summary of what was changed.
    pub summary: String,
    /// Number of transactions that were modified.
    pub rows_changed: usize,
    /// Whether the balance was recascaded.
    pub balance_cascaded: bool,
}

/// Apply a `FinancialIntent` to a list of transactions.
///
/// This is the main entry point for the financial NLP engine. It:
/// 1. Resolves entities (finds matching transactions)
/// 2. Applies the math deterministically using `Decimal`
/// 3. Cascades the running balance from the first edited row
/// 4. Returns a detailed result with a human-readable summary
pub fn apply_financial_intent(
    intent: FinancialIntent,
    transactions: Vec<Transaction>,
) -> FinancialEditResult {
    match intent {
        FinancialIntent::ScaleIncome { payee, factor } => {
            apply_scale_income(transactions, payee, factor)
        }
        FinancialIntent::ScaleExpense { payee, factor } => {
            apply_scale_expense(transactions, payee, factor)
        }
        FinancialIntent::SetAmount { payee, amount, is_income } => {
            apply_set_amount(transactions, payee, amount, is_income)
        }
        FinancialIntent::RenamePayee { from, to } => {
            apply_rename_payee(transactions, &from, &to)
        }
        FinancialIntent::RemoveTransactions { payee } => {
            apply_remove_transactions(transactions, &payee)
        }
        FinancialIntent::ScaleAllIncome { factor } => {
            apply_scale_all_income(transactions, factor)
        }
        FinancialIntent::ScaleAllExpenses { factor } => {
            apply_scale_all_expenses(transactions, factor)
        }
        FinancialIntent::AddTransaction { date, description, amount, is_income } => {
            apply_add_transaction(transactions, date, description, amount, is_income)
        }
        FinancialIntent::Unknown => FinancialEditResult {
            transactions,
            summary: "Intent not recognised by financial NLP engine — routing to AI provider.".to_string(),
            rows_changed: 0,
            balance_cascaded: false,
        },
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Intent handlers
// ─────────────────────────────────────────────────────────────────────────────

fn apply_scale_income(
    mut transactions: Vec<Transaction>,
    payee: Option<String>,
    factor: Decimal,
) -> FinancialEditResult {
    let indices = match &payee {
        Some(p) => find_matching_transactions(&transactions, p),
        None => (0..transactions.len()).collect(),
    };

    // Filter to income-only transactions (debit field set)
    let income_indices: Vec<usize> = indices
        .into_iter()
        .filter(|&i| transactions[i].debit.is_some())
        .collect();

    if income_indices.is_empty() {
        let payee_desc = payee.as_deref().unwrap_or("any payee");
        return FinancialEditResult {
            transactions,
            summary: format!(
                "No income transactions found for '{}'. Check the payee name matches the statement.",
                payee_desc
            ),
            rows_changed: 0,
            balance_cascaded: false,
        };
    }

    let first_changed = *income_indices.iter().min().unwrap();
    let count = income_indices.len();
    let payee_desc = payee.as_deref().unwrap_or("all payees");

    for i in &income_indices {
        if let Some(d) = transactions[*i].debit {
            transactions[*i].debit = Some((d * factor).round_dp(2));
        }
    }

    cascade_running_balances(&mut transactions, first_changed);

    FinancialEditResult {
        transactions,
        summary: format!(
            "Scaled income for '{}' by ×{} across {} transaction(s). Running balances recalculated from row {}.",
            payee_desc, factor, count, first_changed + 1
        ),
        rows_changed: count,
        balance_cascaded: true,
    }
}

fn apply_scale_expense(
    mut transactions: Vec<Transaction>,
    payee: Option<String>,
    factor: Decimal,
) -> FinancialEditResult {
    let indices = match &payee {
        Some(p) => find_matching_transactions(&transactions, p),
        None => (0..transactions.len()).collect(),
    };

    // Filter to expense-only transactions (credit field set)
    let expense_indices: Vec<usize> = indices
        .into_iter()
        .filter(|&i| transactions[i].credit.is_some())
        .collect();

    if expense_indices.is_empty() {
        let payee_desc = payee.as_deref().unwrap_or("any payee");
        return FinancialEditResult {
            transactions,
            summary: format!(
                "No expense transactions found for '{}'. Check the payee name matches the statement.",
                payee_desc
            ),
            rows_changed: 0,
            balance_cascaded: false,
        };
    }

    let first_changed = *expense_indices.iter().min().unwrap();
    let count = expense_indices.len();
    let payee_desc = payee.as_deref().unwrap_or("all payees");

    for i in &expense_indices {
        if let Some(c) = transactions[*i].credit {
            transactions[*i].credit = Some((c * factor).round_dp(2));
        }
    }

    cascade_running_balances(&mut transactions, first_changed);

    FinancialEditResult {
        transactions,
        summary: format!(
            "Scaled expenses for '{}' by ×{} across {} transaction(s). Running balances recalculated from row {}.",
            payee_desc, factor, count, first_changed + 1
        ),
        rows_changed: count,
        balance_cascaded: true,
    }
}

fn apply_set_amount(
    mut transactions: Vec<Transaction>,
    payee: Option<String>,
    amount: Decimal,
    is_income: bool,
) -> FinancialEditResult {
    let indices = match &payee {
        Some(p) => find_matching_transactions(&transactions, p),
        None => (0..transactions.len()).collect(),
    };

    let relevant_indices: Vec<usize> = indices
        .into_iter()
        .filter(|&i| {
            if is_income {
                transactions[i].debit.is_some()
            } else {
                transactions[i].credit.is_some()
            }
        })
        .collect();

    if relevant_indices.is_empty() {
        return FinancialEditResult {
            transactions,
            summary: "No matching transactions found.".to_string(),
            rows_changed: 0,
            balance_cascaded: false,
        };
    }

    let first_changed = *relevant_indices.iter().min().unwrap();
    let count = relevant_indices.len();
    let payee_desc = payee.as_deref().unwrap_or("all payees");

    for i in &relevant_indices {
        if is_income {
            transactions[*i].debit = Some(amount);
        } else {
            transactions[*i].credit = Some(amount);
        }
    }

    cascade_running_balances(&mut transactions, first_changed);

    FinancialEditResult {
        transactions,
        summary: format!(
            "Set {} amount for '{}' to ${} across {} transaction(s). Running balances recalculated.",
            if is_income { "income" } else { "expense" },
            payee_desc, amount, count
        ),
        rows_changed: count,
        balance_cascaded: true,
    }
}

fn apply_rename_payee(
    mut transactions: Vec<Transaction>,
    from: &str,
    to: &str,
) -> FinancialEditResult {
    let indices = find_matching_transactions(&transactions, from);

    if indices.is_empty() {
        return FinancialEditResult {
            transactions,
            summary: format!("No transactions found matching '{}'.", from),
            rows_changed: 0,
            balance_cascaded: false,
        };
    }

    let count = indices.len();
    for i in &indices {
        // Case-insensitive replace in raw_text
        let new_text = replace_case_insensitive(&transactions[*i].raw_text, from, to);
        transactions[*i].raw_text = new_text;
    }

    FinancialEditResult {
        transactions,
        summary: format!(
            "Renamed '{}' to '{}' in {} transaction description(s). No amounts changed.",
            from, to, count
        ),
        rows_changed: count,
        balance_cascaded: false,
    }
}

fn apply_remove_transactions(
    transactions: Vec<Transaction>,
    payee: &str,
) -> FinancialEditResult {
    let indices_to_remove: std::collections::HashSet<usize> =
        find_matching_transactions(&transactions, payee)
            .into_iter()
            .collect();

    if indices_to_remove.is_empty() {
        return FinancialEditResult {
            transactions,
            summary: format!("No transactions found matching '{}'.", payee),
            rows_changed: 0,
            balance_cascaded: false,
        };
    }

    let count = indices_to_remove.len();
    let first_removed = *indices_to_remove.iter().min().unwrap();
    let mut new_txns: Vec<Transaction> = transactions
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !indices_to_remove.contains(i))
        .map(|(_, t)| t)
        .collect();

    let cascade_from = first_removed.min(new_txns.len().saturating_sub(1));
    if !new_txns.is_empty() {
        cascade_running_balances(&mut new_txns, cascade_from);
    }

    FinancialEditResult {
        transactions: new_txns,
        summary: format!(
            "Removed {} transaction(s) matching '{}'. Running balances recalculated.",
            count, payee
        ),
        rows_changed: count,
        balance_cascaded: true,
    }
}

fn apply_scale_all_income(
    mut transactions: Vec<Transaction>,
    factor: Decimal,
) -> FinancialEditResult {
    let count = transactions.iter().filter(|t| t.debit.is_some()).count();
    for tx in transactions.iter_mut() {
        if let Some(d) = tx.debit {
            tx.debit = Some((d * factor).round_dp(2));
        }
    }
    cascade_running_balances(&mut transactions, 0);
    FinancialEditResult {
        transactions,
        summary: format!(
            "Scaled all {} income transactions by ×{}. Running balances recalculated.",
            count, factor
        ),
        rows_changed: count,
        balance_cascaded: true,
    }
}

fn apply_scale_all_expenses(
    mut transactions: Vec<Transaction>,
    factor: Decimal,
) -> FinancialEditResult {
    let count = transactions.iter().filter(|t| t.credit.is_some()).count();
    for tx in transactions.iter_mut() {
        if let Some(c) = tx.credit {
            tx.credit = Some((c * factor).round_dp(2));
        }
    }
    cascade_running_balances(&mut transactions, 0);
    FinancialEditResult {
        transactions,
        summary: format!(
            "Scaled all {} expense transactions by ×{}. Running balances recalculated.",
            count, factor
        ),
        rows_changed: count,
        balance_cascaded: true,
    }
}

fn apply_add_transaction(
    mut transactions: Vec<Transaction>,
    date: String,
    description: String,
    amount: Decimal,
    is_income: bool,
) -> FinancialEditResult {
    let page = transactions.last().map(|t| t.page).unwrap_or(1);
    let line = transactions.last().map(|t| t.line_on_page + 1).unwrap_or(1);
    let new_tx = Transaction {
        page,
        line_on_page: line,
        date,
        raw_text: description,
        debit: if is_income { Some(amount) } else { None },
        credit: if !is_income { Some(amount) } else { None },
        running_balance: None,
        bbox: None,
        field_bboxes: Default::default(),
        provenance: crate::engine::model::Provenance::Computed,
        category: None,
        canonical: crate::engine::model::CanonicalMetadata::default(),
    };
    transactions.push(new_tx);
    let last = transactions.len() - 1;
    cascade_running_balances(&mut transactions, last);
    FinancialEditResult {
        transactions,
        summary: format!(
            "Added new {} transaction of ${}. Running balance updated.",
            if is_income { "income" } else { "expense" },
            amount
        ),
        rows_changed: 1,
        balance_cascaded: true,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn replace_case_insensitive(s: &str, from: &str, to: &str) -> String {
    let lower = s.to_lowercase();
    let from_lower = from.to_lowercase();
    if let Some(pos) = lower.find(&from_lower) {
        format!("{}{}{}", &s[..pos], to, &s[pos + from.len()..])
    } else {
        s.to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public convenience: parse + apply in one call
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a natural language instruction and apply it to the transaction list.
///
/// Returns `None` if the intent is `Unknown` (caller should fall back to LLM).
pub fn parse_and_apply(
    instruction: &str,
    transactions: Vec<Transaction>,
) -> Option<FinancialEditResult> {
    let intent = parse_financial_intent(instruction);
    if matches!(intent, FinancialIntent::Unknown) {
        return None;
    }
    Some(apply_financial_intent(intent, transactions))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_tx(page: usize, line: usize, desc: &str, debit: Option<f64>, credit: Option<f64>, balance: f64) -> Transaction {
        Transaction {
            page,
            line_on_page: line,
            date: format!("2024-01-{:02}", line),
            raw_text: desc.to_string(),
            debit: debit.map(|d| Decimal::from_f64(d).unwrap()),
            credit: credit.map(|c| Decimal::from_f64(c).unwrap()),
            running_balance: Some(Decimal::from_f64(balance).unwrap()),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: crate::engine::model::Provenance::Computed,
            category: None,
            canonical: crate::engine::model::CanonicalMetadata::default(),
        }
    }

    fn sample_transactions() -> Vec<Transaction> {
        vec![
            make_tx(1, 1, "OPENING BALANCE", None, None, 1000.00),
            make_tx(1, 2, "MAREE SMITH PAYROLL", Some(2500.00), None, 3500.00),
            make_tx(1, 3, "WOOLWORTHS SUPERMARKET", None, Some(150.00), 3350.00),
            make_tx(1, 4, "RENT PAYMENT REAL ESTATE", None, Some(1200.00), 2150.00),
            make_tx(1, 5, "MAREE SMITH PAYROLL", Some(2500.00), None, 4650.00),
            make_tx(1, 6, "NETFLIX SUBSCRIPTION", None, Some(22.99), 4627.01),
            make_tx(1, 7, "MAREE SMITH PAYROLL", Some(2500.00), None, 7127.01),
        ]
    }

    // ── The flagship test: "double Maree's pay" ───────────────────────────────
    #[test]
    fn test_double_maree_pay() {
        let txns = sample_transactions();
        let result = parse_and_apply("double Maree's pay", txns).expect("should parse");

        assert_eq!(result.rows_changed, 3, "should change all 3 Maree payroll rows");
        assert!(result.balance_cascaded, "balance should be cascaded");

        // Each pay should now be $5000
        let maree_rows: Vec<_> = result.transactions.iter()
            .filter(|t| t.raw_text.to_lowercase().contains("maree"))
            .collect();
        assert_eq!(maree_rows.len(), 3);
        for row in &maree_rows {
            assert_eq!(row.debit, Some(dec!(5000.00)), "each pay should be doubled to $5000");
        }

        // Verify balance cascade is mathematically correct
        let mut expected_balance = dec!(1000.00); // opening
        for tx in &result.transactions[1..] { // skip opening row
            expected_balance = (expected_balance + tx.net_delta()).round_dp(2);
            assert_eq!(
                tx.running_balance, Some(expected_balance),
                "balance cascade incorrect at row: {}", tx.raw_text
            );
        }
    }

    // ── Scale factor parsing ──────────────────────────────────────────────────
    #[test]
    fn test_extract_scale_factor_double() {
        assert_eq!(extract_scale_factor("double the amount"), Some(dec!(2)));
    }

    #[test]
    fn test_extract_scale_factor_triple() {
        assert_eq!(extract_scale_factor("triple my salary"), Some(dec!(3)));
    }

    #[test]
    fn test_extract_scale_factor_halve() {
        assert_eq!(extract_scale_factor("halve the rent"), Some(dec!(0.5)));
    }

    #[test]
    fn test_extract_scale_factor_percent_increase() {
        let f = extract_scale_factor("increase by 50%").unwrap();
        assert!((f - dec!(1.5)).abs() < dec!(0.001));
    }

    #[test]
    fn test_extract_scale_factor_percent_decrease() {
        let f = extract_scale_factor("reduce by 20%").unwrap();
        assert!((f - dec!(0.8)).abs() < dec!(0.001));
    }

    #[test]
    fn test_extract_scale_factor_x2() {
        assert_eq!(extract_scale_factor("x2 the salary"), Some(dec!(2)));
    }

    // ── Payee extraction ──────────────────────────────────────────────────────
    #[test]
    fn test_extract_payee_possessive() {
        let p = extract_payee_name("double maree's pay", "double Maree's pay");
        assert_eq!(p, Some("Maree".to_string()));
    }

    #[test]
    fn test_extract_payee_from_prep() {
        let p = extract_payee_name("increase income from woolworths", "increase income from Woolworths");
        assert_eq!(p, Some("Woolworths".to_string()));
    }

    #[test]
    fn test_extract_payee_stop_word_ignored() {
        let p = extract_payee_name("double my pay", "double my pay");
        assert_eq!(p, None, "stop word 'my' should not be returned as a payee");
    }

    // ── Intent parsing ────────────────────────────────────────────────────────
    #[test]
    fn test_parse_scale_income_intent() {
        let intent = parse_financial_intent("double Maree's pay");
        assert!(matches!(intent, FinancialIntent::ScaleIncome { .. }));
        if let FinancialIntent::ScaleIncome { payee, factor } = intent {
            assert_eq!(payee, Some("Maree".to_string()));
            assert_eq!(factor, dec!(2));
        }
    }

    #[test]
    fn test_parse_rename_intent() {
        let intent = parse_financial_intent("rename Woolworths to Coles");
        assert!(matches!(intent, FinancialIntent::RenamePayee { .. }));
        if let FinancialIntent::RenamePayee { from, to } = intent {
            assert_eq!(from, "Woolworths");
            assert_eq!(to, "Coles");
        }
    }

    #[test]
    fn test_parse_remove_intent() {
        let intent = parse_financial_intent("remove all Netflix transactions");
        assert!(matches!(intent, FinancialIntent::RemoveTransactions { .. }));
        if let FinancialIntent::RemoveTransactions { payee } = intent {
            assert_eq!(payee, "netflix");
        }
    }

    #[test]
    fn test_parse_set_amount_intent() {
        let intent = parse_financial_intent("set Maree's pay to $3000");
        assert!(matches!(intent, FinancialIntent::SetAmount { .. }));
        if let FinancialIntent::SetAmount { payee, amount, is_income } = intent {
            assert_eq!(payee, Some("Maree".to_string()));
            assert_eq!(amount, dec!(3000));
            assert!(is_income);
        }
    }

    #[test]
    fn test_parse_unknown_falls_through() {
        let intent = parse_financial_intent("what is the weather today");
        assert!(matches!(intent, FinancialIntent::Unknown));
    }

    // ── Payee matching ────────────────────────────────────────────────────────
    #[test]
    fn test_find_matching_transactions_partial() {
        let txns = sample_transactions();
        let matches = find_matching_transactions(&txns, "Maree");
        assert_eq!(matches.len(), 3, "should find all 3 Maree payroll rows");
    }

    #[test]
    fn test_find_matching_transactions_no_match() {
        let txns = sample_transactions();
        let matches = find_matching_transactions(&txns, "Qantas");
        assert!(matches.is_empty());
    }

    // ── Balance cascade ───────────────────────────────────────────────────────
    #[test]
    fn test_cascade_running_balances_from_zero() {
        let mut txns = vec![
            make_tx(1, 1, "Pay", Some(1000.0), None, 1000.0),
            make_tx(1, 2, "Rent", None, Some(500.0), 0.0),
            make_tx(1, 3, "Groceries", None, Some(100.0), 0.0),
        ];
        // Assume opening balance of 0
        cascade_running_balances(&mut txns, 0);
        assert_eq!(txns[0].running_balance, Some(dec!(1000.00)));
        assert_eq!(txns[1].running_balance, Some(dec!(500.00)));
        assert_eq!(txns[2].running_balance, Some(dec!(400.00)));
    }

    #[test]
    fn test_cascade_running_balances_from_middle() {
        let mut txns = sample_transactions();
        // Manually change the second Maree pay to $5000
        txns[4].debit = Some(dec!(5000.00));
        // Cascade from row 4 onwards
        cascade_running_balances(&mut txns, 4);
        // Row 3 balance should be unchanged ($2150)
        assert_eq!(txns[3].running_balance, Some(dec!(2150.00)));
        // Row 4 should be $2150 + $5000 = $7150
        assert_eq!(txns[4].running_balance, Some(dec!(7150.00)));
    }

    // ── Remove transactions ───────────────────────────────────────────────────
    #[test]
    fn test_remove_netflix() {
        let txns = sample_transactions();
        let result = parse_and_apply("remove all Netflix transactions", txns).expect("should parse");
        assert_eq!(result.rows_changed, 1);
        assert!(!result.transactions.iter().any(|t| t.raw_text.to_lowercase().contains("netflix")));
    }

    // ── Rename payee ──────────────────────────────────────────────────────────
    #[test]
    fn test_rename_woolworths_to_coles() {
        let txns = sample_transactions();
        let result = parse_and_apply("rename Woolworths to Coles", txns).expect("should parse");
        assert_eq!(result.rows_changed, 1);
        assert!(result.transactions.iter().any(|t| t.raw_text.contains("Coles")));
        assert!(!result.transactions.iter().any(|t| t.raw_text.to_lowercase().contains("woolworths")));
    }

    // ── Scale all income ──────────────────────────────────────────────────────
    #[test]
    fn test_scale_all_income_10_percent() {
        let txns = sample_transactions();
        let result = parse_and_apply("increase all income by 10%", txns).expect("should parse");
        // 3 Maree payroll rows, each $2500 → $2750
        let maree_rows: Vec<_> = result.transactions.iter()
            .filter(|t| t.raw_text.to_lowercase().contains("maree"))
            .collect();
        for row in &maree_rows {
            assert_eq!(row.debit, Some(dec!(2750.00)));
        }
    }
}
