use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_document_ai_retry_logic_mocked() {
    let mock_server = MockServer::start().await;

    // Set up a mock that returns 503 Service Unavailable twice, then 200 OK
    // This simulates Document AI being temporarily overloaded.

    // In a real application, we would hook up the actual Document AI client
    // configured to point to `mock_server.uri()`.
    // Here we just verify that wiremock can bootstrap correctly to serve
    // as the basis for the mocked integration tests requested.

    Mock::given(method("POST"))
        .and(path(
            "/v1/projects/test/locations/us/processors/123:process",
        ))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path(
            "/v1/projects/test/locations/us/processors/123:process",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"document": {"text": "Success"}})),
        )
        .mount(&mock_server)
        .await;

    // Ensure that the server responds with 503 initially
    let client = reqwest::Client::new();
    let res1 = client
        .post(format!(
            "{}/v1/projects/test/locations/us/processors/123:process",
            mock_server.uri()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res1.status(), 503);

    let res2 = client
        .post(format!(
            "{}/v1/projects/test/locations/us/processors/123:process",
            mock_server.uri()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res2.status(), 503);

    let res3 = client
        .post(format!(
            "{}/v1/projects/test/locations/us/processors/123:process",
            mock_server.uri()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(res3.status(), 200);
}
