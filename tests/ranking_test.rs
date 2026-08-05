use dual_core_pdf_pipeline::ai::document_ai::DocumentAiClient;
use dual_core_pdf_pipeline::ai::llamaparse::LlamaParseClient;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::engine::consensus::merge_consensus_statements;
use dual_core_pdf_pipeline::engine::offline_parser::parse_statement_offline;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn get_test_pdfs() -> Vec<PathBuf> {
    let mut pdfs = Vec::new();
    let dirs = vec!["tests/stress_pdfs", "AU Bank Statements"];
    for dir in dirs {
        let path = Path::new(dir);
        if path.exists() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().unwrap_or_default() == "pdf" {
                        pdfs.push(p);
                    }
                }
            }
        }
    }
    pdfs.truncate(5); // Limit to 5 for test speed
    pdfs
}

#[tokio::test]
#[ignore]
async fn test_parser_ranking() {
    let _ = dotenvy::dotenv();
    let cfg = match AppConfig::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            println!("Skipping live ranking test due to missing config: {}", e);
            return;
        }
    };

    let doc_ai = DocumentAiClient::from_app_config(&cfg);
    let llama = LlamaParseClient::from_app_config(&cfg);
    let mut stats = dual_core_pdf_pipeline::engine::model::ParserStats::default();
    let pdfs = get_test_pdfs();

    if pdfs.is_empty() {
        println!("No PDFs found for ranking test.");
        return;
    }

    println!("Running ranking test on {} PDFs...", pdfs.len());

    for pdf in pdfs {
        println!("Evaluating {:?}", pdf);
        stats.total_attempts += 1;

        let mut stmts = Vec::new();
        let mut named_stmts = Vec::new();

        if let Ok(client) = &doc_ai {
            if let Ok(stmt) = client.parse_entire_statement(&pdf, None).await {
                stmts.push(stmt.clone());
                named_stmts.push(("DocAI", stmt));
            }
        }

        if let Ok(client) = &llama {
            if let Ok(stmt) = client.parse_statement(&pdf).await {
                stmts.push(stmt.clone());
                named_stmts.push(("LlamaParse", stmt));
            }
        }

        let offline = parse_statement_offline(
            &pdf,
            Arc::new(dual_core_pdf_pipeline::pdf::OxidizePdfEngine::new()),
        );
        if let Ok(stmt) = offline {
            stmts.push(stmt.clone());
            named_stmts.push(("Offline", stmt));
        }

        if stmts.is_empty() {
            continue;
        }

        let consensus = merge_consensus_statements(stmts);

        let mut best_dist = usize::MAX;
        let mut winner = "";
        for (name, s) in &named_stmts {
            let dist = (s.transactions.len() as isize - consensus.transactions.len() as isize)
                .unsigned_abs();
            if dist < best_dist {
                best_dist = dist;
                winner = *name;
            }
        }

        match winner {
            "DocAI" => stats.docai_wins += 1,
            "LlamaParse" => stats.llamaparse_wins += 1,
            "Offline" => stats.offline_wins += 1,
            _ => {}
        }

        println!("  Winner for this document: {}", winner);
    }

    println!("\n=== Final Ranking (Consensus Wins) ===");
    println!("DocAI: {}", stats.docai_wins);
    println!("LlamaParse: {}", stats.llamaparse_wins);
    println!("Offline: {}", stats.offline_wins);
}
