//! Parser-chain plumbing extracted from the runtime monolith.
//!
//! This module is **properly declared** by `src/app/runtime.rs`
//! (`mod parser_chain;`), so everything here is compiled and referenced.
//! Unlike the historical dead fork that once lived undeclared in this
//! directory, this file is part of the live build.

/// Router mapping pending interactive-fallback request ids to their one-shot
/// response channels.
pub(crate) type InteractiveFallbackRouter = std::sync::Arc<
    tokio::sync::Mutex<std::collections::HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<String>>>,
>;

/// Fallback order for document extraction: the selected parser first, then
/// progressively more-local fallbacks. Every chain ends offline-safe.
pub(crate) fn extraction_provider_order(
    selected: crate::app::config::DocumentParserMode,
) -> Vec<crate::app::config::DocumentParserMode> {
    use crate::app::config::DocumentParserMode;
    match selected {
        DocumentParserMode::OfflineHeuristic => vec![DocumentParserMode::OfflineHeuristic],
        DocumentParserMode::LlamaParse => vec![
            DocumentParserMode::LlamaParse,
            DocumentParserMode::OfflineHeuristic,
        ],
        DocumentParserMode::DocumentAi => vec![
            DocumentParserMode::DocumentAi,
            DocumentParserMode::OfflineHeuristic,
        ],
        DocumentParserMode::LocalOcrs => vec![DocumentParserMode::LocalOcrs],
        DocumentParserMode::Reducto => vec![
            DocumentParserMode::Reducto,
            DocumentParserMode::OfflineHeuristic,
        ],
    }
}

/// Waits for the user's interactive choice, removing stale routes on timeout
/// or channel close so the router never leaks entries.
pub(crate) async fn wait_for_interactive_choice(
    router: &InteractiveFallbackRouter,
    request_id: uuid::Uuid,
    receiver: tokio::sync::oneshot::Receiver<String>,
    timeout: std::time::Duration,
) -> Result<String, &'static str> {
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(choice)) => Ok(choice),
        Ok(Err(_)) => {
            router.lock().await.remove(&request_id);
            Err("response channel closed")
        }
        Err(_) => {
            router.lock().await.remove(&request_id);
            Err("interactive response timed out")
        }
    }
}

/// Offers the user interactive fallback choices for document parsing and
/// evaluates to `Option<DocumentParserMode>`: the next parser to try, or
/// `None` when the workflow should stop.
///
/// # Expansion-site requirements
///
/// This macro is textually expanded inside the `WorkflowParseAndValidate`
/// loop in `src/app/runtime.rs`. Macro hygiene resolves local variables at
/// the *definition* site, so every run-scoped value is an explicit parameter:
///
/// - `$allow_offline` — the run's `ignore_offline_fallback: bool`.
/// - `JobResult` — resolved via the expansion site's imports (`runtime.rs`).
///
/// Everything else is either a macro parameter or a fully qualified path.
macro_rules! interactive_fallback_or_continue {
    (
        $cfg:expr,
        $router:expr,
        $res_tx:expr,
        $err:expr,
        $next_parser:expr,
        $allow_offline:expr $(,)?
    ) => {{
        if $cfg.interactive_fallbacks && $res_tx.is_interactive() {
            let mut req = crate::engine::interactive_fallback::InteractiveFallbackRequest::new(
                "Document Parsing",
                $err.to_string(),
            );

            if $cfg.document_ai.is_some() {
                req = req.add_alternative("document_ai", "Try Document AI Again", None);
            }
            if $cfg.llamaparse_api_key.is_some() {
                req = req.add_alternative("llamaparse", "Try LlamaParse", None);
            }
            if $allow_offline {
                req = req.add_alternative(
                    "offline_parser",
                    "Fall back to Offline Parser (Local)",
                    None,
                );
            }
            req = req.add_alternative("cancel", "Cancel Workflow", None);

            let (tx, rx) = tokio::sync::oneshot::channel();
            let request_id = req.id;
            {
                let mut map = $router.lock().await;
                map.insert(request_id, tx);
            }
            let _ = $res_tx.send(JobResult::InteractiveFallbackRequired(req));
            let choice = wait_for_interactive_choice(
                &$router,
                request_id,
                rx,
                std::time::Duration::from_secs(300),
            )
            .await
            .unwrap_or_else(|reason| {
                tracing::warn!("[parser] Interactive fallback {reason}; cancelling workflow");
                "cancel".to_string()
            });
            match choice.as_str() {
                "document_ai" => Some(crate::app::config::DocumentParserMode::DocumentAi),
                "llamaparse" => Some(crate::app::config::DocumentParserMode::LlamaParse),
                "offline_parser" => Some(crate::app::config::DocumentParserMode::OfflineHeuristic),
                _ => None,
            }
        } else if $next_parser.is_some() && $allow_offline {
            Some(crate::app::config::DocumentParserMode::OfflineHeuristic)
        } else {
            None
        }
    }};
}

pub(crate) use interactive_fallback_or_continue;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::app::config::DocumentParserMode;

    #[test]
    fn every_provider_chain_ends_offline_safe_except_explicit_local_ocr() {
        for selected in [
            DocumentParserMode::Reducto,
            DocumentParserMode::DocumentAi,
            DocumentParserMode::LlamaParse,
            DocumentParserMode::OfflineHeuristic,
        ] {
            let order = extraction_provider_order(selected);
            assert_eq!(
                order.last(),
                Some(&DocumentParserMode::OfflineHeuristic),
                "{selected:?} chain must terminate at the offline parser"
            );
            assert_eq!(order.first(), Some(&selected));
        }
        // Local OCR is explicitly unsupported-without-models: it fails fast
        // rather than silently degrading.
        assert_eq!(
            extraction_provider_order(DocumentParserMode::LocalOcrs),
            vec![DocumentParserMode::LocalOcrs]
        );
    }
}
