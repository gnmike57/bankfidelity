# Financial NLP Engine Design

## Goal
Enhance the BankFidelity natural language system to deeply understand financial statement semantics, enabling complex, multi-step intents like "double Maree's pay" without relying purely on brittle regex or slow LLM round-trips for the logic.

## Current State
- `nlp_router.rs` handles high-level intents (Extract, Verify, Balance).
- For `AiEdit` ("Change all January transactions to February"), it sends the instruction and the *entire* JSON array of transactions to Gemini.
- Gemini returns the modified JSON array.
- **Problem:** "Double Maree's pay" requires understanding who "Maree" is (payee detection), what her "pay" is (salary/income detection vs. expense), scaling the amount, and then cascading the running balance. LLMs are notoriously bad at math and balance cascading.

## Proposed Architecture: `FinancialNlpEngine`

We will build a deterministic, domain-aware semantic parser in Rust (`src/engine/financial_nlp.rs`) that intercepts financial edits *before* they hit the LLM, or uses the LLM *only* for entity resolution (e.g., "Which transactions belong to Maree?"), and then applies the math and balance cascade deterministically in Rust.

### 1. Intent Taxonomy
- **ScaleIncome:** "double Maree's pay", "increase my salary by 10%"
- **ScaleExpense:** "halve my rent", "reduce groceries by $50"
- **DateShift:** "move all rent payments to the 1st of the month"
- **RenamePayee:** "change Woolworths to Coles"

### 2. Entity Resolution (The "Who" and "What")
- **Payee Detection:** Match "Maree" against the `raw_text` or `description` field of transactions.
- **Income vs Expense:** "pay" implies a `debit` (money in) transaction. "rent" implies a `credit` (money out) transaction.
- **Pay Cycle:** Group transactions by payee and identify recurring patterns (weekly, fortnightly, monthly).

### 3. Execution Pipeline
1. **Parse Intent:** Identify the action (Scale), target (Maree), field (pay/income), and factor (double/x2).
2. **Resolve Entities:** Filter the `Vec<Transaction>` to find matching rows.
3. **Apply Math:** Deterministically multiply the `debit` field by 2 using `rust_decimal::Decimal`.
4. **Cascade Balance:** Call the existing `crate::engine::balance::balance_statement` or a targeted cascade function to recalculate all `running_balance` fields from the first edited transaction onwards.

### 4. Integration
- `nlp_router.rs` will parse "double Maree's pay" into `NlpCommand::FinancialEdit { intent: ScaleIncome, target: "Maree", factor: 2.0 }`.
- `runtime.rs` will dispatch this to `FinancialNlpEngine::apply()`.
- The engine will modify the transactions and return them via `JobResult::NaturalLanguageEditReady`.
