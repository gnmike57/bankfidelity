use crate::ai::document_ai::BankStatement;
use crate::app::config::{AiProviderMode, AppConfig};
use reqwest::StatusCode;
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::path::Path;

const LLAMAPARSE_API_BASE: &str = "https://api.cloud.llamaindex.ai/api/parsing";
const INITIAL_POLL_DELAY_MS: u64 = 2000;
const MAX_POLL_ATTEMPTS: usize = 30;

#[derive(Debug, thiserror::Error)]
pub enum LlamaParseError {
    #[error("Missing Configuration: {0}")]
    MissingConfig(&'static str),
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Middleware Error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("API Error (HTTP {0}): {1}")]
    Api(StatusCode, String),
    #[error("Extraction Failed: {0}")]
    ExtractionFailed(String),
}

#[derive(Deserialize)]
struct UploadResponse {
    id: String,
}

#[derive(Deserialize)]
struct JobStatusResponse {
    status: String,
}

#[derive(Deserialize)]
struct MarkdownResponse {
    markdown: String,
}

pub struct LlamaParseClient {
    http: ClientWithMiddleware,
    raw_http: reqwest::Client,
    api_key: String,
    passphrase: Option<String>,
    app_config: AppConfig,
}

impl LlamaParseClient {
    pub fn from_app_config(cfg: &AppConfig) -> Result<Self, LlamaParseError> {
        let api_key = cfg
            .llamaparse_api_key
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or(LlamaParseError::MissingConfig(
                "LLAMAPARSE_API_KEY is not set",
            ))?;

        let http = crate::app::config::global_http_client();
        let raw_http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();

        Ok(Self {
            http,
            raw_http,
            api_key,
            passphrase: if cfg.passphrase.is_empty() {
                None
            } else {
                Some(cfg.passphrase.clone())
            },
            app_config: cfg.clone(),
        })
    }

    pub async fn parse_statement(&self, pdf_path: &Path) -> Result<BankStatement, LlamaParseError> {
        let cache = match crate::ai::docai_cache::DocAiCache::open_default(
            self.passphrase.as_deref().unwrap_or_default(),
        ) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("[llamaparse] Could not open cache: {}", e);
                None
            }
        };

        let cache_key = cache
            .as_ref()
            .and_then(|_c: &crate::ai::docai_cache::DocAiCache| {
                crate::ai::docai_cache::DocAiCache::make_key(
                    pdf_path,
                    "llamaparse",
                    "global",
                    "default",
                    "v1",
                )
                .ok()
            });

        if let (Some(c), Some(h)) = (cache.as_ref(), cache_key.as_ref()) {
            if let Some(mut cached_stmt) = c.get(h) {
                tracing::info!("[llamaparse] Found cached parsed statement for this file");
                cached_stmt.ensure_canonical_metadata();
                return Ok(cached_stmt);
            }
        }

        let job_id = self.upload_document(pdf_path).await?;
        self.poll_until_complete(&job_id).await?;
        let markdown = self.fetch_markdown(&job_id).await?;

        let mut stmt = self.parse_markdown_to_statement(&markdown)?;
        stmt.ensure_canonical_metadata();

        let stmt_clone = stmt.clone();

        if self.app_config.ai_provider == AiProviderMode::ManualOnly {
            tracing::info!("[llamaparse] AI repair skipped (ManualOnly mode)");
        } else {
            let backend = match crate::ai::backend::AiBackend::from_app_config_async(
                &self.app_config,
            )
            .await
            {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("[llamaparse] Failed to init AI backend for repair: {}", e);
                    return Ok(stmt_clone);
                }
            };

            stmt = crate::ai::repair::verify_and_repair_extraction(&backend, stmt, &markdown)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("[llamaparse] Extraction repair failed completely: {}", e);
                    stmt_clone
                });
        }

        stmt.ensure_canonical_metadata();
        if let (Some(ref c), Some(ref h)) = (&cache, &cache_key) {
            if let Err(e) = c.put(h, &stmt) {
                tracing::warn!("[llamaparse] Failed to cache statement: {}", e);
            }
        }

        Ok(stmt)
    }

    /// Parse statement semantics for transfer planning without invoking the
    /// extraction-repair provider loop. Transfer has its own exact mutation,
    /// engine-math, optional provider-math, and publication gates; waiting for
    /// a second provider repair here can otherwise consume the entire job
    /// deadline before layout mapping begins.
    pub async fn parse_statement_for_transfer(
        &self,
        pdf_path: &Path,
    ) -> Result<BankStatement, LlamaParseError> {
        let cache = crate::ai::docai_cache::DocAiCache::open_default(
            self.passphrase.as_deref().unwrap_or_default(),
        )
        .ok();
        let cache_key = cache.as_ref().and_then(|_| {
            crate::ai::docai_cache::DocAiCache::make_key(
                pdf_path,
                "llamaparse",
                "global",
                "default",
                "v1",
            )
            .ok()
        });
        if let (Some(cache), Some(cache_key)) = (cache.as_ref(), cache_key.as_ref()) {
            if let Some(mut cached_stmt) = cache.get(cache_key) {
                tracing::info!(
                    "[llamaparse] Found cached parsed statement for transfer (repair skipped)"
                );
                cached_stmt.ensure_canonical_metadata();
                return Ok(cached_stmt);
            }
        }

        let job_id = self.upload_document(pdf_path).await?;
        self.poll_until_complete(&job_id).await?;
        let markdown = self.fetch_markdown(&job_id).await?;
        let mut statement = self.parse_markdown_to_statement(&markdown)?;
        statement.ensure_canonical_metadata();
        tracing::info!(
            "[llamaparse] Transfer parse returned {} transactions without extraction repair",
            statement.transactions.len()
        );
        Ok(statement)
    }

    async fn upload_document(&self, pdf_path: &Path) -> Result<String, LlamaParseError> {
        let pdf_bytes = std::fs::read(pdf_path)?;
        let filename = pdf_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let url = format!("{LLAMAPARSE_API_BASE}/upload");
        let mut delay_ms = 1000;
        let max_attempts = 3;

        for attempt in 1..=max_attempts {
            let part = reqwest::multipart::Part::bytes(pdf_bytes.clone())
                .file_name(filename.clone())
                .mime_str("application/pdf")
                .unwrap_or_else(|_| reqwest::multipart::Part::bytes(Vec::new()));

            let form = reqwest::multipart::Form::new().part("file", part);

            match self
                .raw_http
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .multipart(form)
                .send()
                .await
            {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        let status = resp.status();
                        let text = resp.text().await.unwrap_or_default();
                        if attempt == max_attempts {
                            return Err(LlamaParseError::Api(status, text));
                        }
                        tracing::warn!(
                            "[llamaparse] Upload failed (attempt {}): HTTP {} - {}",
                            attempt,
                            status,
                            text
                        );
                    } else {
                        let upload_resp: UploadResponse = resp.json().await.map_err(|e| {
                            LlamaParseError::ExtractionFailed(format!(
                                "Failed to parse upload response: {}",
                                e
                            ))
                        })?;
                        return Ok(upload_resp.id);
                    }
                }
                Err(e) => {
                    if attempt == max_attempts {
                        return Err(e.into());
                    }
                    tracing::warn!(
                        "[llamaparse] Upload network error (attempt {}): {}",
                        attempt,
                        e
                    );
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            delay_ms *= 2;
        }

        Err(LlamaParseError::ExtractionFailed(
            "Upload retries exhausted".into(),
        ))
    }

    async fn poll_until_complete(&self, job_id: &str) -> Result<(), LlamaParseError> {
        let url = format!("{LLAMAPARSE_API_BASE}/job/{job_id}");
        let mut delay_ms = INITIAL_POLL_DELAY_MS;

        for attempt in 1..=MAX_POLL_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

            let resp = match self
                .http
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "[llamaparse] Network error polling job (attempt {}): {}",
                        attempt,
                        e
                    );
                    delay_ms = (delay_ms * 2).min(10000);
                    continue;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if attempt == MAX_POLL_ATTEMPTS {
                    return Err(LlamaParseError::Api(status, text));
                }
                tracing::warn!(
                    "[llamaparse] Poll failed (attempt {}): HTTP {} - {}",
                    attempt,
                    status,
                    text
                );
                delay_ms = (delay_ms * 2).min(10000);
                continue;
            }

            let status_resp: JobStatusResponse = resp.json().await.map_err(|e| {
                LlamaParseError::ExtractionFailed(format!("Failed to parse job status: {}", e))
            })?;

            match status_resp.status.as_str() {
                "SUCCESS" => return Ok(()),
                "ERROR" | "FAILED" => {
                    return Err(LlamaParseError::ExtractionFailed(
                        "LlamaParse job failed on server".into(),
                    ))
                }
                _ => {
                    tracing::debug!("[llamaparse] poll {attempt}: status={}", status_resp.status);
                }
            }
            delay_ms = (delay_ms * 2).min(10000);
        }

        Err(LlamaParseError::ExtractionFailed(
            "Timed out waiting for LlamaParse job to complete".into(),
        ))
    }

    async fn fetch_markdown(&self, job_id: &str) -> Result<String, LlamaParseError> {
        let url = format!("{LLAMAPARSE_API_BASE}/job/{job_id}/result/markdown");

        let mut delay_ms = 1000;
        for attempt in 1..=3 {
            let resp = match self
                .http
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if attempt == 3 {
                        return Err(e.into());
                    }
                    tracing::warn!(
                        "[llamaparse] Network error fetching markdown (attempt {}): {}",
                        attempt,
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    delay_ms *= 2;
                    continue;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if attempt == 3 {
                    return Err(LlamaParseError::Api(status, text));
                }
                tracing::warn!(
                    "[llamaparse] Fetch failed (attempt {}): HTTP {} - {}",
                    attempt,
                    status,
                    text
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                delay_ms *= 2;
                continue;
            }

            let md_resp: MarkdownResponse = resp.json().await.map_err(|e| {
                LlamaParseError::ExtractionFailed(format!(
                    "Failed to parse markdown response: {}",
                    e
                ))
            })?;

            return Ok(md_resp.markdown);
        }

        Err(LlamaParseError::ExtractionFailed(
            "Fetch retries exhausted".into(),
        ))
    }

    pub fn parse_markdown_to_statement(
        &self,
        markdown: &str,
    ) -> Result<BankStatement, LlamaParseError> {
        parse_markdown_to_statement_inner(markdown)
    }
}

/// Parse LlamaParse markdown tables into a `BankStatement`.
///
/// Page markers (`Page N`, `# Page N`) advance a 0-based page counter so
/// multi-page statements keep correct identities for transfer/geometry merge.
/// Empty-date description rows append onto the previous transaction (multi-line).
fn parse_markdown_to_statement_inner(markdown: &str) -> Result<BankStatement, LlamaParseError> {
    let mut transactions: Vec<crate::engine::model::Transaction> = Vec::new();
    let mut in_table = false;
    let mut line_on_page = 0usize;
    // 0-based page index aligned with offline_parser / Document AI.
    let mut current_page = 0usize;
    let mut max_page = 0usize;
    let mut opening_balance = Decimal::ZERO;
    let mut closing_balance = Decimal::ZERO;
    let mut found_opening = false;
    let mut found_closing = false;

    #[allow(clippy::expect_used)] // Static regex patterns — compilation cannot fail
    let page_marker =
        regex::Regex::new(r"(?i)^(?:#{1,6}\s*)?(?:page|pg\.?)\s*(\d+)\s*(?:of\s*\d+)?\s*$")
            .expect("page marker regex");
    #[allow(clippy::expect_used)]
    let opening_re =
        regex::Regex::new(r"(?i)(?:opening|beginning)\s+balance[^0-9\-\(]*(-?\$?[\d,]+\.\d{2})")
            .expect("opening balance regex");
    #[allow(clippy::expect_used)]
    let closing_re =
        regex::Regex::new(r"(?i)(?:closing|ending)\s+balance[^0-9\-\(]*(-?\$?[\d,]+\.\d{2})")
            .expect("closing balance regex");

    let parse_dec = |s: &str| -> Option<Decimal> {
        let cleaned = s.replace(['$', ',', ' ', '(', ')'], "");
        if cleaned.is_empty() || cleaned == "-" || cleaned == "—" {
            return None;
        }
        cleaned.parse::<Decimal>().ok()
    };

    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(caps) = page_marker.captures(line) {
            if let Ok(n) = caps[1].parse::<usize>() {
                // Markdown page labels are typically 1-based.
                current_page = n.saturating_sub(1);
                max_page = max_page.max(current_page);
                line_on_page = 0;
                in_table = false;
            }
            continue;
        }

        if !found_opening {
            if let Some(caps) = opening_re.captures(line) {
                if let Some(v) = parse_dec(caps.get(1).map(|m| m.as_str()).unwrap_or("")) {
                    opening_balance = v;
                    found_opening = true;
                }
            }
        }
        if !found_closing {
            if let Some(caps) = closing_re.captures(line) {
                if let Some(v) = parse_dec(caps.get(1).map(|m| m.as_str()).unwrap_or("")) {
                    closing_balance = v;
                    found_closing = true;
                }
            }
        }

        if line.starts_with('|') {
            if line.contains("---") {
                in_table = true;
                continue;
            }
            if !in_table {
                // Header row starts a table; skip until separator, but if we
                // already saw a separator on a previous pass this is fine.
                // Treat any pipe row with enough cells as table body once past ---.
                continue;
            }
            let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
            // | Date | Description | Debit | Credit | Balance |
            // split yields leading/trailing empties → need >= 5 meaningful cells.
            if parts.len() < 5 {
                continue;
            }
            let date = parts.get(1).unwrap_or(&"").to_string();
            let desc = parts.get(2).unwrap_or(&"").to_string();
            let debit = parts.get(3).and_then(|s| parse_dec(s));
            let credit = parts.get(4).and_then(|s| parse_dec(s));
            let balance = parts.get(5).and_then(|s| parse_dec(s));

            // Skip obvious header labels.
            let date_l = date.to_ascii_lowercase();
            if date_l == "date" || date_l.contains("transaction") {
                continue;
            }

            let is_continuation =
                date.is_empty() && debit.is_none() && credit.is_none() && !desc.is_empty();

            if is_continuation {
                // Multi-line description wrap (optionally with a balance-only
                // cell we ignore). Append onto the previous row.
                if let Some(last_tx) = transactions.last_mut() {
                    if !last_tx.raw_text.ends_with(' ') && !desc.starts_with(' ') {
                        last_tx.raw_text.push(' ');
                    }
                    last_tx.raw_text.push_str(&desc);
                    if last_tx.running_balance.is_none() {
                        last_tx.running_balance = balance;
                    }
                }
                continue;
            }

            if date.is_empty() || (debit.is_none() && credit.is_none()) {
                continue;
            }

            line_on_page += 1;
            max_page = max_page.max(current_page);
            let raw_text = if desc.is_empty() {
                date.clone()
            } else {
                format!("{date} {desc}")
            };
            transactions.push(crate::engine::model::Transaction {
                page: current_page,
                line_on_page,
                date,
                raw_text,
                debit,
                credit,
                running_balance: balance,
                bbox: None,
                field_bboxes: Default::default(),
                provenance: crate::engine::model::Provenance::Computed,
                category: None,
                canonical: Default::default(),
            });
        } else {
            in_table = false;
        }
    }

    if transactions.is_empty() {
        tracing::warn!(
            "[llamaparse] No transactions found in markdown. Returning ExtractionFailed to trigger fallback hook."
        );
        return Err(LlamaParseError::ExtractionFailed(
            "LlamaParse returned markdown but 0 transactions were extracted. Triggering fallback."
                .into(),
        ));
    }

    tracing::info!(
        "[llamaparse] Parsed {} transactions from markdown (pages={}).",
        transactions.len(),
        max_page + 1
    );

    // Infer opening/closing from running balances when not explicit.
    if !found_opening {
        if let Some(first) = transactions.first() {
            if let Some(bal) = first.running_balance {
                let net =
                    first.debit.unwrap_or(Decimal::ZERO) - first.credit.unwrap_or(Decimal::ZERO);
                opening_balance = bal - net;
            }
        }
    }
    if !found_closing {
        if let Some(last) = transactions.last() {
            if let Some(bal) = last.running_balance {
                closing_balance = bal;
            }
        }
    }

    Ok(BankStatement {
        total_pages: max_page + 1,
        transactions,
        opening_balance,
        closing_balance,
        account_number: None,
        bank_name: None::<String>,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn markdown_continuation_appends_to_previous_raw_text() {
        let md = r#"
| Date | Description | Debit | Credit | Balance |
|------|-------------|-------|--------|---------|
| 15/01/2024 | Payment to Merchant | 50.00 |  | 1050.00 |
|  | Ref 1394711 Osko |  |  |  |
| 16/01/2024 | Coffee | 5.00 |  | 1045.00 |
"#;
        let stmt = parse_markdown_to_statement_inner(md).expect("parse");
        assert_eq!(stmt.transactions.len(), 2);
        assert!(
            stmt.transactions[0].raw_text.contains("Ref 1394711 Osko"),
            "continuation must append: {:?}",
            stmt.transactions[0].raw_text
        );
        assert!(!stmt.transactions[1].raw_text.contains("1394711"));
    }

    #[test]
    fn markdown_page_markers_set_zero_based_pages() {
        let md = r#"
Page 1

| Date | Description | Debit | Credit | Balance |
|------|-------------|-------|--------|---------|
| 01/01/2024 | Alpha | 10.00 |  | 110.00 |

Page 2

| Date | Description | Debit | Credit | Balance |
|------|-------------|-------|--------|---------|
| 02/01/2024 | Beta |  | 5.00 | 105.00 |
"#;
        let stmt = parse_markdown_to_statement_inner(md).expect("parse");
        assert_eq!(stmt.total_pages, 2);
        assert_eq!(stmt.transactions.len(), 2);
        assert_eq!(stmt.transactions[0].page, 0);
        assert_eq!(stmt.transactions[1].page, 1);
        assert_eq!(stmt.transactions[0].line_on_page, 1);
        assert_eq!(stmt.transactions[1].line_on_page, 1);
    }

    #[test]
    fn markdown_opening_closing_and_raw_text_include_date() {
        let md = r#"
Opening Balance $1,000.00

| Date | Description | Debit | Credit | Balance |
|------|-------------|-------|--------|---------|
| 15/01/2024 | Deposit |  | 500.00 | 1500.00 |

Closing Balance $1,500.00
"#;
        let stmt = parse_markdown_to_statement_inner(md).expect("parse");
        assert_eq!(stmt.opening_balance, dec!(1000.00));
        assert_eq!(stmt.closing_balance, dec!(1500.00));
        assert_eq!(stmt.transactions.len(), 1);
        assert!(stmt.transactions[0].raw_text.starts_with("15/01/2024"));
        assert!(stmt.transactions[0].raw_text.contains("Deposit"));
        assert_eq!(stmt.transactions[0].credit, Some(dec!(500.00)));
    }
}
