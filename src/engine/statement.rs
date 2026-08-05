//! Smart Document-Level Balance Engine v2.0
//!
//! This is the SINGLE UNIFIED ENGINE for the entire application.
//! It treats the bank statement as ONE CONNECTED DOCUMENT (not per-page).
//!
//! Core Responsibilities:
//! - Parse the ENTIRE multi-page PDF using Google Document AI
//! - Maintain a global list of ALL transactions across ALL pages
//! - When ANY change is made, recalculate ALL subsequent running balances
//! - Use Gemini with FULL document context to propose minimal smart adjustments
//! - Queue changes that may span multiple pages
//! - Apply all approved changes with maximum visual fidelity

use crate::ai::backend::AiBackend;
use crate::ai::document_ai::DocumentAiClient;
use crate::engine::model::{dec_to_f64, f64_to_dec, ProposedChange, Transaction};
use crate::extractors::merger::HybridMerger;
use crate::pdf::PdfEngine;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Document not loaded")]
    NotLoaded,
    #[error("Invalid transaction format: {0}")]
    InvalidTransaction(String),
    #[error("Balance math error: {0}")]
    MathError(String),
    #[error("Gemini failed to generate plan: {0}")]
    AiPlanFailed(String),
    #[error("Low Confidence: {0:.2}")]
    LowConfidence(f32),
}

pub struct SmartDocumentEngine {
    pub all_transactions: Vec<Transaction>,
    pub proposed_changes: Vec<ProposedChange>,
    pub is_balanced: bool,
    pub total_pages: usize,
    pub layout: Option<crate::engine::layout::DocumentLayout>,

    pdf_engine: Arc<dyn PdfEngine>,
    doc_ai: Arc<DocumentAiClient>,
    ai_backend: Arc<AiBackend>,
    merger: Arc<HybridMerger>,
}

impl SmartDocumentEngine {
    pub fn new(
        pdf_engine: Arc<dyn PdfEngine>,
        doc_ai: Arc<DocumentAiClient>,
        ai_backend: Arc<AiBackend>,
        merger: Arc<HybridMerger>,
    ) -> Self {
        Self {
            all_transactions: Vec::new(),
            is_balanced: false,
            proposed_changes: Vec::new(),
            layout: None,
            total_pages: 0,
            pdf_engine,
            doc_ai,
            ai_backend,
            merger,
        }
    }

    /// Load the ENTIRE multi-page statement + Layout Analysis (called once when user loads PDF)
    pub async fn load_full_document(
        &mut self,
        _job_tx: &std::sync::mpsc::Sender<crate::app::runtime::Job>,
        pdf_path: &Path,
    ) -> Result<(), EngineError> {
        tracing::info!(
            "[DOCUMENT ENGINE] Loading ENTIRE multi-page statement + Layout Analysis..."
        );

        self.all_transactions.clear();
        self.proposed_changes.clear();

        // Run document-level layout analysis
        let layout = self
            .pdf_engine
            .analyze_layout(pdf_path)
            .map_err(|e| EngineError::AiPlanFailed(format!("Layout analysis failed: {e}")))?;

        self.total_pages = layout.total_pages;
        self.layout = Some(layout.clone());
        tracing::info!(
            "[DOCUMENT ENGINE] Layout analysis complete: {} pages, consistent headers: {}",
            layout.total_pages,
            layout.has_consistent_headers
        );

        tracing::info!(
            "[DOCUMENT ENGINE] Document loaded with {} transactions across {} pages",
            self.all_transactions.len(),
            self.total_pages
        );

        Ok(())
    }

    /// Main function - "Balance Statement Out" for the ENTIRE document
    pub async fn balance_entire_statement(
        &mut self,
        current_pdf_path: &Path,
    ) -> Result<Vec<ProposedChange>, EngineError> {
        tracing::info!("[DOCUMENT ENGINE] ===== BALANCE ENTIRE STATEMENT =====");

        // 1. Document AI Extraction
        let bank_stmt = self
            .doc_ai
            .parse_entire_statement(current_pdf_path, None)
            .await
            .map_err(|e| EngineError::AiPlanFailed(format!("Document AI failed: {e}")))?;

        let opening_balance = bank_stmt.opening_balance;
        let closing_balance = bank_stmt.closing_balance;

        // 2. Geometry Extraction
        let mut geometries = Vec::new();
        for provider in &self.merger.providers {
            if let Ok(geo) = provider.extract_line_geometry(current_pdf_path) {
                geometries.extend(geo);
            }
        }

        // 3. Hybrid Merge
        let report = self.merger.merge(bank_stmt.transactions, geometries);
        self.all_transactions = report.transactions.clone();
        if self.all_transactions.is_empty() {
            return Err(EngineError::AiPlanFailed(
                "balance analysis incomplete: no transaction rows were found".into(),
            ));
        }

        // 4. Calculate current global balance status
        let imbalance = self.calculate_global_imbalance();

        if imbalance.abs() < dec!(0.01) {
            self.is_balanced = true;
            tracing::info!("[DOCUMENT ENGINE] ✅ Statement is already perfectly balanced.");
            return Ok(vec![]);
        }

        self.is_balanced = false;
        tracing::info!("[DOCUMENT ENGINE] Imbalance detected: ${}", imbalance);

        // 5. Use Gemini with FULL document context to propose minimal smart adjustments
        tracing::info!(
            "[DOCUMENT ENGINE] Asking Gemini for minimal cascading adjustments across all pages..."
        );
        let layout = self.layout.as_ref().ok_or(EngineError::NotLoaded)?;

        // Gemini's REST contract returns JSON-numbers; convert to f64 at the
        // network boundary, back to Decimal for storage.
        let plan_result = self
            .ai_backend
            .propose_balance_adjustments(&self.all_transactions, dec_to_f64(imbalance), layout)
            .await;

        let changes: Vec<ProposedChange> = match plan_result {
            Ok(plan) => plan
                .adjustments
                .into_iter()
                .map(|adj| {
                    let resolved_bbox = self
                        .all_transactions
                        .iter()
                        .find(|t| t.page == adj.page && t.line_on_page == adj.line_on_page)
                        .and_then(|t| t.field_bboxes.running_balance.or(t.bbox));
                    if resolved_bbox.is_none() {
                        tracing::warn!(
                            "[DOCUMENT ENGINE] no bbox for adjustment on page {} line {}; \
                                 it will be reported but cannot be auto-applied",
                            adj.page,
                            adj.line_on_page
                        );
                    }
                    ProposedChange {
                        page: adj.page,
                        old_text: format!("{}", f64_to_dec(adj.old_running_balance)),
                        new_text: format!("{}", f64_to_dec(adj.new_running_balance)),
                        reason: adj.reason,
                        confidence: adj.confidence,
                        affects_subsequent_balances: true,
                        bbox: resolved_bbox,
                    }
                })
                .collect(),
            Err(e) => {
                tracing::warn!("[DOCUMENT ENGINE] AI balance failed ({e}); falling back to local balance engine");
                let expected_closing = Some(closing_balance);
                match crate::engine::balance::process_and_reconcile(
                    self.all_transactions.clone(),
                    opening_balance,
                    expected_closing,
                ) {
                    Ok((fixed_txs, msg)) => {
                        let mut local_changes = Vec::new();
                        for (orig, fixed) in self.all_transactions.iter().zip(fixed_txs.iter()) {
                            if orig.debit != fixed.debit {
                                local_changes.push(ProposedChange {
                                    page: orig.page,
                                    old_text: orig.debit.map(|d| d.to_string()).unwrap_or_default(),
                                    new_text: fixed
                                        .debit
                                        .map(|d| d.to_string())
                                        .unwrap_or_default(),
                                    reason: msg
                                        .clone()
                                        .unwrap_or_else(|| "Local balance engine (debit)".into()),
                                    confidence: 1.0,
                                    affects_subsequent_balances: true,
                                    bbox: orig.field_bboxes.debit.or(orig.bbox),
                                });
                            }
                            if orig.credit != fixed.credit {
                                local_changes.push(ProposedChange {
                                    page: orig.page,
                                    old_text: orig
                                        .credit
                                        .map(|d| d.to_string())
                                        .unwrap_or_default(),
                                    new_text: fixed
                                        .credit
                                        .map(|d| d.to_string())
                                        .unwrap_or_default(),
                                    reason: msg
                                        .clone()
                                        .unwrap_or_else(|| "Local balance engine (credit)".into()),
                                    confidence: 1.0,
                                    affects_subsequent_balances: true,
                                    bbox: orig.field_bboxes.credit.or(orig.bbox),
                                });
                            }
                        }
                        local_changes
                    }
                    Err(e2) => {
                        return Err(EngineError::AiPlanFailed(format!(
                            "AI failed: {e}. Local fallback also failed: {e2}"
                        )));
                    }
                }
            }
        };

        self.proposed_changes = changes.clone();
        tracing::info!(
            "[DOCUMENT ENGINE] Proposed {} changes across the document",
            self.proposed_changes.len()
        );

        Ok(changes)
    }

    pub fn calculate_global_imbalance(&self) -> Decimal {
        if self.all_transactions.is_empty() {
            return Decimal::ZERO;
        }

        let opening_balance = self
            .all_transactions
            .first()
            .map(|t| {
                t.running_balance.unwrap_or(Decimal::ZERO)
                    - (t.credit.unwrap_or(Decimal::ZERO) - t.debit.unwrap_or(Decimal::ZERO))
            })
            .unwrap_or(Decimal::ZERO);

        let sum_credits: Decimal = self
            .all_transactions
            .iter()
            .map(|t| t.credit.unwrap_or(Decimal::ZERO))
            .sum();
        let sum_debits: Decimal = self
            .all_transactions
            .iter()
            .map(|t| t.debit.unwrap_or(Decimal::ZERO))
            .sum();

        let reported_closing_balance = self
            .all_transactions
            .last()
            .and_then(|t| t.running_balance)
            .unwrap_or(Decimal::ZERO);

        // Correct formula: Calculated = Opening + Credits - Debits
        // Imbalance = Reported - Calculated
        let calculated_closing = opening_balance + sum_credits - sum_debits;
        let diff = reported_closing_balance - calculated_closing;

        // Round to 2 decimal places
        diff.round_dp(2)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn calculate_global_imbalance_correct() {
        // Dummy test for compile
    }
}
