use dual_core_pdf_pipeline::ai::backend::AiBackend;
use dual_core_pdf_pipeline::ai::document_ai::DocumentAiClient;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::engine::model::Transaction;
use dual_core_pdf_pipeline::engine::statement::SmartDocumentEngine;
use dual_core_pdf_pipeline::extractors::merger::HybridMerger;
use dual_core_pdf_pipeline::pdf::OxidizePdfEngine;
use rust_decimal_macros::dec;
use std::sync::Arc;

#[tokio::test]
async fn test_smart_document_engine_initialization() {
    let mut config = AppConfig::default();
    config.passphrase = "test-passphrase-1234567890".into();
    let config_arc = Arc::new(config);

    let pdf_engine = Arc::new(OxidizePdfEngine::new());
    let doc_ai = Arc::new(DocumentAiClient::new_mock(&config_arc));
    let ai_backend = Arc::new(AiBackend::new_mock());
    let merger = Arc::new(HybridMerger::new(vec![]));

    let engine = SmartDocumentEngine::new(
        pdf_engine.clone(),
        doc_ai.clone(),
        ai_backend.clone(),
        merger.clone(),
    );

    assert!(!engine.is_balanced);
    assert_eq!(engine.total_pages, 0);
    assert!(engine.all_transactions.is_empty());
}

#[tokio::test]
async fn test_calculate_global_imbalance_empty() {
    let mut config = AppConfig::default();
    config.passphrase = "test-passphrase-1234567890".into();
    let config_arc = Arc::new(config);

    let pdf_engine = Arc::new(OxidizePdfEngine::new());
    let doc_ai = Arc::new(DocumentAiClient::new_mock(&config_arc));
    let ai_backend = Arc::new(AiBackend::new_mock());
    let merger = Arc::new(HybridMerger::new(vec![]));

    let engine = SmartDocumentEngine::new(
        pdf_engine.clone(),
        doc_ai.clone(),
        ai_backend.clone(),
        merger.clone(),
    );

    let imbalance = engine.calculate_global_imbalance();
    assert_eq!(imbalance, dec!(0.0));
}

#[tokio::test]
async fn test_calculate_global_imbalance_with_transactions() {
    let mut config = AppConfig::default();
    config.passphrase = "test-passphrase-1234567890".into();
    let config_arc = Arc::new(config);

    let pdf_engine = Arc::new(OxidizePdfEngine::new());
    let doc_ai = Arc::new(DocumentAiClient::new_mock(&config_arc));
    let ai_backend = Arc::new(AiBackend::new_mock());
    let merger = Arc::new(HybridMerger::new(vec![]));

    let mut engine = SmartDocumentEngine::new(
        pdf_engine.clone(),
        doc_ai.clone(),
        ai_backend.clone(),
        merger.clone(),
    );

    // Opening balance 100, add 50, subtract 20 -> Ending balance should be 130
    engine.all_transactions = vec![
        Transaction {
            page: 1,
            line_on_page: 0,
            date: "2023-01-01".into(),
            raw_text: "Opening".into(),
            credit: Some(dec!(100.0)),
            debit: None,
            running_balance: Some(dec!(100.0)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: dual_core_pdf_pipeline::engine::model::Provenance::Computed,
            category: None,
            canonical: Default::default(),
        },
        Transaction {
            page: 1,
            line_on_page: 1,
            date: "2023-01-02".into(),
            raw_text: "Deposit".into(),
            credit: Some(dec!(50.0)),
            debit: None,
            running_balance: Some(dec!(150.0)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: dual_core_pdf_pipeline::engine::model::Provenance::Computed,
            category: None,
            canonical: Default::default(),
        },
        Transaction {
            page: 1,
            line_on_page: 2,
            date: "2023-01-03".into(),
            raw_text: "Withdrawal".into(),
            credit: None,
            debit: Some(dec!(20.0)),
            running_balance: Some(dec!(130.0)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: dual_core_pdf_pipeline::engine::model::Provenance::Computed,
            category: None,
            canonical: Default::default(),
        },
        Transaction {
            page: 1,
            line_on_page: 3,
            date: "2023-01-31".into(),
            raw_text: "Closing".into(),
            credit: None,
            debit: None,
            running_balance: Some(dec!(130.0)),
            bbox: None,
            field_bboxes: Default::default(),
            provenance: dual_core_pdf_pipeline::engine::model::Provenance::Computed,
            category: None,
            canonical: Default::default(),
        },
    ];

    let imbalance = engine.calculate_global_imbalance();
    assert_eq!(imbalance, dec!(0.0)); // Perfectly balanced

    // Now introduce an error
    engine.all_transactions[2].debit = Some(dec!(30.0));
    // Opening = 100 - (100 - 0) = 0? Wait, the first transaction has credit 100.
    // If the first line is just a transaction, opening_balance = 100 - 100 = 0.
    // Sum credits = 100 + 50 = 150
    // Sum debits = 30
    // Calculated = 0 + 150 - 30 = 120
    // Reported closing = 130
    // diff = reported (130) - calculated (120) = 10.0
    let new_imbalance = engine.calculate_global_imbalance();
    assert_eq!(new_imbalance, dec!(10.0));
}

#[tokio::test]
async fn test_balance_entire_statement_rejects_empty_ledger() {
    let mut config = AppConfig::default();
    config.passphrase = "test-passphrase-1234567890".into();
    let config_arc = Arc::new(config);

    let pdf_engine = Arc::new(OxidizePdfEngine::new());
    let doc_ai = Arc::new(DocumentAiClient::new_mock(&config_arc));
    let ai_backend = Arc::new(AiBackend::new_mock());
    let merger = Arc::new(HybridMerger::new(vec![]));

    let mut engine = SmartDocumentEngine::new(
        pdf_engine.clone(),
        doc_ai.clone(),
        ai_backend.clone(),
        merger.clone(),
    );
    engine.layout = Some(dual_core_pdf_pipeline::engine::layout::DocumentLayout::default());

    // Create a dummy PDF
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("dummy.pdf");
    use lopdf::{dictionary, Document, Object};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(&pdf_path).unwrap();

    let error = engine
        .balance_entire_statement(&pdf_path)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no transaction rows were found"));
    assert!(!engine.is_balanced);
}

#[tokio::test]
async fn test_load_full_document() {
    let mut config = AppConfig::default();
    config.passphrase = "test-passphrase-1234567890".into();
    let config_arc = Arc::new(config);

    let pdf_engine = Arc::new(OxidizePdfEngine::new());
    let doc_ai = Arc::new(DocumentAiClient::new_mock(&config_arc));
    let ai_backend = Arc::new(AiBackend::new_mock());
    let merger = Arc::new(HybridMerger::new(vec![]));

    let mut engine = SmartDocumentEngine::new(
        pdf_engine.clone(),
        doc_ai.clone(),
        ai_backend.clone(),
        merger.clone(),
    );

    // load_full_document doesn't actually require the file to exist for the mocked pdf_engine in OxidizePdfEngine?
    // Wait, OxidizePdfEngine might try to load it. Let's create a dummy PDF.
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("dummy.pdf");

    // Instead of actually using pdf_engine for real file, wait, does OxidizePdfEngine fail if file missing?
    // OxidizePdfEngine::get_page_count loads the document. So we must provide a real PDF.
    use lopdf::{dictionary, Document, Object};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(&pdf_path).unwrap();

    let (tx, _rx) = std::sync::mpsc::channel();
    let res = engine.load_full_document(&tx, &pdf_path).await;
    assert!(res.is_ok());
    assert_eq!(engine.total_pages, 1);
}

#[tokio::test]
async fn test_balance_entire_statement_with_ai_fallback() {
    let mut config = AppConfig::default();
    config.passphrase = "test-passphrase-1234567890".into();
    let config_arc = Arc::new(config);

    let pdf_engine = Arc::new(OxidizePdfEngine::new());

    // We will set up a mock DocumentAiClient that returns an unbalanced statement
    let doc_ai = Arc::new(DocumentAiClient::new_mock(&config_arc));
    let ai_backend = Arc::new(AiBackend::new_mock()); // This mock will fail, triggering the fallback!
    let merger = Arc::new(HybridMerger::new(vec![]));

    let mut engine = SmartDocumentEngine::new(
        pdf_engine.clone(),
        doc_ai.clone(),
        ai_backend.clone(),
        merger.clone(),
    );
    engine.layout = Some(dual_core_pdf_pipeline::engine::layout::DocumentLayout::default());

    // Because doc_ai parse_entire_statement is hard to mock dynamically here (it's using the real mock function),
    // let's actually let it return what it returns, and then we'll see if we can trigger the fallback.
    // Wait, the new_mock() for doc_ai returns a perfectly balanced dummy statement:
    // opening: 1000, 1 withdrawal of 200, closing: 800.

    // Actually, DocumentAiClient::new_mock is not very configurable here.
    // Let's create a new engine with a special test wrapper or we can mock DocumentAi?
    // Wait! I can't easily mock `doc_ai.parse_entire_statement` because it's hardcoded.
    // Let me check what `parse_entire_statement` does in mock mode!
}
