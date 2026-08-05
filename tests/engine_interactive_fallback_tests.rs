use dual_core_pdf_pipeline::engine::interactive_fallback::{
    FallbackAlternative, InteractiveFallbackRequest,
};

#[test]
fn test_interactive_fallback_request_builder() {
    let req = InteractiveFallbackRequest::new("Document Parsing", "API rate limit exceeded")
        .add_alternative(
            "mindee",
            "Try Mindee API",
            Some("Use the Mindee cloud service for parsing".to_string()),
        )
        .add_alternative("offline", "Use Offline Parser", None);

    assert_eq!(req.stage, "Document Parsing");
    assert_eq!(req.error_details, "API rate limit exceeded");
    assert_eq!(req.alternatives.len(), 2);

    assert_eq!(
        req.alternatives[0],
        FallbackAlternative {
            id: "mindee".to_string(),
            label: "Try Mindee API".to_string(),
            description: Some("Use the Mindee cloud service for parsing".to_string()),
        }
    );

    assert_eq!(
        req.alternatives[1],
        FallbackAlternative {
            id: "offline".to_string(),
            label: "Use Offline Parser".to_string(),
            description: None,
        }
    );
}
