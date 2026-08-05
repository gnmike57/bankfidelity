use dual_core_pdf_pipeline::ai::document_ai::DocumentAiClient;
use dual_core_pdf_pipeline::ai::gemini_client::GeminiClient;
use dual_core_pdf_pipeline::app::config::AppConfig;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Setup tracing so we can see the internal debug/error logs of our clients
    tracing_subscriber::fmt::init();

    println!("========================================");
    println!("  AI Credential Verification Suite      ");
    println!("========================================\n");

    // Load configuration from environment (includes GEMINI_API_KEY if set)
    dotenvy::dotenv().ok();
    let cfg = match AppConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ Configuration Load Failed: {e}");
            return Err(e.into());
        }
    };

    println!("✅ Configuration Loaded Successfully.");

    // --- GEMINI VERIFICATION ---
    println!("\n--- Verifying Gemini API ---");
    if cfg.gemini_api_key.is_some()
        || cfg.gemini_auth_mode == dual_core_pdf_pipeline::app::config::GeminiAuthMode::Vertex
    {
        match GeminiClient::from_app_config_async(&cfg).await {
            Ok(gemini) => {
                // Ping the Gemini API with a dummy payload
                println!("📡 Pinging Gemini API...");
                match gemini.ping().await {
                    Ok(_) => println!("✅ Gemini API is ONLINE and credentials are VALID."),
                    Err(e) => eprintln!("❌ Gemini API Ping Failed: {e}"),
                }
            }
            Err(e) => eprintln!("❌ Failed to initialize GeminiClient: {e}"),
        }
    } else {
        println!("⚠️ Gemini is skipped (no credentials provided).");
    }

    // --- DOCUMENT AI VERIFICATION ---
    println!("\n--- Verifying Document AI ---");
    if cfg.has_ai_for_extraction() {
        match DocumentAiClient::from_app_config(&cfg) {
            Ok(docai) => {
                println!("📡 Pinging Document AI API...");
                match docai.ping().await {
                    Ok(_) => println!("✅ Document AI is ONLINE and credentials are VALID."),
                    Err(e) => eprintln!("❌ Document AI Ping Failed: {e}"),
                }
            }
            Err(e) => eprintln!("❌ Failed to initialize DocumentAiClient: {e}"),
        }
    } else {
        println!("⚠️ Document AI is skipped (no configuration provided).");
    }

    println!("\n========================================");
    println!("  Verification Suite Complete!          ");
    println!("========================================");

    Ok(())
}
