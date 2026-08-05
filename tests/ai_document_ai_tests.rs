use dual_core_pdf_pipeline::ai::document_ai::DocumentAiClient;
use dual_core_pdf_pipeline::app::config::AppConfig;
use dual_core_pdf_pipeline::app::config::DocumentAiConfig;
use mockito::Server;

#[tokio::test]
async fn test_parse_entire_statement_success() {
    let mut server = Server::new_async().await;

    // We can simulate the successful JSON response that DocAI sends back
    let doc_ai_response = serde_json::json!({
        "document": {
            "text": "Opening Balance: $100\nClosing Balance: $150\n",
            "entities": [
                { "type": "opening_balance", "mentionText": "$100" },
                { "type": "closing_balance", "mentionText": "$150" }
            ],
            "pages": [{
                "pageNumber": 1,
                "dimension": { "width": 8.5, "height": 11.0 }
            }]
        }
    });

    let mock = server
        .mock(
            "POST",
            "/v1/projects/my-project/locations/us/processors/my-processor:process?key=fake-key",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(doc_ai_response.to_string())
        .create_async()
        .await;

    let _app_config = AppConfig::default();
    let config = DocumentAiConfig {
        project_id: "my-project".to_string(),
        location: "us".to_string(),
        processor_id: "my-processor".to_string(),
        api_key: "fake-key".to_string(), // ensures it goes down the beta api_key auth path
        ..DocumentAiConfig::default()
    };

    // We construct the client manually to hit the real logic
    let mut app_config = AppConfig::default();
    app_config.passphrase = uuid::Uuid::new_v4().to_string(); // bypass cache
    let mut client = DocumentAiClient::new_for_test(config, &app_config, Some(server.url()));
    client.location = "us".to_string(); // override mock location from test builder

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

    // Bypass cache by injecting a unique UUID into the trailer
    doc.trailer
        .set("TestNonce", uuid::Uuid::new_v4().to_string());

    doc.save(&pdf_path).unwrap();

    let result = client.parse_entire_statement(&pdf_path, None).await;

    if let Err(e) = &result {
        println!("Error returned: {:?}", e);
    }

    let bank_statement = result.unwrap();
    mock.assert_async().await;

    assert_eq!(bank_statement.opening_balance.to_string(), "100");
    assert_eq!(bank_statement.closing_balance.to_string(), "150");
    assert_eq!(bank_statement.total_pages, 1);
}

#[tokio::test]
async fn test_parse_entire_statement_retry_on_429() {
    let mut server = Server::new_async().await;

    let _doc_ai_response = serde_json::json!({
        "document": {
            "text": "Opening Balance: $0\nClosing Balance: $0\n",
            "entities": [],
            "pages": [{
                "pageNumber": 1,
                "dimension": { "width": 8.5, "height": 11.0 }
            }]
        }
    });

    // In mockito, to test a sequence, we can return 429 for all requests,
    // and wait for the loop to exhaust max_attempts (which is 4) and return the 429 error!
    let mock_429 = server
        .mock(
            "POST",
            "/v1/projects/my-project/locations/us/processors/my-processor:process?key=fake-key",
        )
        .match_header("content-type", "application/json")
        .with_status(429)
        .with_body("Too Many Requests")
        .expect(16)
        .create_async()
        .await;

    let _app_config = AppConfig::default();
    let config = DocumentAiConfig {
        project_id: "my-project".to_string(),
        location: "us".to_string(),
        processor_id: "my-processor".to_string(),
        api_key: "fake-key".to_string(), // use the API key branch
        ..DocumentAiConfig::default()
    };

    let mut app_config = AppConfig::default();
    app_config.passphrase = uuid::Uuid::new_v4().to_string(); // bypass cache
    let mut client = DocumentAiClient::new_for_test(config, &app_config, Some(server.url()));
    client.location = "us".to_string(); // override mock location from test builder

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

    // Bypass cache by injecting a unique UUID into the trailer
    doc.trailer
        .set("TestNonce", uuid::Uuid::new_v4().to_string());

    doc.save(&pdf_path).unwrap();

    // Call it
    let result = client.parse_entire_statement(&pdf_path, None).await;

    if let Err(e) = &result {
        println!("Error returned: {:?}", e);
    }

    assert!(result.is_err());
    mock_429.assert_async().await;
}
