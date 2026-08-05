//! Google Document AI client.
//!
//! Auth strategy (in priority order):
//!  1. **Primary (Beta):** API key via the `v1beta3` endpoint with `?key=...`.
//!  2. **Fallback A (ADC):** Application Default Credentials - the file written
//!     by `gcloud auth application-default login`. We swap the cached
//!     `refresh_token` for a fresh access token at the OAuth2 endpoint.
//!  3. **Fallback B (legacy SA):** Service-account JWT signed locally with the
//!     RSA private key from a service-account JSON.
//!
//! Auth-class errors (401/403) on tier 1 cascade through tiers 2 and 3 in
//! turn. Network errors and non-auth API errors propagate immediately.
//! The response shape is identical across endpoints, so the parser is shared.

use base64::{engine::general_purpose::STANDARD as Base64Standard, Engine};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::StatusCode;
use reqwest_middleware::ClientWithMiddleware;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::config::{AppConfig, DocumentAiConfig};
use crate::engine::model::{f64_to_dec, FieldBboxes, Provenance, Transaction};

#[derive(thiserror::Error, Debug)]
pub enum DocAiError {
    #[error("Missing Configuration: {0}")]
    MissingConfig(&'static str),
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JWT Error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("Auth Error (HTTP {0}): {1}")]
    Auth(StatusCode, String),
    #[error("Network Error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Middleware Error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("Parse Error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Operation timed out after {timeout_secs}s: {operation}")]
    Timeout {
        operation: &'static str,
        timeout_secs: u64,
    },
    #[error("API Error (HTTP {0}): {1}")]
    Api(StatusCode, String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankStatement {
    pub total_pages: usize,
    pub transactions: Vec<Transaction>,
    pub opening_balance: Decimal,
    pub closing_balance: Decimal,
    pub account_number: Option<String>,
    pub bank_name: Option<String>,
}

impl BankStatement {
    pub fn ensure_canonical_metadata(&mut self) {
        for transaction in &mut self.transactions {
            transaction.ensure_canonical_metadata();
            if transaction.canonical.currency.is_none() {
                let text = transaction.raw_text.to_uppercase();
                transaction.canonical.currency = if text.contains("AUD") || text.contains("A$") {
                    Some("AUD".into())
                } else if text.contains("NZD") || text.contains("NZ$") {
                    Some("NZD".into())
                } else if text.contains('€') {
                    Some("EUR".into())
                } else if text.contains('£') {
                    Some("GBP".into())
                } else {
                    None
                };
            }
            if transaction.canonical.locale.is_none() {
                transaction.canonical.locale = match transaction.canonical.currency.as_deref() {
                    Some("AUD") => Some("en-AU".into()),
                    Some("NZD") => Some("en-NZ".into()),
                    Some("GBP") => Some("en-GB".into()),
                    _ => None,
                };
            }
        }
    }
}

#[derive(Serialize)]
struct JwtClaims {
    iss: String,
    scope: String,
    aud: String,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug)]
struct CachedToken {
    access_token: String,
    expires_at: u64,
}

pub struct DocumentAiClient {
    pub config: DocumentAiConfig,
    pub http: ClientWithMiddleware,
    pub location: String,
    pub api_endpoint_override: Option<String>,
    token_cache: tokio::sync::Mutex<Option<CachedToken>>,
    pub app_config: AppConfig,
}

impl DocumentAiClient {
    #[allow(unused_variables)]
    #[allow(unreachable_code)]
    pub fn from_app_config(cfg: &AppConfig) -> Result<Self, DocAiError> {
        #[allow(unreachable_code)]
        return Err(DocAiError::MissingConfig(
            "Document AI is temporarily disabled via user request.",
        ));
        let doc_ai = cfg
            .document_ai
            .clone()
            .ok_or(DocAiError::MissingConfig("document_ai"))?;
        // Require *some* form of credential.
        let mut has_valid_credential = false;
        if !doc_ai.api_key.is_empty()
            || (!doc_ai.service_account_path.is_empty()
                && Path::new(&doc_ai.service_account_path).exists())
            || (!doc_ai.adc_path.is_empty() && Path::new(&doc_ai.adc_path).exists())
        {
            has_valid_credential = true;
        }

        if !has_valid_credential {
            return Err(DocAiError::MissingConfig(
                "DOCUMENT_AI_API_KEY, ADC, or GOOGLE_APPLICATION_CREDENTIALS (files must exist)",
            ));
        }
        Ok(Self {
            config: doc_ai.clone(),
            http: crate::app::config::global_http_client(),
            location: doc_ai.location.clone(),
            api_endpoint_override: None,
            token_cache: tokio::sync::Mutex::new(None),
            app_config: cfg.clone(),
        })
    }

    pub fn new_for_test(
        config: DocumentAiConfig,
        cfg: &AppConfig,
        api_endpoint_override: Option<String>,
    ) -> Self {
        Self {
            config,
            http: crate::app::config::global_http_client(),
            location: "mock-location".into(), // Or real location depending on test
            api_endpoint_override,
            token_cache: tokio::sync::Mutex::new(None),
            app_config: cfg.clone(),
        }
    }

    pub fn new_mock(cfg: &AppConfig) -> Self {
        Self {
            config: DocumentAiConfig::default(),
            http: crate::app::config::global_http_client(),
            location: "mock-location".into(),
            api_endpoint_override: None,
            token_cache: tokio::sync::Mutex::new(None),
            app_config: cfg.clone(),
        }
    }

    fn process_url(&self, version: Option<&str>) -> String {
        let base = if let Some(ref override_url) = self.api_endpoint_override {
            format!(
                "{}/v1/projects/{}/locations/{}/processors/{}",
                override_url,
                self.config.project_id,
                self.config.location,
                self.config.processor_id
            )
        } else {
            format!(
                "https://{}-documentai.googleapis.com/v1/projects/{}/locations/{}/processors/{}",
                self.config.location,
                self.config.project_id,
                self.config.location,
                self.config.processor_id
            )
        };
        match version {
            Some(v) => format!("{base}/processorVersions/{v}:process"),
            None => format!("{base}:process"),
        }
    }

    fn batch_process_url(&self, version: Option<&str>) -> String {
        let base = format!(
            "https://{}-documentai.googleapis.com/v1/projects/{}/locations/{}/processors/{}",
            self.config.location,
            self.config.project_id,
            self.config.location,
            self.config.processor_id
        );
        match version {
            Some(v) => format!("{base}/processorVersions/{v}:batchProcess"),
            None => format!("{base}:batchProcess"),
        }
    }

    async fn get_access_token(&self) -> Result<String, DocAiError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        {
            let cache = self.token_cache.lock().await;
            if let Some(token) = cache.as_ref() {
                if token.expires_at > now + 60 {
                    return Ok(token.access_token.clone());
                }
            }
        }

        // Try ADC first if configured (same precedence the rest of the
        // pipeline expects: API-key > ADC > service-account).
        if !self.config.adc_path.is_empty() {
            match self.refresh_via_adc(now).await {
                Ok(token) => return Ok(token),
                Err(e) => {
                    tracing::warn!(
                        "[doc_ai] ADC token refresh failed: {}; falling back to service-account",
                        e
                    );
                }
            }
        }

        if self.config.service_account_path.is_empty() {
            return Err(DocAiError::MissingConfig("GOOGLE_APPLICATION_CREDENTIALS"));
        }

        let key_content = std::fs::read_to_string(&self.config.service_account_path)?;
        let service_account: serde_json::Value = serde_json::from_str(&key_content)?;

        let client_email = service_account["client_email"]
            .as_str()
            .ok_or_else(|| DocAiError::Parse(serde::de::Error::custom("missing client_email")))?;
        let private_key = service_account["private_key"]
            .as_str()
            .ok_or_else(|| DocAiError::Parse(serde::de::Error::custom("missing private_key")))?;

        let claims = JwtClaims {
            iss: client_email.to_string(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
            aud: "https://oauth2.googleapis.com/token".to_string(),
            iat: now,
            exp: now + 3600,
        };

        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        let encoding_key = EncodingKey::from_rsa_pem(private_key.as_bytes())?;
        let signed_jwt = encode(&header, &claims, &encoding_key)?;

        let response = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &signed_jwt),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(DocAiError::Auth(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }

        let token_resp: TokenResponse = response.json().await?;

        let mut cache = self.token_cache.lock().await;
        *cache = Some(CachedToken {
            access_token: token_resp.access_token.clone(),
            expires_at: now + token_resp.expires_in,
        });

        Ok(token_resp.access_token)
    }

    /// Exchange the refresh_token in an ADC file for a fresh access token.
    /// The ADC file is the one written by `gcloud auth application-default
    /// login` and lives at the platform-specific well-known path.
    async fn refresh_via_adc(&self, now: u64) -> Result<String, DocAiError> {
        let raw = std::fs::read_to_string(&self.config.adc_path)?;
        let adc: serde_json::Value = serde_json::from_str(&raw)?;
        // ADC user files have "type":"authorized_user" + client_id/secret/refresh_token.
        // ADC service-account files have "type":"service_account" + private_key (we
        // intentionally do not handle that here; service-account path covers it).
        let kind = adc["type"].as_str().unwrap_or("");
        if kind != "authorized_user" {
            return Err(DocAiError::Parse(serde::de::Error::custom(format!(
                "ADC file is not an authorized_user (got type={kind:?})"
            ))));
        }
        let client_id = adc["client_id"]
            .as_str()
            .ok_or_else(|| DocAiError::Parse(serde::de::Error::custom("ADC missing client_id")))?;
        let client_secret = adc["client_secret"].as_str().ok_or_else(|| {
            DocAiError::Parse(serde::de::Error::custom("ADC missing client_secret"))
        })?;
        let refresh_token = adc["refresh_token"].as_str().ok_or_else(|| {
            DocAiError::Parse(serde::de::Error::custom("ADC missing refresh_token"))
        })?;

        let response = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(DocAiError::Auth(
                response.status(),
                response.text().await.unwrap_or_default(),
            ));
        }

        let token_resp: TokenResponse = response.json().await?;
        let mut cache = self.token_cache.lock().await;
        *cache = Some(CachedToken {
            access_token: token_resp.access_token.clone(),
            expires_at: now + token_resp.expires_in,
        });
        Ok(token_resp.access_token)
    }

    /// Very lightweight test call to verify the configured credentials
    /// can successfully mint an OAuth token.
    pub async fn ping(&self) -> Result<(), DocAiError> {
        // Just attempting to mint/refresh the token proves the credential exists,
        // is valid JSON, and is cryptographically accepted by Google.
        let _ = self.get_access_token().await?;
        Ok(())
    }

    pub async fn parse_entire_statement(
        &self,
        pdf_path: &Path,
        version: Option<&str>,
    ) -> Result<BankStatement, DocAiError> {
        if self.location == "mock-location" {
            return Ok(BankStatement {
                total_pages: 1,
                transactions: vec![],
                opening_balance: rust_decimal_macros::dec!(0.0),
                closing_balance: rust_decimal_macros::dec!(0.0),
                account_number: None,
                bank_name: None,
            });
        }

        // ----- Cache lookup --------------------------------------------------
        // Document AI is billed per page; if we've parsed this exact PDF
        // through this exact processor before, return the cached result.
        let cache = match crate::ai::docai_cache::DocAiCache::open_default(&self.config.passphrase)
        {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("[doc_ai] cache disabled (open failed): {}", e);
                None
            }
        };
        let cache_key = if cache.is_some() {
            crate::ai::docai_cache::DocAiCache::make_key(
                pdf_path,
                &self.config.project_id,
                &self.config.location,
                &self.config.processor_id,
                version.unwrap_or("default"),
            )
            .ok()
        } else {
            None
        };
        if let (Some(c), Some(k)) = (cache.as_ref(), cache_key.as_ref()) {
            if let Some(hit) = c.get(k) {
                tracing::info!("[doc_ai] cache HIT (skipping network) key={}", &k[..16]);
                return Ok(hit);
            }
        }

        let real_dims = Self::get_real_page_dims(pdf_path);

        let pdf_bytes = std::fs::read(pdf_path)?;
        let base64_pdf = Base64Standard.encode(&pdf_bytes);
        let body = serde_json::json!({
            "rawDocument": {
                "content": base64_pdf,
                "mimeType": "application/pdf"
            }
        });

        // 1. Primary: API key -> v1.
        if !self.config.api_key.is_empty() {
            let url = format!("{}?key={}", self.process_url(version), self.config.api_key);
            tracing::debug!("[doc_ai] trying v1 API-key auth");

            let mut attempts = 0;
            let max_attempts = 4;
            let api_key_res;
            loop {
                attempts += 1;
                match self.http.post(&url).json(&body).send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                            && attempts < max_attempts
                        {
                            let delay =
                                std::time::Duration::from_millis(500 * (1 << (attempts - 1)));
                            tracing::warn!(
                                "[doc_ai] API-key {} error, retrying in {:?}...",
                                status,
                                delay
                            );
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        if status == StatusCode::BAD_REQUEST
                            || status == StatusCode::UNAUTHORIZED
                            || status == StatusCode::FORBIDDEN
                        {
                            tracing::error!("[doc_ai] API Key rejected with {}! Check if your key is valid. Falling back to OAuth.", status);
                        }
                        api_key_res = Some(Ok(resp));
                        break;
                    }
                    Err(e) => {
                        if attempts < max_attempts {
                            let delay =
                                std::time::Duration::from_millis(500 * (1 << (attempts - 1)));
                            tracing::warn!(
                                "[doc_ai] API-key network error {}, retrying in {:?}...",
                                e,
                                delay
                            );
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        api_key_res = Some(Err(e));
                        break;
                    }
                }
            }

            let api_key_res = api_key_res.ok_or_else(|| {
                DocAiError::Api(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "API key request loop failed to yield a response".into(),
                )
            })?;
            match api_key_res {
                Ok(resp) if resp.status().is_success() => {
                    let result: serde_json::Value = resp.json().await?;
                    let stmt = Self::parse_response_into_bank_statement(&result, Some(&real_dims))?;
                    if let (Some(c), Some(k)) = (cache.as_ref(), cache_key.as_ref()) {
                        if let Err(e) = c.put(k, &stmt) {
                            tracing::warn!("[doc_ai] cache write failed: {}", e);
                        }
                    }
                    return Ok(stmt);
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    if status == StatusCode::UNAUTHORIZED
                        || status == StatusCode::FORBIDDEN
                        || status == StatusCode::BAD_REQUEST
                    {
                        tracing::warn!(
                            "[doc_ai] API-key auth rejected ({}); falling back to service-account",
                            status
                        );
                    } else {
                        // Non-auth errors propagate immediately.
                        return Err(DocAiError::Api(status, text));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "[doc_ai] API-key request failed: {}; trying service-account",
                        e
                    );
                }
            }
        }

        // 2. Fallback: OAuth - get_access_token() handles ADC then SA in priority order.
        if self.config.adc_path.is_empty() && self.config.service_account_path.is_empty() {
            return Err(DocAiError::MissingConfig(
                "no OAuth credential available (neither ADC nor service account)",
            ));
        }

        let url = self.process_url(version);
        tracing::debug!("[doc_ai] using v1 OAuth (ADC or service-account)");

        let mut attempts = 0;
        let max_attempts = 4;
        let final_resp;
        loop {
            attempts += 1;
            let access_token = self.get_access_token().await?;
            let req = self.http.post(&url).bearer_auth(access_token).json(&body);
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                        && attempts < max_attempts
                    {
                        let delay = std::time::Duration::from_millis(500 * (1 << (attempts - 1)));
                        tracing::warn!(
                            "[doc_ai] OAuth {} error, retrying in {:?}...",
                            status,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    if status == StatusCode::BAD_REQUEST
                        || status == StatusCode::UNAUTHORIZED
                        || status == StatusCode::FORBIDDEN
                    {
                        tracing::error!(
                            "[doc_ai] OAuth rejected with {}! Check your credentials.",
                            status
                        );
                    }
                    final_resp = Some(Ok(resp));
                    break;
                }
                Err(e) => {
                    if attempts < max_attempts {
                        let delay = std::time::Duration::from_millis(500 * (1 << (attempts - 1)));
                        tracing::warn!(
                            "[doc_ai] OAuth network error {}, retrying in {:?}...",
                            e,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    final_resp = Some(Err(e));
                    break;
                }
            }
        }

        let final_resp = final_resp.ok_or_else(|| {
            DocAiError::Api(
                StatusCode::INTERNAL_SERVER_ERROR,
                "OAuth request loop failed to yield a response".into(),
            )
        })??;

        if !final_resp.status().is_success() {
            return Err(DocAiError::Api(
                final_resp.status(),
                final_resp.text().await.unwrap_or_default(),
            ));
        }

        let result: serde_json::Value = final_resp.json().await?;
        let mut stmt = Self::parse_response_into_bank_statement(&result, Some(&real_dims))?;

        let raw_text = result["text"].as_str().unwrap_or_default();
        let stmt_clone = stmt.clone();

        let backend =
            match crate::ai::backend::AiBackend::from_app_config_async(&self.app_config).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("[doc_ai] Failed to init AI backend for repair: {}", e);
                    return Ok(stmt_clone);
                }
            };

        stmt = crate::ai::repair::verify_and_repair_extraction(&backend, stmt, raw_text)
            .await
            .unwrap_or_else(|e| {
                tracing::error!("[doc_ai] Extraction repair failed completely: {}", e);
                stmt_clone
            });
        if let (Some(c), Some(k)) = (cache.as_ref(), cache_key.as_ref()) {
            if let Err(e) = c.put(k, &stmt) {
                tracing::warn!("[doc_ai] cache write failed: {}", e);
            }
        }
        Ok(stmt)
    }

    /// Replaces the old `parse_chunked_statement`. Dynamically uses `v1:process`
    /// for <= 15 pages and `v1:batchProcess` (LRO) for > 15 pages.
    pub async fn parse_smart_batch(
        &self,
        pdf_path: &Path,
        version: Option<&str>,
        total_pages: usize,
    ) -> Result<BankStatement, DocAiError> {
        if total_pages <= 15 {
            self.parse_entire_statement(pdf_path, version).await
        } else {
            const LRO_TIMEOUT_SECS: u64 = 20 * 60;
            match tokio::time::timeout(
                std::time::Duration::from_secs(LRO_TIMEOUT_SECS),
                self.parse_via_lro(pdf_path, version),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(DocAiError::Timeout {
                    operation: "Document AI batch processing",
                    timeout_secs: LRO_TIMEOUT_SECS,
                }),
            }
        }
    }

    async fn upload_to_gcs(
        &self,
        pdf_path: &Path,
        access_token: &str,
    ) -> Result<String, DocAiError> {
        let uri = &self.config.gcs_output_uri;
        if uri.is_empty() {
            return Err(DocAiError::MissingConfig(
                "DOCUMENT_AI_GCS_URI is required for files > 15 pages",
            ));
        }
        let bucket = uri
            .strip_prefix("gs://")
            .and_then(|s| s.split('/').next())
            .unwrap_or_default();
        let filename = pdf_path.file_name().unwrap_or_default().to_string_lossy();
        let object_name = format!("inputs/{}/{}", uuid::Uuid::new_v4(), filename);
        let url = format!(
            "https://storage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&name={}",
            bucket,
            urlencoding::encode(&object_name)
        );

        let mut attempts = 0;
        let max_attempts = 5;
        loop {
            attempts += 1;
            // We have to recreate the file and stream for each retry since stream is consumed
            let file = tokio::fs::File::open(pdf_path).await?;
            let stream =
                tokio_util::codec::FramedRead::new(file, tokio_util::codec::BytesCodec::new());
            let body = reqwest::Body::wrap_stream(stream);

            match self
                .http
                .post(&url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/pdf")
                .body(body)
                .timeout(std::time::Duration::from_secs(300))
                .send()
                .await
            {
                Ok(r) => {
                    let status = r.status();
                    if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                        && attempts < max_attempts
                    {
                        let delay = std::time::Duration::from_millis(1000 * (1 << (attempts - 1)));
                        tracing::warn!(
                            "[doc_ai] GCS upload error {}, retrying in {:?}...",
                            status,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(DocAiError::Api(status, r.text().await.unwrap_or_default()));
                    }
                    break;
                }
                Err(e) => {
                    if attempts < max_attempts {
                        let delay = std::time::Duration::from_millis(1000 * (1 << (attempts - 1)));
                        tracing::warn!(
                            "[doc_ai] GCS upload network error {}, retrying in {:?}...",
                            e,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(DocAiError::Api(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("GCS upload failed: {}", e),
                    ));
                }
            }
        }

        Ok(format!("gs://{bucket}/{object_name}"))
    }

    async fn parse_via_lro(
        &self,
        pdf_path: &Path,
        version: Option<&str>,
    ) -> Result<BankStatement, DocAiError> {
        let access_token = self.get_access_token().await?;
        let gcs_input_uri = self.upload_to_gcs(pdf_path, &access_token).await?;

        let output_prefix = format!("outputs/{}/", uuid::Uuid::new_v4());
        let gcs_output_uri = format!(
            "{}/{}",
            self.config.gcs_output_uri.trim_end_matches('/'),
            output_prefix
        );

        let url = self.batch_process_url(version);
        let body = serde_json::json!({
            "inputDocuments": {
                "gcsDocuments": {
                    "documents": [{
                        "gcsUri": gcs_input_uri,
                        "mimeType": "application/pdf"
                    }]
                }
            },
            "documentOutputConfig": {
                "gcsOutputConfig": {
                    "gcsUri": gcs_output_uri
                }
            }
        });

        let mut attempts = 0;
        let max_attempts = 5;
        let resp = loop {
            attempts += 1;
            match self
                .http
                .post(&url)
                .bearer_auth(&access_token)
                .json(&body)
                .timeout(std::time::Duration::from_secs(300))
                .send()
                .await
            {
                Ok(r) => {
                    let status = r.status();
                    if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                        && attempts < max_attempts
                    {
                        let delay = std::time::Duration::from_millis(1000 * (1 << (attempts - 1)));
                        tracing::warn!(
                            "[doc_ai] LRO start error {}, retrying in {:?}...",
                            status,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(DocAiError::Api(status, r.text().await.unwrap_or_default()));
                    }
                    break r;
                }
                Err(e) => {
                    if attempts < max_attempts {
                        let delay = std::time::Duration::from_millis(1000 * (1 << (attempts - 1)));
                        tracing::warn!(
                            "[doc_ai] LRO start network error {}, retrying in {:?}...",
                            e,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(DocAiError::Api(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("LRO start failed: {}", e),
                    ));
                }
            }
        };

        let json: serde_json::Value = resp.json().await?;
        let op_name = json["name"].as_str().unwrap_or_default().to_string();

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let op_url = format!(
                "https://{}-documentai.googleapis.com/v1/{}",
                self.config.location, op_name
            );
            let mut poll_attempts = 0;
            let op_resp = loop {
                poll_attempts += 1;
                match self
                    .http
                    .get(&op_url)
                    .bearer_auth(&access_token)
                    .timeout(std::time::Duration::from_secs(30))
                    .send()
                    .await
                {
                    Ok(r) => {
                        let status = r.status();
                        if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                            && poll_attempts < 5
                        {
                            tracing::warn!("[doc_ai] Polling error {}, retrying...", status);
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                        break Ok(r);
                    }
                    Err(e) => {
                        if poll_attempts < 5 {
                            tracing::warn!("[doc_ai] Polling network error {}, retrying...", e);
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                        break Err(e);
                    }
                }
            };

            let op_resp = match op_resp {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("[doc_ai] Polling failed definitively: {}", e);
                    continue;
                }
            };

            if !op_resp.status().is_success() {
                continue;
            }
            let op_json: serde_json::Value = op_resp.json().await?;

            if op_json["done"].as_bool().unwrap_or(false) {
                if let Some(error) = op_json.get("error") {
                    return Err(DocAiError::Api(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        error.to_string(),
                    ));
                }
                break;
            }
        }

        self.download_and_merge_gcs_outputs(&gcs_output_uri, &access_token)
            .await
    }

    async fn download_and_merge_gcs_outputs(
        &self,
        gcs_output_uri: &str,
        access_token: &str,
    ) -> Result<BankStatement, DocAiError> {
        let mut full_raw_text = String::new();
        let uri = gcs_output_uri;
        let bucket = uri
            .strip_prefix("gs://")
            .and_then(|s| s.split('/').next())
            .unwrap_or_default();
        let prefix = uri
            .strip_prefix(&format!("gs://{bucket}/"))
            .unwrap_or_default();

        let list_url = format!(
            "https://storage.googleapis.com/storage/v1/b/{}/o?prefix={}",
            bucket,
            urlencoding::encode(prefix)
        );
        let mut list_attempts = 0;
        let list_resp = loop {
            list_attempts += 1;
            match self
                .http
                .get(&list_url)
                .bearer_auth(access_token)
                .send()
                .await
            {
                Ok(r) => {
                    let status = r.status();
                    if (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                        && list_attempts < 5
                    {
                        tracing::warn!("[doc_ai] GCS list error {}, retrying...", status);
                        tokio::time::sleep(std::time::Duration::from_millis(
                            1000 * (1 << (list_attempts - 1)),
                        ))
                        .await;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(DocAiError::Api(status, r.text().await.unwrap_or_default()));
                    }
                    break r;
                }
                Err(e) => {
                    if list_attempts < 5 {
                        tracing::warn!("[doc_ai] GCS list network error {}, retrying...", e);
                        tokio::time::sleep(std::time::Duration::from_millis(
                            1000 * (1 << (list_attempts - 1)),
                        ))
                        .await;
                        continue;
                    }
                    return Err(DocAiError::Api(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("GCS list failed: {}", e),
                    ));
                }
            }
        };
        let list_json: serde_json::Value = list_resp.json().await?;

        let mut statements = Vec::new();
        if let Some(items) = list_json["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or_default();
                if name.ends_with(".json") {
                    let dl_url = format!(
                        "https://storage.googleapis.com/storage/v1/b/{}/o/{}?alt=media",
                        bucket,
                        urlencoding::encode(name)
                    );
                    let mut dl_attempts = 0;
                    let dl_resp = loop {
                        dl_attempts += 1;
                        match self
                            .http
                            .get(&dl_url)
                            .bearer_auth(access_token)
                            .send()
                            .await
                        {
                            Ok(r) => {
                                let status = r.status();
                                if (status.is_server_error()
                                    || status == StatusCode::TOO_MANY_REQUESTS)
                                    && dl_attempts < 5
                                {
                                    tracing::warn!(
                                        "[doc_ai] GCS download error {}, retrying...",
                                        status
                                    );
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        1000 * (1 << (dl_attempts - 1)),
                                    ))
                                    .await;
                                    continue;
                                }
                                break Ok(r);
                            }
                            Err(e) => {
                                if dl_attempts < 5 {
                                    tracing::warn!(
                                        "[doc_ai] GCS download network error {}, retrying...",
                                        e
                                    );
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        1000 * (1 << (dl_attempts - 1)),
                                    ))
                                    .await;
                                    continue;
                                }
                                break Err(e);
                            }
                        }
                    };

                    let dl_resp = match dl_resp {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::error!(
                                "[doc_ai] Download definitively failed for {}: {}",
                                name,
                                e
                            );
                            continue;
                        }
                    };

                    if !dl_resp.status().is_success() {
                        continue;
                    }
                    let doc_json: serde_json::Value = dl_resp.json().await?;
                    if let Some(text) = doc_json.get("text").and_then(|t| t.as_str()) {
                        full_raw_text.push_str(text);
                        full_raw_text.push('\n');
                    }
                    if let Ok(stmt) = Self::parse_response_into_bank_statement(&doc_json, None) {
                        statements.push(stmt);
                    }
                }
            }
        }

        let mut final_stmt = merge_chunk_results(statements);
        let stmt_clone = final_stmt.clone();

        let backend =
            match crate::ai::backend::AiBackend::from_app_config_async(&self.app_config).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("[doc_ai] Failed to init AI backend for LRO repair: {}", e);
                    return Ok(stmt_clone);
                }
            };

        final_stmt =
            crate::ai::repair::verify_and_repair_extraction(&backend, final_stmt, &full_raw_text)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("[doc_ai] LRO Extraction repair failed completely: {}", e);
                    stmt_clone
                });

        Ok(final_stmt)
    }

    fn get_real_page_dims(pdf_path: &Path) -> std::collections::HashMap<usize, (f32, f32)> {
        let mut dims = std::collections::HashMap::new();
        if let Ok(doc) = lopdf::Document::load(pdf_path) {
            for (i, (_, page_id)) in doc.get_pages().into_iter().enumerate() {
                if let Ok(page_dict) = doc.get_dictionary(page_id) {
                    if let Ok(rect) = page_dict.get(b"MediaBox").and_then(lopdf::Object::as_array) {
                        if rect.len() == 4 {
                            let get_num = |obj: &lopdf::Object| -> f32 {
                                match obj {
                                    lopdf::Object::Real(r) => *r,
                                    lopdf::Object::Integer(i) => *i as f32,
                                    _ => 0.0,
                                }
                            };
                            let w = (get_num(&rect[2]) - get_num(&rect[0])).abs();
                            let h = (get_num(&rect[3]) - get_num(&rect[1])).abs();
                            dims.insert(i, (w, h));
                        }
                    }
                }
            }
        }
        dims
    }

    fn parse_response_into_bank_statement(
        result: &serde_json::Value,
        real_page_dims: Option<&std::collections::HashMap<usize, (f32, f32)>>,
    ) -> Result<BankStatement, DocAiError> {
        let response: DocAiResponse = serde_json::from_value(result.clone())?;
        let total_pages = response.document.pages.len();

        let pages_dim: Vec<(f32, f32, String)> = response.document.pages
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if let Some(real_dims) = real_page_dims {
                    if let Some(&(rw, rh)) = real_dims.get(&i) {
                        return Ok((rw, rh, "points".to_string()));
                    }
                }

                let dim = p.dimension.as_ref().ok_or_else(|| {
                    DocAiError::Parse(serde::de::Error::custom(format!(
                        "Missing physical page dimensions for page {} and no real dimensions provided", i
                    )))
                })?;
                let w = dim.width.ok_or_else(|| {
                    DocAiError::Parse(serde::de::Error::custom("Missing width"))
                })? as f32;
                let h = dim.height.ok_or_else(|| {
                    DocAiError::Parse(serde::de::Error::custom("Missing height"))
                })? as f32;
                Ok((w, h, "points".to_string()))
            })
            .collect::<Result<Vec<_>, DocAiError>>()?;

        let mut transactions = Vec::new();
        let mut opening_balance = Decimal::new(0, 2);
        let mut closing_balance = Decimal::new(0, 2);
        let mut account_number = None;

        for entity in &response.document.entities {
            let (page_idx, row_bbox) = entity_page_and_bbox(entity, &pages_dim);
            let idx = transactions.len();
            let text = entity
                .mention_text
                .as_deref()
                .unwrap_or_else(|| {
                    tracing::warn!("Entity '{}' missing mention_text", entity.entity_type);
                    ""
                })
                .trim()
                .to_string();
            let confidence = entity.confidence.unwrap_or_else(|| {
                tracing::warn!(
                    "Entity '{}' missing confidence, defaulting to 1.0",
                    entity.entity_type
                );
                1.0
            });

            match entity.entity_type.as_str() {
                "table_item" => {
                    let date = extract_string_property(entity, "transaction_deposit_date")
                        .or_else(|| extract_string_property(entity, "transaction_withdrawal_date"))
                        .or_else(|| extract_string_property(entity, "transaction_date"))
                        .unwrap_or_default();

                    let description =
                        extract_string_property(entity, "transaction_deposit_description")
                            .or_else(|| {
                                extract_string_property(
                                    entity,
                                    "transaction_withdrawal_description",
                                )
                            })
                            .or_else(|| extract_string_property(entity, "transaction_description"))
                            .unwrap_or_else(|| text.clone());

                    let credit = extract_number_property(entity, "transaction_deposit");
                    let debit = extract_number_property(entity, "transaction_withdrawal");
                    let running_balance = extract_number_property(entity, "running_balance")
                        .or_else(|| extract_number_property(entity, "transaction_balance"));

                    if credit.is_none() && debit.is_none() && running_balance.is_none() {
                        continue;
                    }

                    let field_bboxes = FieldBboxes {
                        date: property_bbox(
                            entity,
                            &[
                                "transaction_deposit_date",
                                "transaction_withdrawal_date",
                                "transaction_date",
                            ],
                            &pages_dim,
                        ),
                        description: property_bbox(
                            entity,
                            &[
                                "transaction_deposit_description",
                                "transaction_withdrawal_description",
                                "transaction_description",
                            ],
                            &pages_dim,
                        ),
                        debit: property_bbox(
                            entity,
                            &["transaction_withdrawal", "debit"],
                            &pages_dim,
                        ),
                        credit: property_bbox(
                            entity,
                            &["transaction_deposit", "credit"],
                            &pages_dim,
                        ),
                        running_balance: property_bbox(
                            entity,
                            &["running_balance", "transaction_balance"],
                            &pages_dim,
                        ),
                    };

                    transactions.push(Transaction {
                        page: page_idx,
                        line_on_page: idx,
                        date,
                        raw_text: description,
                        debit,
                        credit,
                        running_balance,
                        bbox: row_bbox,
                        field_bboxes,
                        provenance: Provenance::DocumentAI { confidence },
                        category: None,
                        canonical: Default::default(),
                    });
                }
                "transaction" => {
                    let field_bboxes = FieldBboxes {
                        date: property_bbox(
                            entity,
                            &[
                                "transaction_date",
                                "transaction_deposit_date",
                                "transaction_withdrawal_date",
                            ],
                            &pages_dim,
                        ),
                        description: property_bbox(
                            entity,
                            &[
                                "transaction_description",
                                "transaction_deposit_description",
                                "transaction_withdrawal_description",
                            ],
                            &pages_dim,
                        ),
                        debit: property_bbox(
                            entity,
                            &["debit", "transaction_withdrawal"],
                            &pages_dim,
                        ),
                        credit: property_bbox(
                            entity,
                            &["credit", "transaction_deposit"],
                            &pages_dim,
                        ),
                        running_balance: property_bbox(entity, &["running_balance"], &pages_dim),
                    };

                    let date = extract_string_property(entity, "transaction_date")
                        .or_else(|| extract_string_property(entity, "transaction_deposit_date"))
                        .or_else(|| extract_string_property(entity, "transaction_withdrawal_date"))
                        .unwrap_or_default();

                    let description = extract_string_property(entity, "transaction_description")
                        .or_else(|| {
                            extract_string_property(entity, "transaction_deposit_description")
                        })
                        .or_else(|| {
                            extract_string_property(entity, "transaction_withdrawal_description")
                        })
                        .unwrap_or_else(|| text.clone());

                    transactions.push(Transaction {
                        page: page_idx,
                        line_on_page: idx,
                        date,
                        raw_text: description,
                        debit: extract_number_property(entity, "debit")
                            .or_else(|| extract_number_property(entity, "transaction_withdrawal")),
                        credit: extract_number_property(entity, "credit")
                            .or_else(|| extract_number_property(entity, "transaction_deposit")),
                        running_balance: extract_number_property(entity, "running_balance"),
                        bbox: row_bbox,
                        field_bboxes,
                        provenance: Provenance::DocumentAI { confidence },
                        category: None,
                        canonical: Default::default(),
                    });
                }
                "starting_balance" | "opening_balance" => {
                    if let Ok(v) = text.replace(['$', ','], "").parse::<f64>() {
                        opening_balance = f64_to_dec(v);
                    }
                }
                "ending_balance" | "closing_balance" => {
                    if let Ok(v) = text.replace(['$', ','], "").parse::<f64>() {
                        closing_balance = f64_to_dec(v);
                    }
                }
                "account_number" => {
                    if !text.is_empty() {
                        account_number = Some(text);
                    }
                }
                _ => {}
            }
        }

        let mut statement = BankStatement {
            total_pages,
            transactions,
            opening_balance,
            closing_balance,
            account_number,
            bank_name: None,
        };
        statement.ensure_canonical_metadata();
        Ok(statement)
    }

    // -----------------------------------------------------------------------
    // Stage 4 / Item #12: Training orchestration.
    //
    // Document AI lets us train a custom processor version on a labelled
    // dataset. The flow:
    //   1. Poll dataset to count labelled documents.
    //   2. When ≥8 are labelled, kick off `processorVersions:train`.
    //   3. Poll the returned long-running operation until done.
    //   4. Optionally set the new version as the processor's default.
    //
    // All four steps go through the same auth path as `parse_entire_statement`
    // (API key first, ADC, service account). We always use v1beta3 because
    // the training API isn't on v1.
    // -----------------------------------------------------------------------

    /// Authenticate the next request, returning either a complete URL with
    /// `?key=...` appended, or `(url, Some(bearer_token))`.
    ///
    /// Hides the auth tier selection from each training method.
    async fn authed_url(&self, base_url: &str) -> Result<(String, Option<String>), DocAiError> {
        // When using the mock endpoint, don't attempt real auth
        if self.api_endpoint_override.is_some() {
            return Ok((base_url.to_string(), Some("mock-token".to_string())));
        }

        // 1. Beta API key takes precedence if provided.
        if !self.config.api_key.is_empty() {
            let glue = if base_url.contains('?') { '&' } else { '?' };
            return Ok((format!("{base_url}{glue}key={}", self.config.api_key), None));
        }
        if self.config.adc_path.is_empty() && self.config.service_account_path.is_empty() {
            return Err(DocAiError::MissingConfig(
                "no OAuth credential available (neither ADC nor service account)",
            ));
        }
        let token = self.get_access_token().await?;
        Ok((base_url.to_string(), Some(token)))
    }

    /// Apply auth to a `RequestBuilder`. Caller has already chosen the URL
    /// from [`authed_url`]; here we just attach the bearer if present.
    fn apply_auth(
        &self,
        builder: reqwest_middleware::RequestBuilder,
        token: Option<&str>,
    ) -> reqwest_middleware::RequestBuilder {
        match token {
            Some(t) => builder.bearer_auth(t),
            None => builder,
        }
    }

    /// Count documents marked as `LABELED` in the processor's dataset.
    /// Returns `(labeled_count, total_count)`.
    pub async fn count_labeled_documents(&self) -> Result<(usize, usize), DocAiError> {
        let base = format!(
            "https://{}-documentai.googleapis.com/v1beta3/projects/{}/locations/{}/processors/{}/dataset/documents:list",
            self.config.location, self.config.project_id, self.config.location, self.config.processor_id
        );
        let (url, token) = self.authed_url(&base).await?;
        let req = self.http.get(&url);
        let req = self.apply_auth(req, token.as_deref());
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        let body: serde_json::Value = resp.json().await?;
        let docs = body["documentMetadata"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let total = docs.len();
        let labeled = docs
            .iter()
            .filter(|d| {
                d["datasetType"].as_str() == Some("DATASET_SPLIT_TRAIN")
                    || d["datasetType"].as_str() == Some("DATASET_SPLIT_TEST")
            })
            .filter(|d| d["labelingState"].as_str() == Some("DOCUMENT_LABELED"))
            .count();
        Ok((labeled, total))
    }

    /// Kick off training for a new processor version. Returns the LRO name
    /// (`projects/.../operations/<id>`) the caller can poll.
    pub async fn start_training(&self, display_name: &str) -> Result<String, DocAiError> {
        let base = format!(
            "https://{}-documentai.googleapis.com/v1beta3/projects/{}/locations/{}/processors/{}/processorVersions:train",
            self.config.location, self.config.project_id, self.config.location, self.config.processor_id
        );
        let (url, token) = self.authed_url(&base).await?;
        let body = serde_json::json!({
            "processorVersion": {
                "displayName": display_name
            }
        });
        let req = self.http.post(&url).json(&body);
        let req = self.apply_auth(req, token.as_deref());
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        let body: serde_json::Value = resp.json().await?;
        body["name"].as_str().map(|s| s.to_string()).ok_or_else(|| {
            DocAiError::Parse(serde::de::Error::custom("training response missing 'name'"))
        })
    }

    /// Poll an LRO once. Returns `(done, error_message_if_failed)`.
    pub async fn poll_operation(
        &self,
        op_name: &str,
    ) -> Result<(bool, Option<String>), DocAiError> {
        let base = format!(
            "https://{}-documentai.googleapis.com/v1beta3/{}",
            self.config.location, op_name
        );
        let (url, token) = self.authed_url(&base).await?;
        let req = self.http.get(&url);
        let req = self.apply_auth(req, token.as_deref());
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        let body: serde_json::Value = resp.json().await?;
        let done = body["done"].as_bool().unwrap_or(false);
        let err = body["error"]["message"].as_str().map(|s| s.to_string());
        Ok((done, err))
    }

    /// Set a processor version as the processor's default.
    pub async fn set_default_version(&self, version_id: &str) -> Result<(), DocAiError> {
        let base = format!(
            "https://{}-documentai.googleapis.com/v1beta3/projects/{}/locations/{}/processors/{}:setDefaultProcessorVersion",
            self.config.location, self.config.project_id, self.config.location, self.config.processor_id
        );
        let (url, token) = self.authed_url(&base).await?;
        let body = serde_json::json!({
            "defaultProcessorVersion": format!(
                "projects/{}/locations/{}/processors/{}/processorVersions/{}",
                self.config.project_id, self.config.location, self.config.processor_id, version_id
            )
        });
        let req = self.http.post(&url).json(&body);
        let req = self.apply_auth(req, token.as_deref());
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    // ── Document AI Training & Version Management ────────────────────────
    //
    // These methods enable the app to:
    //   1. List available processor versions (pre-trained + custom)
    //   2. Deploy/undeploy versions for inference
    //   3. Trigger training of custom extractors
    //   4. Evaluate trained models
    //   5. Poll long-running operations

    fn v1_base_url(&self) -> String {
        if let Some(ref override_url) = self.api_endpoint_override {
            format!(
                "{}/v1/projects/{}/locations/{}/processors/{}",
                override_url,
                self.config.project_id,
                self.config.location,
                self.config.processor_id
            )
        } else {
            format!(
                "https://{}-documentai.googleapis.com/v1/projects/{}/locations/{}/processors/{}",
                self.config.location,
                self.config.project_id,
                self.config.location,
                self.config.processor_id,
            )
        }
    }

    /// List all processor versions (pre-trained + custom trained).
    /// Returns the raw JSON for the caller to process.
    pub async fn list_processor_versions(&self) -> Result<Vec<ProcessorVersionInfo>, DocAiError> {
        let url = format!("{}/processorVersions", self.v1_base_url());
        let token = self.get_access_token().await?;
        let resp = self.http.get(&url).bearer_auth(&token).send().await?;

        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        let versions = body["processorVersions"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(ProcessorVersionInfo {
                            name: v["name"].as_str()?.to_string(),
                            display_name: v["displayName"].as_str().unwrap_or("").to_string(),
                            state: v["state"].as_str().unwrap_or("UNKNOWN").to_string(),
                            create_time: v["createTime"].as_str().unwrap_or("").to_string(),
                            model_type: if v["googleManaged"].as_bool().unwrap_or(false) {
                                "google_managed".to_string()
                            } else {
                                "custom".to_string()
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(versions)
    }

    /// Deploy a specific processor version for inference.
    /// Returns the operation name for polling.
    pub async fn deploy_processor_version(&self, version_id: &str) -> Result<String, DocAiError> {
        let url = format!(
            "{}/processorVersions/{}:deploy",
            self.v1_base_url(),
            version_id
        );
        let token = self.get_access_token().await?;
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body["name"].as_str().unwrap_or("").to_string())
    }

    /// Undeploy a processor version to stop hosting charges.
    pub async fn undeploy_processor_version(&self, version_id: &str) -> Result<String, DocAiError> {
        let url = format!(
            "{}/processorVersions/{}:undeploy",
            self.v1_base_url(),
            version_id
        );
        let token = self.get_access_token().await?;
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body["name"].as_str().unwrap_or("").to_string())
    }

    /// Trigger training of a new processor version from labeled data.
    ///
    /// `display_name`: Human-readable name for the version (e.g. "AU Bank Statement v1").
    /// `base_version`: Optional base model to fine-tune from. If None, uses the processor's default.
    ///
    /// Returns the long-running operation name for polling.
    pub async fn train_processor_version(
        &self,
        display_name: &str,
        base_version: Option<&str>,
    ) -> Result<String, DocAiError> {
        let url = format!("{}/processorVersions:train", self.v1_base_url());
        let token = self.get_access_token().await?;

        let mut body = serde_json::json!({
            "processorVersion": {
                "displayName": display_name,
            }
        });

        if let Some(base) = base_version {
            body["baseProcessorVersion"] = serde_json::json!(base);
        }

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let result: serde_json::Value = resp.json().await?;
        Ok(result["name"].as_str().unwrap_or("").to_string())
    }

    /// Evaluate a trained processor version. Returns the operation name.
    pub async fn evaluate_processor_version(&self, version_id: &str) -> Result<String, DocAiError> {
        let url = format!(
            "{}/processorVersions/{}:evaluateProcessorVersion",
            self.v1_base_url(),
            version_id
        );
        let token = self.get_access_token().await?;
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body["name"].as_str().unwrap_or("").to_string())
    }

    /// Poll a long-running operation by name. Returns (done, result_json).
    pub async fn get_operation(
        &self,
        operation_name: &str,
    ) -> Result<(bool, serde_json::Value), DocAiError> {
        let url = format!(
            "https://{}-documentai.googleapis.com/v1/{}",
            self.config.location, operation_name,
        );
        let token = self.get_access_token().await?;
        let resp = self.http.get(&url).bearer_auth(&token).send().await?;

        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        let done = body["done"].as_bool().unwrap_or(false);
        Ok((done, body))
    }

    /// Set the default processor version to a specific version ID.
    pub async fn set_default_processor_version(
        &self,
        version_id: &str,
    ) -> Result<String, DocAiError> {
        let url = format!("{}:setDefaultProcessorVersion", self.v1_base_url());
        let token = self.get_access_token().await?;

        let full_version_name = format!(
            "projects/{}/locations/{}/processors/{}/processorVersions/{}",
            self.config.project_id, self.config.location, self.config.processor_id, version_id,
        );

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "defaultProcessorVersion": full_version_name,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(DocAiError::Api(
                resp.status(),
                resp.text().await.unwrap_or_default(),
            ));
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(body["name"].as_str().unwrap_or("").to_string())
    }
}

/// Metadata for a Document AI processor version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorVersionInfo {
    pub name: String,
    pub display_name: String,
    pub state: String,
    pub create_time: String,
    pub model_type: String,
}

/// Merge per-chunk parses into one `BankStatement`. Stage 3 / Item #16.
///
/// Chunks are assumed to be in document order. Transactions retain their
/// (already shifted) `page` numbers; total_pages sums; opening balance and
/// account number come from the first chunk; closing balance from the last.
fn merge_chunk_results(chunked: Vec<BankStatement>) -> BankStatement {
    let mut merged = BankStatement {
        total_pages: 0,
        transactions: Vec::new(),
        opening_balance: Decimal::ZERO,
        closing_balance: Decimal::ZERO,
        account_number: None,
        bank_name: None,
    };
    for (i, stmt) in chunked.into_iter().enumerate() {
        merged.total_pages += stmt.total_pages;
        merged.transactions.extend(stmt.transactions);
        if i == 0 {
            merged.opening_balance = stmt.opening_balance;
            merged.account_number = stmt.account_number;
        }
        merged.closing_balance = stmt.closing_balance;
    }
    merged
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiResponse {
    document: DocAiDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiDocument {
    #[serde(default)]
    pages: Vec<DocAiPage>,
    #[serde(default)]
    entities: Vec<DocAiEntity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiPage {
    dimension: Option<DocAiDimension>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiDimension {
    width: Option<f64>,
    height: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiEntity {
    #[serde(rename = "type")]
    entity_type: String,
    mention_text: Option<String>,
    confidence: Option<f32>,
    page_anchor: Option<DocAiPageAnchor>,
    #[serde(default)]
    properties: Vec<DocAiEntity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiPageAnchor {
    #[serde(default)]
    page_refs: Vec<DocAiPageRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiPageRef {
    #[serde(default)]
    page: Option<serde_json::Value>,
    bounding_poly: Option<DocAiBoundingPoly>,
}

impl DocAiPageRef {
    fn page_idx(&self) -> usize {
        match &self.page {
            Some(serde_json::Value::String(s)) => match s.parse() {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Invalid page string '{}': {}", s, e);
                    0
                }
            },
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or_else(|| {
                tracing::warn!("Invalid page number '{}'", n);
                0
            }) as usize,
            None => {
                tracing::warn!("Missing page property in pageRef");
                0
            }
            Some(v) => {
                tracing::warn!("Unexpected type for page property: {:?}", v);
                0
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiBoundingPoly {
    #[serde(default)]
    normalized_vertices: Vec<DocAiVertex>,
    #[serde(default)]
    vertices: Vec<DocAiVertex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiVertex {
    x: Option<f32>,
    y: Option<f32>,
}

fn extract_string_property(entity: &DocAiEntity, kind: &str) -> Option<String> {
    let prop = entity.properties.iter().find(|p| p.entity_type == kind);
    match prop {
        Some(p) => {
            if p.mention_text.is_none() {
                tracing::warn!("Property '{}' found but missing mention_text", kind);
            }
            p.mention_text.as_deref().map(|s| s.trim().to_string())
        }
        None => None,
    }
}

fn extract_number_property(entity: &DocAiEntity, kind: &str) -> Option<Decimal> {
    let prop = entity.properties.iter().find(|p| p.entity_type == kind);
    match prop {
        Some(p) => {
            if let Some(text) = p.mention_text.as_deref() {
                let s = text.replace(['$', ','], "");
                match s.parse::<f64>() {
                    Ok(f) => Some(f64_to_dec(f)),
                    Err(e) => {
                        tracing::warn!(
                            "Property '{}' contains invalid number format '{}': {}",
                            kind,
                            text,
                            e
                        );
                        None
                    }
                }
            } else {
                tracing::warn!("Property '{}' found but missing mention_text", kind);
                None
            }
        }
        None => None,
    }
}

fn bbox_from_bounding_poly(
    poly: &DocAiBoundingPoly,
    page_idx: usize,
    pages_dim: &[(f32, f32, String)],
) -> Option<[f32; 4]> {
    let is_norm = !poly.normalized_vertices.is_empty();
    let verts = if is_norm {
        &poly.normalized_vertices
    } else {
        &poly.vertices
    };

    if verts.is_empty() {
        return None;
    }

    let mut x0 = f32::MAX;
    let mut y0 = f32::MAX;
    let mut x1 = f32::MIN;
    let mut y1 = f32::MIN;

    for v in verts {
        let x = match v.x {
            Some(val) => val,
            None => {
                tracing::warn!("Vertex missing 'x' coordinate");
                0.0
            }
        };
        let y = match v.y {
            Some(val) => val,
            None => {
                tracing::warn!("Vertex missing 'y' coordinate");
                0.0
            }
        };
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }

    if is_norm {
        if let Some(&(w, h, _)) = pages_dim.get(page_idx) {
            x0 *= w;
            x1 *= w;
            y0 *= h;
            y1 *= h;
        }
    }
    Some([x0, y0, x1, y1])
}

fn entity_page_and_bbox(
    entity: &DocAiEntity,
    pages_dim: &[(f32, f32, String)],
) -> (usize, Option<[f32; 4]>) {
    let Some(anchor) = &entity.page_anchor else {
        tracing::warn!("Entity '{}' missing page_anchor", entity.entity_type);
        return (0, None);
    };
    let Some(first) = anchor.page_refs.first() else {
        tracing::warn!(
            "Entity '{}' has page_anchor but no page_refs",
            entity.entity_type
        );
        return (0, None);
    };
    let page_idx = first.page_idx();
    let bbox = first
        .bounding_poly
        .as_ref()
        .and_then(|p| bbox_from_bounding_poly(p, page_idx, pages_dim));
    if bbox.is_none() {
        tracing::warn!(
            "Entity '{}' missing valid bounding_poly on page {}",
            entity.entity_type,
            page_idx
        );
    }
    (page_idx, bbox)
}

fn property_bbox(
    entity: &DocAiEntity,
    kinds: &[&str],
    pages_dim: &[(f32, f32, String)],
) -> Option<[f32; 4]> {
    for kind in kinds {
        for p in &entity.properties {
            if p.entity_type == *kind {
                let (_, bbox) = entity_page_and_bbox(p, pages_dim);
                if bbox.is_some() {
                    return bbox;
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_into_bank_statement_works() {
        // Two table_items: one with deposit, one without (just a description) - the
        // empty one should be skipped by the parser because it has no money values.
        let json_str = r#"{
            "document": {
                "pages": [{"dimension": {"width": 612.0, "height": 792.0, "unit": "points"}}],
                "entities": [
                    {
                        "type": "table_item",
                        "mentionText": "09/02/2026 Interest Paid 242.83",
                        "confidence": 0.84,
                        "properties": [
                            { "type": "transaction_deposit_date", "mentionText": "09/02/2026" },
                            { "type": "transaction_deposit_description", "mentionText": "Interest Paid" },
                            { "type": "transaction_deposit", "mentionText": "242.83" }
                        ]
                    },
                    {
                        "type": "table_item",
                        "mentionText": "Header row",
                        "confidence": 0.5,
                        "properties": [
                            { "type": "transaction_deposit_description", "mentionText": "Date Description Amount" }
                        ]
                    },
                    { "type": "starting_balance", "mentionText": "$1000.00", "confidence": 0.95 },
                    { "type": "ending_balance",   "mentionText": "$1242.83", "confidence": 0.95 },
                    { "type": "account_number",   "mentionText": "807466413",  "confidence": 0.99 }
                ]
            }
        }"#;
        let val: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let stmt = DocumentAiClient::parse_response_into_bank_statement(&val, None).unwrap();
        assert_eq!(stmt.total_pages, 1);
        assert_eq!(
            stmt.transactions.len(),
            1,
            "header row without amounts should be skipped"
        );
        let tx = &stmt.transactions[0];
        assert_eq!(tx.date, "09/02/2026");
        assert_eq!(tx.raw_text, "Interest Paid");
        assert_eq!(tx.credit, Some(f64_to_dec(242.83)));
        assert_eq!(tx.debit, None);
        assert_eq!(stmt.opening_balance, f64_to_dec(1000.00));
        assert_eq!(stmt.closing_balance, f64_to_dec(1242.83));
        assert_eq!(stmt.account_number.as_deref(), Some("807466413"));
        match tx.provenance {
            Provenance::DocumentAI { confidence } => assert!((confidence - 0.84).abs() < 0.01),
            _ => panic!("Wrong provenance"),
        }
    }

    #[test]
    fn parse_response_handles_legacy_transaction_type() {
        // Older / custom processors emit `type: "transaction"` directly.
        let json_str = r#"{
            "document": {
                "pages": [{"dimension": {"width": 612.0, "height": 792.0, "unit": "points"}}],
                "entities": [
                    {
                        "type": "transaction",
                        "mentionText": "Coffee 3.50",
                        "confidence": 0.9,
                        "properties": [
                            { "type": "transaction_date", "mentionText": "2026-01-15" },
                            { "type": "debit",            "mentionText": "$3.50" }
                        ]
                    }
                ]
            }
        }"#;
        let val: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let stmt = DocumentAiClient::parse_response_into_bank_statement(&val, None).unwrap();
        assert_eq!(stmt.transactions.len(), 1);
        assert_eq!(stmt.transactions[0].debit, Some(f64_to_dec(3.50)));
    }

    #[test]
    fn jwt_claim_shape() {
        const ISSUED_AT: u64 = 1_700_000_000;
        const CLIENT_EMAIL: &str = "document-ai-test@example.invalid";
        let claims = JwtClaims {
            iss: CLIENT_EMAIL.to_string(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
            aud: "https://oauth2.googleapis.com/token".to_string(),
            iat: ISSUED_AT,
            exp: ISSUED_AT + 3600,
        };

        let token_data = serde_json::to_value(&claims).expect("JWT claims must serialize");
        assert_eq!(token_data["iss"], CLIENT_EMAIL);
        assert_eq!(
            token_data["scope"],
            "https://www.googleapis.com/auth/cloud-platform"
        );
        assert_eq!(token_data["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(token_data["iat"], ISSUED_AT);
        assert_eq!(
            token_data["exp"].as_u64().unwrap() - token_data["iat"].as_u64().unwrap(),
            3600
        );
    }

    #[test]
    fn extract_helpers_pull_nested_properties() {
        let entity: crate::ai::document_ai::DocAiEntity = serde_json::from_str(
            r#"{
                "type": "transaction",
                "mentionText": "Coffee 3.50",
                "properties": [
                    { "type": "transaction_date", "mentionText": "2026-05-01" },
                    { "type": "debit", "mentionText": "$3.50" }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(
            extract_string_property(&entity, "transaction_date").as_deref(),
            Some("2026-05-01")
        );
        assert_eq!(
            extract_number_property(&entity, "debit"),
            Some(f64_to_dec(3.50))
        );
        assert_eq!(extract_string_property(&entity, "credit"), None);
    }

    #[test]
    fn from_app_config_requires_credential() {
        let mut cfg = AppConfig::default();
        cfg.passphrase = "x".repeat(20);
        cfg.document_ai = Some(DocumentAiConfig {
            project_id: "p".into(),
            location: "us".into(),
            processor_id: "abc".into(),
            service_account_path: "".into(),
            api_key: "".into(),
            adc_path: "".into(),
            gcs_output_uri: "".into(),
            passphrase: "".into(),
        });

        let res = DocumentAiClient::from_app_config(&cfg);
        assert!(matches!(res, Err(DocAiError::MissingConfig(_))));
    }

    fn fake_chunk(
        page_count: usize,
        opening: f64,
        closing: f64,
        tx_pages: &[usize],
    ) -> BankStatement {
        fake_chunk_with_account(page_count, opening, closing, tx_pages, None)
    }
    fn fake_chunk_with_account(
        page_count: usize,
        opening: f64,
        closing: f64,
        tx_pages: &[usize],
        account: Option<&str>,
    ) -> BankStatement {
        BankStatement {
            total_pages: page_count,
            transactions: tx_pages
                .iter()
                .enumerate()
                .map(|(i, p)| Transaction {
                    page: *p,
                    line_on_page: i,
                    date: "01/01/2026".into(),
                    raw_text: format!("tx{i}"),
                    debit: Some(f64_to_dec(10.0)),
                    credit: None,
                    running_balance: Some(f64_to_dec(opening + 10.0 * (i as f64 + 1.0))),
                    bbox: None,
                    field_bboxes: Default::default(),
                    provenance: Provenance::DocumentAI { confidence: 0.9 },
                    category: None,
                    canonical: Default::default(),
                })
                .collect(),
            opening_balance: f64_to_dec(opening),
            closing_balance: f64_to_dec(closing),
            account_number: account.map(|s| s.to_string()),
            bank_name: None,
        }
    }

    #[test]
    fn merge_chunk_results_sums_pages_keeps_first_opening_last_closing() {
        // Two chunks: first 30 pages opening 100, second 20 pages closing 500.
        let chunks = vec![
            fake_chunk_with_account(30, 100.0, 350.0, &[0, 5, 12], Some("ACC123")),
            fake_chunk(20, 350.0, 500.0, &[30, 35]),
        ];
        let merged = merge_chunk_results(chunks);
        assert_eq!(merged.total_pages, 50);
        assert_eq!(merged.transactions.len(), 5);
        assert_eq!(merged.opening_balance, f64_to_dec(100.0));
        assert_eq!(merged.closing_balance, f64_to_dec(500.0));
        assert_eq!(merged.account_number.as_deref(), Some("ACC123"));
    }

    #[test]
    fn merge_chunk_results_preserves_transaction_order() {
        let chunks = vec![
            fake_chunk(30, 0.0, 0.0, &[0, 1, 2]),
            fake_chunk(30, 0.0, 0.0, &[30, 31]),
        ];
        let merged = merge_chunk_results(chunks);
        let pages: Vec<usize> = merged.transactions.iter().map(|t| t.page).collect();
        assert_eq!(pages, vec![0, 1, 2, 30, 31]);
    }

    #[test]
    fn merge_chunk_results_handles_single_chunk() {
        let chunks = vec![fake_chunk_with_account(10, 50.0, 200.0, &[0, 5], Some("X"))];
        let merged = merge_chunk_results(chunks);
        assert_eq!(merged.total_pages, 10);
        assert_eq!(merged.opening_balance, f64_to_dec(50.0));
        assert_eq!(merged.closing_balance, f64_to_dec(200.0));
    }

    #[test]
    fn merge_chunk_results_handles_empty() {
        let merged = merge_chunk_results(vec![]);
        assert_eq!(merged.total_pages, 0);
        assert!(merged.transactions.is_empty());
    }

    /// Stage 7.5: parse -> edit pipeline integrity. Confirm that bbox info
    /// from Document AI's `pageAnchor.boundingPoly.normalizedVertices`
    /// flows all the way into `Transaction.bbox` and `Transaction.field_bboxes`.
    /// Without this the binary editor redacts the wrong region (or no
    /// region at all).
    #[test]
    fn parse_extracts_row_and_per_field_bboxes_from_page_anchor() {
        let json_str = r#"{
            "document": {
                "pages": [{
                    "pageNumber": 1,
                    "dimension": { "width": 612, "height": 792, "unit": "points" }
                }],
                "entities": [
                    {
                        "type": "table_item",
                        "mentionText": "INTEREST PAID",
                        "confidence": 0.92,
                        "pageAnchor": {
                            "pageRefs": [{
                                "page": "0",
                                "boundingPoly": {
                                    "normalizedVertices": [
                                        { "x": 0.10, "y": 0.30 },
                                        { "x": 0.90, "y": 0.30 },
                                        { "x": 0.90, "y": 0.34 },
                                        { "x": 0.10, "y": 0.34 }
                                    ]
                                }
                            }]
                        },
                        "properties": [
                            {
                                "type": "transaction_deposit_date",
                                "mentionText": "09/02/2026",
                                "pageAnchor": { "pageRefs": [{
                                    "page": "0",
                                    "boundingPoly": { "normalizedVertices": [
                                        { "x": 0.10, "y": 0.30 }, { "x": 0.20, "y": 0.30 },
                                        { "x": 0.20, "y": 0.34 }, { "x": 0.10, "y": 0.34 }
                                    ]}
                                }]}
                            },
                            {
                                "type": "transaction_deposit_description",
                                "mentionText": "Interest Paid",
                                "pageAnchor": { "pageRefs": [{
                                    "page": "0",
                                    "boundingPoly": { "normalizedVertices": [
                                        { "x": 0.22, "y": 0.30 }, { "x": 0.55, "y": 0.30 },
                                        { "x": 0.55, "y": 0.34 }, { "x": 0.22, "y": 0.34 }
                                    ]}
                                }]}
                            },
                            {
                                "type": "transaction_deposit",
                                "mentionText": "$242.83",
                                "pageAnchor": { "pageRefs": [{
                                    "page": "0",
                                    "boundingPoly": { "normalizedVertices": [
                                        { "x": 0.60, "y": 0.30 }, { "x": 0.72, "y": 0.30 },
                                        { "x": 0.72, "y": 0.34 }, { "x": 0.60, "y": 0.34 }
                                    ]}
                                }]}
                            },
                            {
                                "type": "running_balance",
                                "mentionText": "$1,242.83",
                                "pageAnchor": { "pageRefs": [{
                                    "page": "0",
                                    "boundingPoly": { "normalizedVertices": [
                                        { "x": 0.78, "y": 0.30 }, { "x": 0.90, "y": 0.30 },
                                        { "x": 0.90, "y": 0.34 }, { "x": 0.78, "y": 0.34 }
                                    ]}
                                }]}
                            }
                        ]
                    }
                ]
            }
        }"#;
        let val: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let stmt = DocumentAiClient::parse_response_into_bank_statement(&val, None).unwrap();
        assert_eq!(stmt.transactions.len(), 1);

        let tx = &stmt.transactions[0];
        // Row-level bbox: 0.10..0.90 of width 612 = 61.2..550.8, y 237.6..269.28
        let row = tx.bbox.expect("row bbox should be set from pageAnchor");
        assert!((row[0] - 61.2).abs() < 1.0, "x0={}", row[0]);
        assert!((row[1] - 237.6).abs() < 1.0, "y0={}", row[1]);
        assert!((row[2] - 550.8).abs() < 1.0, "x1={}", row[2]);
        assert!((row[3] - 269.28).abs() < 1.0, "y1={}", row[3]);

        // Per-field bboxes: each cell is its own narrower rectangle.
        let credit_box = tx
            .field_bboxes
            .credit
            .expect("transaction_deposit bbox should be set");
        // Credit box: 0.60..0.72 of width 612 = 367.2..440.64
        assert!(
            (credit_box[0] - 367.2).abs() < 1.0,
            "credit x0={}",
            credit_box[0]
        );
        assert!(
            (credit_box[2] - 440.64).abs() < 1.0,
            "credit x1={}",
            credit_box[2]
        );

        let bal_box = tx
            .field_bboxes
            .running_balance
            .expect("running_balance bbox should be set");
        assert!((bal_box[0] - 477.36).abs() < 1.0, "bal x0={}", bal_box[0]);
        assert!((bal_box[2] - 550.8).abs() < 1.0, "bal x1={}", bal_box[2]);

        // The credit and running_balance bboxes do not overlap horizontally.
        assert!(
            credit_box[2] < bal_box[0],
            "credit ({}..{}) and balance ({}..{}) overlap",
            credit_box[0],
            credit_box[2],
            bal_box[0],
            bal_box[2]
        );
    }

    /// Edit-payload integrity: the GUI's bbox_for_field equivalent must
    /// pick the field-specific bbox when present, and the row-level bbox
    /// otherwise. We test the data shape here so a refactor of the helper
    /// stays consistent with what DocAI actually returns.
    #[test]
    fn parse_falls_back_to_row_bbox_when_property_anchor_missing() {
        // Same as above but the deposit has no own pageAnchor - falls back
        // to the row's bbox.
        let json_str = r#"{
            "document": {
                "pages": [{ "pageNumber": 1, "dimension": { "width": 100, "height": 100, "unit": "points" } }],
                "entities": [
                    {
                        "type": "table_item",
                        "mentionText": "X",
                        "confidence": 0.9,
                        "pageAnchor": {
                            "pageRefs": [{
                                "page": "0",
                                "boundingPoly": { "normalizedVertices": [
                                    { "x": 0.0, "y": 0.0 }, { "x": 1.0, "y": 0.0 },
                                    { "x": 1.0, "y": 0.1 }, { "x": 0.0, "y": 0.1 }
                                ]}
                            }]
                        },
                        "properties": [
                            { "type": "transaction_deposit", "mentionText": "10.00" }
                        ]
                    }
                ]
            }
        }"#;
        let val: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let stmt = DocumentAiClient::parse_response_into_bank_statement(&val, None).unwrap();
        assert_eq!(stmt.transactions.len(), 1);
        let tx = &stmt.transactions[0];
        assert!(tx.bbox.is_some(), "row bbox should be set");
        assert!(
            tx.field_bboxes.credit.is_none(),
            "no property anchor -> field bbox should be None"
        );
    }
}
