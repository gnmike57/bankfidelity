# Tests — Bank Statement Fidelity Editor

## Test Categories

Tests are organised into tiers by their external prerequisites.
Only **Tier 1** tests are expected to pass in every environment.

### Tier 1 — Pure Logic (No External Dependencies)

These tests exercise in-memory data structures and algorithms with zero
filesystem, network, or runtime requirements. They must always pass.

| Test File | Purpose |
|---|---|
| `segment_map_unit.rs` | Unit tests for `SegmentMap` page mapping (resolve, to_global, group_edits, ordered_merge_paths) |
| `segment_map_props.rs` | Property-based tests (proptest) for bidirectional page mapping roundtrip |
| `segment_mapping.rs` | Additional proptest coverage for segment resolve/to_global |
| `setting_combinations.rs` | 250-combination JSON roundtrip for all setting enum axes |
| `static_analysis.rs` | Verifies `pdf_split_merge.rs` contains no PyMuPDF/PyO3 references |

```bash
cargo test --test segment_map_unit --test segment_map_props --test segment_mapping --test setting_combinations --test static_analysis
```

### Tier 2 — Local Runtime (Requires Python + PyMuPDF)

These tests spin up the `Runtime` (which initialises the Python actor thread)
and may read test PDF files from `examples/` or generate synthetic ones.

| Test File | Requirements | Self-skip |
|---|---|---|
| `runtime_smoke.rs` | Python + PyMuPDF | No — always runs (Ping/Pong path) |
| `transparency.rs` | `examples/sample.pdf` | Yes — logs `[skip]` when PDF missing |
| `split_merge_fidelity.rs` | `examples/sample.pdf` | Yes — logs `[skip]` when PDF missing |
| `chaos_fallback.rs` | None (uses mock engine) | No |
| `server_e2e.rs` | Free TCP port (auto-assigned) | No |
| `e2e_segmented_pipeline.rs` | `AU Bank Statements/*.pdf` | Yes — logs skip when dir missing |
| `font_cascade.rs` | System TTF fonts + Python + PyMuPDF | Yes — multi-stage skip |

```bash
cargo test --test runtime_smoke --test chaos_fallback --test server_e2e
```

### Tier 3 — API Integration (Requires External Services)

These tests call real external APIs and must be run manually with `--ignored`.
They require API keys configured in `.env`.

| Test File | Requirements | Run Command |
|---|---|---|
| `workflow_e2e.rs` | AU PDF + Document AI + Gemini | `cargo test --test workflow_e2e -- --ignored --nocapture` |
| `au_statements_deep_dive.rs` | AU PDFs + Document AI + Gemini | `cargo test --test au_statements_deep_dive -- --ignored --nocapture` |
| `au_transfer_stress.rs` | AU PDFs + all API keys | `cargo test --test au_transfer_stress -- --ignored --nocapture` |
| `ai_live.rs` | Gemini + Document AI keys | `cargo test --test ai_live -- --ignored --nocapture` |

### Tier 4 — CLI E2E

CLI tests that build the binary and run subcommands. Require `cargo build`
to succeed. Most self-skip when test PDFs are absent.

| Test File | Requirements |
|---|---|
| `e2e_engine_tests.rs` | `test_doc.pdf` or `examples/sample.pdf` for PDF-dependent tests |

## Fixtures

The `fixtures/` directory contains shared test utilities:

- `fixtures/mod.rs` — `generate_test_pdf(pages, path)` creates minimal
  synthetic PDFs for testing without depending on checked-in sample files.

## Running All Tests

```bash
# Fast: pure logic only (~5s)
cargo test --test segment_map_unit --test segment_map_props --test segment_mapping --test setting_combinations --test static_analysis

# Standard: all non-ignored tests
cargo test --no-fail-fast

# Full: including API-dependent tests (requires .env)
cargo test --no-fail-fast -- --include-ignored --nocapture
```
