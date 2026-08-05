use dual_core_pdf_pipeline::pdf::native_engine::pdfium_resolver;
use pdfium_render::prelude::Pdfium;
use std::time::Duration;
use tokio::net::TcpStream;

#[test]
fn test_pdfium_library_loads() {
    let directory = pdfium_resolver::probe_local().unwrap_or_else(|error| {
        panic!(
            "FATAL: no checksum-verified bundled or loadable system Pdfium is available: {error}"
        )
    });

    let bindings = if directory.as_os_str().is_empty() {
        Pdfium::bind_to_system_library()
    } else {
        #[cfg(target_os = "windows")]
        let library_name = "pdfium.dll";
        #[cfg(target_os = "macos")]
        let library_name = "libpdfium.dylib";
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        let library_name = "libpdfium.so";

        Pdfium::bind_to_library(directory.join(library_name))
    };

    assert!(
        bindings.is_ok(),
        "FATAL: the checksum-verified Pdfium library could not be loaded: {:?}",
        bindings.err()
    );
}

#[tokio::test]
async fn test_ai_provider_dns_resolution() {
    let endpoints = vec![
        "generativelanguage.googleapis.com:443", // Gemini AI Studio
        "us-central1-aiplatform.googleapis.com:443", // Vertex AI
        "api.groq.com:443",                      // Groq
        "openrouter.ai:443",                     // OpenRouter
        "us-documentai.googleapis.com:443",      // Document AI
    ];

    for endpoint in endpoints {
        let result =
            tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(endpoint)).await;

        assert!(
            result.is_ok() && result.unwrap().is_ok(),
            "FATAL: Could not resolve or connect to AI provider {}. Network/DNS blocked?",
            endpoint
        );
    }
}
