use crate::ai::document_ai::BankStatement;
use crate::engine::model::Transaction;
use rust_decimal::Decimal;

fn geometry_score(transaction: &Transaction) -> usize {
    usize::from(transaction.bbox.is_some())
        + usize::from(transaction.field_bboxes.date.is_some())
        + usize::from(transaction.field_bboxes.description.is_some())
        + usize::from(transaction.field_bboxes.debit.is_some())
        + usize::from(transaction.field_bboxes.credit.is_some())
        + usize::from(transaction.field_bboxes.running_balance.is_some())
}

fn normalized_date(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn action_amount(transaction: &Transaction) -> Option<Decimal> {
    transaction
        .debit
        .or(transaction.credit)
        .map(|amount| amount.abs().round_dp(2))
}

fn rows_match(left: &Transaction, right: &Transaction) -> bool {
    let date_matches = normalized_date(&left.date) == normalized_date(&right.date);
    let action_matches = match (action_amount(left), action_amount(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    };
    let balance_matches = match (left.running_balance, right.running_balance) {
        (Some(left), Some(right)) => left.round_dp(2) == right.round_dp(2),
        _ => true, // Tolerate missing running balance in one of the sources
    };

    // Fallback: If both action and balance match, but date differs slightly due to OCR, still consider it a match
    if action_matches && balance_matches {
        return true;
    }

    action_matches && balance_matches && date_matches
}

/// Copy exact PDF row/field geometry from parser donors into a coherent
/// semantic ledger without adding or removing transactions.
pub fn enrich_statement_geometry(statement: &mut BankStatement, donors: &[BankStatement]) -> usize {
    if let Some(donor_statement) = donors.iter().find(|donor| {
        !statement.transactions.is_empty()
            && donor.transactions.len() == statement.transactions.len()
            && donor
                .transactions
                .iter()
                .all(|transaction| geometry_score(transaction) > 0)
    }) {
        let mut donor_rows: Vec<&Transaction> = donor_statement.transactions.iter().collect();
        donor_rows.sort_by(|left, right| {
            left.page.cmp(&right.page).then_with(|| {
                let left_y = left.bbox.map(|bbox| bbox[1]).unwrap_or(f32::MAX);
                let right_y = right.bbox.map(|bbox| bbox[1]).unwrap_or(f32::MAX);
                left_y
                    .partial_cmp(&right_y)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(left.line_on_page.cmp(&right.line_on_page))
            })
        });
        for (transaction, donor) in statement.transactions.iter_mut().zip(donor_rows) {
            transaction.page = donor.page;
            transaction.line_on_page = donor.line_on_page;
            transaction.date = donor.date.clone();
            transaction.raw_text = donor.raw_text.clone();
            transaction.bbox = donor.bbox;
            transaction.field_bboxes = donor.field_bboxes.clone();
            transaction.running_balance = donor.running_balance;
            transaction.debit = donor.debit;
            transaction.credit = donor.credit;
            transaction.ensure_canonical_metadata();
        }
        return statement.transactions.len();
    }

    let mut used_donors = std::collections::HashSet::new();
    let mut enriched = 0usize;
    for transaction in &mut statement.transactions {
        let mut best: Option<(usize, usize, &Transaction)> = None;
        let mut best_score = geometry_score(transaction);
        for (statement_index, donor_statement) in donors.iter().enumerate() {
            for (transaction_index, donor) in donor_statement.transactions.iter().enumerate() {
                if used_donors.contains(&(statement_index, transaction_index))
                    || !rows_match(transaction, donor)
                {
                    continue;
                }
                let score = geometry_score(donor);
                if score > best_score {
                    best = Some((statement_index, transaction_index, donor));
                    best_score = score;
                }
            }
        }
        if let Some((statement_index, transaction_index, donor)) = best {
            used_donors.insert((statement_index, transaction_index));
            transaction.page = donor.page;
            transaction.line_on_page = donor.line_on_page;
            transaction.raw_text = donor.raw_text.clone();
            transaction.bbox = donor.bbox;
            transaction.field_bboxes = donor.field_bboxes.clone();
            if transaction.running_balance.is_none() {
                transaction.running_balance = donor.running_balance;
            }
            if transaction.debit.is_none() && transaction.credit.is_none() {
                transaction.debit = donor.debit;
                transaction.credit = donor.credit;
            }
            transaction.ensure_canonical_metadata();
            enriched += 1;
        }
    }
    enriched
}

/// Make `line_on_page` the zero-based transaction ordinal used by transfer
/// mappings, independent of header/text-block row numbers from any parser.
pub fn normalize_statement_row_indices(statement: &mut BankStatement) {
    statement.transactions.sort_by(|left, right| {
        left.page.cmp(&right.page).then_with(|| {
            let left_y = left.bbox.map(|bbox| bbox[1]).unwrap_or(f32::MAX);
            let right_y = right.bbox.map(|bbox| bbox[1]).unwrap_or(f32::MAX);
            left_y
                .partial_cmp(&right_y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.line_on_page.cmp(&right.line_on_page))
        })
    });
    let mut next_by_page = std::collections::HashMap::new();
    for transaction in &mut statement.transactions {
        let next = next_by_page.entry(transaction.page).or_insert(0usize);
        transaction.line_on_page = *next;
        *next += 1;
        transaction.canonical.stable_row_id =
            format!("p{}:r{}", transaction.page, transaction.line_on_page);
    }
}

fn majority_decimal(
    statements: &[BankStatement],
    fallback: Decimal,
    value: impl Fn(&BankStatement) -> Decimal,
) -> Decimal {
    let mut votes = std::collections::HashMap::new();
    for statement in statements {
        *votes.entry(value(statement)).or_insert(0usize) += 1;
    }
    let mut winner = fallback;
    let mut winner_votes = votes.get(&fallback).copied().unwrap_or_default();
    for (candidate, count) in votes {
        if count > winner_votes {
            winner = candidate;
            winner_votes = count;
        }
    }
    winner
}

/// Takes up to 3 `BankStatement`s from different AI/Offline parsers and
/// performs a majority-rule vote to synthesize the most accurate result.
pub fn merge_consensus_statements(statements: Vec<BankStatement>) -> BankStatement {
    if statements.is_empty() {
        return BankStatement {
            total_pages: 0,
            transactions: Vec::new(),
            opening_balance: Decimal::ZERO,
            closing_balance: Decimal::ZERO,
            account_number: None,
            bank_name: None,
        };
    }

    // If only 1 statement, just return it.
    if statements.len() == 1 {
        return statements.into_iter().next().unwrap();
    }

    // Preserve one coherent semantic ledger instead of unioning unmatched rows
    // from two parsers. The prior two-parser union inflated CommBank from 65 to
    // 88 rows and detached transaction semantics from editable geometry.
    let mut primary_index = 0usize;
    for index in 1..statements.len() {
        if statements[index].transactions.len() > statements[primary_index].transactions.len() {
            primary_index = index;
        }
    }
    let primary = &statements[primary_index];
    let majority_opening = majority_decimal(&statements, primary.opening_balance, |statement| {
        statement.opening_balance
    });
    let majority_closing = majority_decimal(&statements, primary.closing_balance, |statement| {
        statement.closing_balance
    });

    let mut final_txs = primary.transactions.clone();
    let donors: Vec<BankStatement> = statements
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != primary_index)
        .map(|(_, statement)| statement.clone())
        .collect();
    let mut merged_statement = BankStatement {
        opening_balance: majority_opening,
        closing_balance: majority_closing,
        transactions: std::mem::take(&mut final_txs),
        account_number: primary.account_number.clone(),
        total_pages: statements
            .iter()
            .map(|statement| statement.total_pages)
            .max()
            .unwrap_or(primary.total_pages),
        bank_name: primary.bank_name.clone(),
    };
    enrich_statement_geometry(&mut merged_statement, &donors);
    normalize_statement_row_indices(&mut merged_statement);

    merged_statement
}
