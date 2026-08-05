//! pdfRest AI Client for High-Fidelity Rendering
//!
//! Optional additive cloud rendering through the pdfRest PDF-to-Images API.
//! Mandatory verification remains local; provider output is explicit evidence only.

use reqwest::multipart;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::fs;
use tokio::time::sleep;

#[derive(Error, Debug)]
pub enum PdfRestError {
    #[error("Failed to upload PDF: {0}")]
    Upload(String),
    #[error("Failed to poll job status: {0}")]
    Poll(String),
    #[error("Failed to download result: {0}")]
    Download(String),
    #[error("Operation timed out during {stage}")]
    Timeout { stage: &'static str },
    #[error("Authentication failed: Check your PDFREST_API_KEY")]
    Auth,
    #[error("Unexpected response from API: {0}")]
    BadResponse(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct PdfRestClient {
    api_key: String,
    http: reqwest::Client,
    base_url: String,
    max_poll_attempts: usize,
    poll_interval: Duration,
}

impl std::fmt::Debug for PdfRestClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfRestClient")
            .field(
                "api_key",
                &format!("<masked: {} chars>", self.api_key.len()),
            )
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    #[serde(rename = "outputId")]
    output_id: Option<String>,
    #[serde(rename = "outputUrl")]
    output_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResourceResponse {
    #[serde(rename = "outputUrl")]
    output_url: Option<String>,
}

impl PdfRestClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            base_url: "https://api.pdfrest.com".into(),
            max_poll_attempts: 60,
            poll_interval: Duration::from_secs(1),
        }
    }

    #[doc(hidden)]
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        let mut client = Self::new(api_key);
        client.base_url = base_url;
        client
    }

    #[doc(hidden)]
    pub fn with_test_policy(
        api_key: String,
        base_url: String,
        max_poll_attempts: usize,
        poll_interval: Duration,
    ) -> Self {
        let mut client = Self::with_base_url(api_key, base_url);
        client.max_poll_attempts = max_poll_attempts;
        client.poll_interval = poll_interval;
        client
    }

    fn authed_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .header("Api-Key", &self.api_key)
    }

    /// Renders a PDF to PNG images using pdfRest.
    /// Returns a list of PathBufs to the downloaded images.
    pub async fn render_pdf_to_images(
        &self,
        pdf: &Path,
        out_dir: &Path,
        dpi: u32,
    ) -> Result<Vec<PathBuf>, PdfRestError> {
        if self.api_key.trim().is_empty() {
            return Err(PdfRestError::Auth);
        }
        if self.max_poll_attempts == 0 {
            return Err(PdfRestError::Timeout { stage: "poll" });
        }
        fs::create_dir_all(out_dir).await?;

        let file = fs::File::open(pdf).await?;
        let stream = tokio_util::codec::FramedRead::new(file, tokio_util::codec::BytesCodec::new());
        let body = reqwest::Body::wrap_stream(stream);
        let filename = pdf
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document.pdf")
            .to_string();

        let form = multipart::Form::new()
            .part(
                "file",
                multipart::Part::stream(body)
                    .file_name(filename)
                    .mime_str("application/pdf")
                    .map_err(|e| PdfRestError::Upload(e.to_string()))?,
            )
            .text("output_type", "png")
            .text("resolution", dpi.to_string());

        let url = format!("{}/pdf-to-images", self.base_url);
        let resp = self
            .authed_request(reqwest::Method::POST, &url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| PdfRestError::Upload(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(PdfRestError::Auth);
        }

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(PdfRestError::Upload(body));
        }

        let upload_res: UploadResponse = resp
            .json()
            .await
            .map_err(|e| PdfRestError::BadResponse(e.to_string()))?;

        let mut output_urls = Vec::new();
        if let Some(url) = upload_res.output_url {
            output_urls.push(url);
        } else if let Some(id) = upload_res.output_id {
            // Poll for completion
            let poll_url = format!("{}/resource/{}", self.base_url, id);
            let mut attempts = 0;

            loop {
                if attempts >= self.max_poll_attempts {
                    return Err(PdfRestError::Timeout { stage: "poll" });
                }

                let poll_resp = self
                    .authed_request(reqwest::Method::GET, &poll_url)
                    .send()
                    .await
                    .map_err(|e| PdfRestError::Poll(e.to_string()))?;

                if poll_resp.status().is_success() {
                    let res: ResourceResponse = poll_resp
                        .json()
                        .await
                        .map_err(|e| PdfRestError::Poll(e.to_string()))?;
                    if let Some(url) = res.output_url {
                        output_urls.push(url);
                        break;
                    }
                }

                attempts += 1;
                sleep(self.poll_interval).await;
            }
        } else {
            return Err(PdfRestError::BadResponse(
                "No outputUrl or outputId in response".into(),
            ));
        }

        let mut downloaded_paths = Vec::new();
        for (i, url) in output_urls.into_iter().enumerate() {
            let download_resp = self
                .http
                .get(&url)
                .send()
                .await
                .map_err(|e| PdfRestError::Download(e.to_string()))?;

            if !download_resp.status().is_success() {
                return Err(PdfRestError::Download(format!(
                    "Status: {}",
                    download_resp.status()
                )));
            }

            let bytes = download_resp
                .bytes()
                .await
                .map_err(|e| PdfRestError::Download(e.to_string()))?;
            if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
                return Err(PdfRestError::BadResponse(format!(
                    "downloaded result {} is not a PNG image",
                    i + 1
                )));
            }
            let out_path = out_dir.join(format!("pdfrest_p{}.png", i + 1));
            fs::write(&out_path, bytes).await?;
            downloaded_paths.push(out_path);
        }

        Ok(downloaded_paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn pdf_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let pdf = directory.path().join("statement.pdf");
        let output = directory.path().join("rendered");
        std::fs::write(&pdf, b"%PDF-1.7\nsynthetic provider fixture\n%%EOF").unwrap();
        (directory, pdf, output)
    }

    fn png_fixture() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\nprovider-evidence".to_vec()
    }

    #[tokio::test]
    async fn missing_key_fails_before_input_io() {
        let client = PdfRestClient::with_base_url(String::new(), "http://127.0.0.1:1".into());
        let result = client
            .render_pdf_to_images(Path::new("missing.pdf"), Path::new("missing-output"), 300)
            .await;
        assert!(matches!(result, Err(PdfRestError::Auth)));
    }

    #[tokio::test]
    async fn malformed_upload_response_is_explicit() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/pdf-to-images"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "unexpected": true
            })))
            .mount(&server)
            .await;
        let (_directory, pdf, output) = pdf_fixture();
        let client = PdfRestClient::with_base_url("test-key".into(), server.uri());
        let result = client.render_pdf_to_images(&pdf, &output, 300).await;
        assert!(
            matches!(result, Err(PdfRestError::BadResponse(message)) if message.contains("No outputUrl"))
        );
    }

    #[tokio::test]
    async fn polling_is_bounded_and_times_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/pdf-to-images"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "outputId": "job-1"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/resource/job-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let (_directory, pdf, output) = pdf_fixture();
        let client = PdfRestClient::with_test_policy(
            "test-key".into(),
            server.uri(),
            2,
            Duration::from_millis(1),
        );
        let result = client.render_pdf_to_images(&pdf, &output, 300).await;
        assert!(matches!(
            result,
            Err(PdfRestError::Timeout { stage: "poll" })
        ));
    }

    #[tokio::test]
    async fn successful_request_is_scoped_and_download_is_uncredentialed() {
        let server = MockServer::start().await;
        let download_url = format!("{}/render.png", server.uri());
        Mock::given(method("POST"))
            .and(path("/pdf-to-images"))
            .and(header("Api-Key", "test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "outputUrl": download_url
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/render.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png_fixture()))
            .mount(&server)
            .await;
        let (_directory, pdf, output) = pdf_fixture();
        let client = PdfRestClient::with_base_url("test-key".into(), server.uri());
        let paths = client
            .render_pdf_to_images(&pdf, &output, 300)
            .await
            .unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(std::fs::read(&paths[0]).unwrap(), png_fixture());

        let requests = server.received_requests().await.unwrap();
        let upload = requests
            .iter()
            .find(|request| request.url.path() == "/pdf-to-images")
            .unwrap();
        let body = String::from_utf8_lossy(&upload.body);
        assert!(body.contains("statement.pdf"));
        assert!(!body.contains(&pdf.to_string_lossy().to_string()));
        let download = requests
            .iter()
            .find(|request| request.url.path() == "/render.png")
            .unwrap();
        assert!(!download.headers.contains_key("Api-Key"));
    }

    #[tokio::test]
    async fn non_png_download_is_rejected_without_artifact() {
        let server = MockServer::start().await;
        let download_url = format!("{}/not-image", server.uri());
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "outputUrl": download_url
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/not-image"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not-a-png"))
            .mount(&server)
            .await;
        let (_directory, pdf, output) = pdf_fixture();
        let client = PdfRestClient::with_base_url("test-key".into(), server.uri());
        let result = client.render_pdf_to_images(&pdf, &output, 300).await;
        assert!(
            matches!(result, Err(PdfRestError::BadResponse(message)) if message.contains("not a PNG"))
        );
        assert!(!output.join("pdfrest_p1.png").exists());
    }
}
