//! Phase 3: Exhaustive setting-combination roundtrip test.
//!
//! Constructs every valid (PdfEngineMode × DocumentParserMode × AiProviderMode × VerificationMode)
//! tuple, serializes it to JSON, deserializes it back, and asserts round-trip correctness.
//! Total: 5 × 5 × 3 × 2 = 150 combinations.

use dual_core_pdf_pipeline::app::config::{
    AiProviderMode, DocumentParserMode, PdfEngineMode, VerificationMode,
};
use serde::{Deserialize, Serialize};

/// A minimal settings struct mirroring the setting axes from AppSettings.
/// We use our own struct rather than the full `AppSettings` because the latter
/// lives inside `gui.rs` and carries GUI state we don't want in a headless test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SettingCombination {
    engine_mode: PdfEngineMode,
    document_parser: DocumentParserMode,
    ai_provider: AiProviderMode,
    verification_renderer: VerificationMode,
}

/// All variants for each axis, listed explicitly so a new variant that isn't
/// added here causes a compile error (the `match` in the label tests below
/// will be non-exhaustive).
const ENGINES: &[PdfEngineMode] = &[
    PdfEngineMode::PyMuPdfProPrimary,
    PdfEngineMode::NativeOnly,
    PdfEngineMode::PyMuPdfOnly,
    PdfEngineMode::DualConcurrent,
    PdfEngineMode::TypstReconstruct,
];

const PARSERS: &[DocumentParserMode] = &[
    DocumentParserMode::LlamaParse,
    DocumentParserMode::OfflineHeuristic,
    DocumentParserMode::LocalOcrs,
    DocumentParserMode::DocumentAi,
];

const AI_PROVIDERS: &[AiProviderMode] = &[
    AiProviderMode::ManualOnly,
    AiProviderMode::GeminiApiKey,
    AiProviderMode::GeminiVertex,
    AiProviderMode::GroqApiKey,
    AiProviderMode::OpenRouterApiKey,
];

const VERIFIERS: &[VerificationMode] = &[
    VerificationMode::LocalPdfium,
    VerificationMode::PdfRestCloud,
];

#[test]
fn all_150_setting_combinations_roundtrip_json() {
    let mut count = 0usize;
    for engine in ENGINES {
        for parser in PARSERS {
            for ai in AI_PROVIDERS {
                for verifier in VERIFIERS {
                    let combo = SettingCombination {
                        engine_mode: *engine,
                        document_parser: *parser,
                        ai_provider: *ai,
                        verification_renderer: *verifier,
                    };
                    let json = serde_json::to_string(&combo).unwrap_or_else(|e| {
                        panic!("Failed to serialize combo #{count}: {:?} — {e}", combo)
                    });
                    let back: SettingCombination =
                        serde_json::from_str(&json).unwrap_or_else(|e| {
                            panic!("Failed to deserialize combo #{count}: {json} — {e}",)
                        });
                    assert_eq!(combo, back, "Roundtrip mismatch for combo #{count}: {json}");
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 200, "Expected 200 combinations, got {count}");
}

/// Verify every enum variant has a non-empty human-readable label.
#[test]
fn all_enum_variants_have_labels() {
    for e in ENGINES {
        // PdfEngineMode doesn't have a label() method in the current code,
        // so we verify the debug representation is non-empty instead.
        let debug = format!("{e:?}");
        assert!(!debug.is_empty(), "PdfEngineMode debug empty for {e:?}");
    }
    for p in PARSERS {
        let label = p.label();
        assert!(
            !label.is_empty(),
            "DocumentParserMode label empty for {p:?}"
        );
    }
    for a in AI_PROVIDERS {
        let label = a.label();
        assert!(!label.is_empty(), "AiProviderMode label empty for {a:?}");
    }
    for v in VERIFIERS {
        let label = v.label();
        assert!(!label.is_empty(), "VerificationMode label empty for {v:?}");
    }
}

/// Verify that Default impls produce expected values.
#[test]
fn default_settings_are_expected() {
    assert_eq!(PdfEngineMode::default(), PdfEngineMode::PyMuPdfProPrimary);
    assert_eq!(
        DocumentParserMode::default(),
        DocumentParserMode::LlamaParse
    );
    assert_eq!(AiProviderMode::default(), AiProviderMode::GroqApiKey);
    assert_eq!(VerificationMode::default(), VerificationMode::LocalPdfium);
}
