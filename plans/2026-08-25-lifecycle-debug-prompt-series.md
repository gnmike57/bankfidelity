# BankFidelity — Maximum-Effort Lifecycle Debug & Hardening Prompt Series

Date: 2026-08-25 · Mode: Debug · Scope: full user lifecycle, cold start → certification

---

## HOW TO USE THIS SERIES (read once, then never again)

**Optimal count: 4 prompts.** Rationale: the lifecycle decomposes into four domains that are each
large enough to saturate one maximum-effort agent session (investigation + fixes + regression tests +
validation gate), while staying small enough that nothing gets deferred or shallow-executed. Three
prompts would force two domains to share a session and get truncated; five would fragment related
work and re-pay context-gathering costs.

| # | Prompt | Lifecycle domain | Report artifact |
|---|--------|------------------|-----------------|
| 1 | Launch & Orchestration | Desktop shortcut → Windows launchers → env/config → local Qwen → MCP bridge → UFO execution → GUI operational readiness | [plans/2026-08-25-phase1-launch-orchestration-report.md](2026-08-25-phase1-launch-orchestration-report.md) |
| 2 | Fidelity Editing Core | Corpus ground truth → parser chain & fallback law → font/typography replication → edit-engine tiers → structural integrity | [plans/2026-08-25-phase2-fidelity-editing-report.md](2026-08-25-phase2-fidelity-editing-report.md) |
| 3 | Ledger Truth & Visual Proof | Balance engine exactness → verification gates & evidence ledger → visual fidelity render/diff proof → adversarial robustness | [plans/2026-08-25-phase3-ledger-visual-report.md](2026-08-25-phase3-ledger-visual-report.md) |
| 4 | Transfer, Template Synthesis & Certification | Template intelligence → target template PDF synthesis → transfer matrix & round-trip truth → capstone full-lifecycle gauntlet → consolidation | [plans/2026-08-25-lifecycle-certification-report.md](2026-08-25-lifecycle-certification-report.md) |

**Protocol:** Send Prompt 1 alone. Let it run to its final report. Then send Prompt 2. Then 3. Then 4.
Each prompt is fully self-contained (constitution embedded) but each begins by reading the previous
phase reports, so knowledge compounds. Do not merge prompts; do not skip ahead.

**Context baseline:** all four prompts assume the verified facts in
[plans/2026-08-24-full-context-audit-and-master-prompt.md](2026-08-24-full-context-audit-and-master-prompt.md)
(Part 1 module map + Part 2 fault register). Every fault-register item is treated as *verify current
status first* — some may already be fixed.

---
---

# PROMPT 1 OF 4 — LAUNCH & ORCHESTRATION LIFECYCLE

> Copy everything from the next line down. Send alone. Wait for completion before Prompt 2.

---

**MISSION: Make the entire cold-start journey — double-click desktop shortcut → Windows launcher chain → environment/config boot → local Qwen LLM stack → MCP stdio handshake → UFO native desktop execution → BankFidelity GUI fully operational — work deterministically, fail loudly and actionably everywhere it can fail, and prove every link in that chain with executed evidence, not code reading alone. Diagnose before you fix. Fix durably. Pin every bug with a named regression test.**

You are working in `c:/bankfidelity/bankfidelity` — the Rust orchestrator half of BankFidelity
(`dual-core-pdf-pipeline` v2.0.0, egui GUI + CLI, tokio job runtime, rust_decimal balance engine,
multi-provider AI with mandatory offline fallbacks, MCP stdio bridge to Microsoft UFO at `C:\UFO`,
local Qwen 2.5 Coder 7B via OpenAI-compatible endpoint). OS: Windows 11, shell cmd.exe (use
PowerShell for PS1 work).

## READ FIRST

1. [AGENTS.md](../AGENTS.md) — rules, autonomy, secrets policy.
2. [plans/2026-08-24-full-context-audit-and-master-prompt.md](2026-08-24-full-context-audit-and-master-prompt.md) — Part 1 architecture facts, Part 2 fault register (P0-2, P0-3, P1-4, P1-5, P1-9 are most likely alive in YOUR domain).
3. [findings_register.md](../findings_register.md) — prior findings; verify status, don't trust staleness.
4. The bankfidelity project skill (`.agents/skills/bankfidelity/SKILL.md`) — MCP tool/resource contract.

## NON-NEGOTIABLE CONSTITUTION

1. **SINGLE LIVE RUNTIME:** [src/app/runtime.rs](../src/app/runtime.rs) is the ONLY live runtime module; its only submodule is [src/app/runtime/parser_chain.rs](../src/app/runtime/parser_chain.rs). Never treat any other historical runtime fork file as a reference for current behavior. [tests/static_analysis.rs](../tests/static_analysis.rs) guards this — keep it green.
2. **SECRETS:** Never print, copy, rewrite, or commit secret values. Report only set/missing status (e.g., "GEMINI_API_KEY is set"). Read [.env.example](../.env.example), never `.env`. If a variable is missing, fix docs/.env.example instead of inventing values.
3. **FALLBACK CHAIN LAW:** cloud parsers → offline_parser; AI balance → local engine; cloud render → local Pdfium; visual AI → SSIM-only; PyMuPDF edit → Pdfium → Typst reconstruct. Exception: TransferTransactions mapping requires Gemini/Groq/OpenRouter (not your domain this session, but never break the law).
4. **TOOLCHAIN:** [rust-toolchain.toml](../rust-toolchain.toml) channel must be `1.89.0` with rustfmt+clippy components. If it says `dev`, replace it, run `rustup install 1.89.0` then `rustup override unset`.
5. **CONFIRMATION REQUIRED BEFORE:** touching `.env`/secrets/private PDFs/real banking data/generated audit or output PDFs/git history/push/commit/reset, recursive deletes, or commands spending meaningful API credits. Everything else: proceed autonomously.
6. **DIAGNOSE-BEFORE-FIX:** For every observed fault, enumerate ≥5 candidate sources across (toolchain/setup, dependency, compile, logic, config/env, external service, filesystem permissions, async/concurrency, data/format assumption); distill to ≤2 most probable using evidence; add temporary instrumentation (logs/tests) to validate; state the confirmed root cause to the user BEFORE the durable fix when the fix is non-obvious or behavior-changing. Trivial mechanical breaks (missing file, wrong path, typo) may be fixed immediately with evidence noted.
7. **DURABLE FIXES ONLY:** typed errors, context-rich messages, fail-fast validation before expensive work, no silent swallows, no unchecked unwraps in production paths. Every fixed bug gets a named regression test.
8. **VALIDATION GATE:** `cargo fmt && cargo check && cargo test && cargo clippy --all-targets --all-features -- -D warnings` must pass before you declare any phase complete. Run targeted tests first for speed; full gate at phase end.
9. **REPORTING:** Per AGENTS.md format — root cause / files changed / commands run / validation result / remaining manual steps — written to [plans/2026-08-25-phase1-launch-orchestration-report.md](2026-08-25-phase1-launch-orchestration-report.md), plus new rows appended to [findings_register.md](../findings_register.md).

## PHASED OPERATION PLAN (execute in order; keep a running findings table)

### Phase A — Toolchain & artifact reality check
- Record `rustc -vV` host triple and `cargo --version`. Confirm effective channel is 1.89.0.
- `cargo build --release`. Identify the ACTUAL produced binary path (which target-triple directory?).
- Compare against [build_and_shortcut.ps1](../build_and_shortcut.ps1), which hardcodes `target\x86_64-pc-windows-gnu\release\dual-core-pdf-pipeline.exe` with Arguments `"gui"` and WorkingDirectory repo root. **If your host builds to a different triple (e.g., msvc), the desktop shortcut is broken by construction — this is a prime suspect.** Fix the script to resolve the target dir dynamically (`cargo metadata`/`rustc -vV`), regenerate `BankFidelity.lnk` on the Desktop, and verify the .lnk's TargetPath resolves to an existing exe.
- Flag version coherence of [BankStatementFidelityEditor-v2.0.0-windows-x86_64.msi](../BankStatementFidelityEditor-v2.0.0-windows-x86_64.msi) vs `CARGO_PKG_VERSION` (report only; rebuild only if trivially scriptable).

### Phase B — Launcher chain integrity ([desktop_launchers/](../desktop_launchers))
- Static-walk every launcher: [00_Master_Launchpad.bat](../desktop_launchers/00_Master_Launchpad.bat) (menu items 1–7 must map to existing sibling files), [01_BankFidelity_Terminal.bat](../desktop_launchers/01_BankFidelity_Terminal.bat), [02_UFO_Control_Panel.bat](../desktop_launchers/02_UFO_Control_Panel.bat), [03_AI_Dream_Team_Launcher.bat](../desktop_launchers/03_AI_Dream_Team_Launcher.bat), [04_E2E_Diagnostics.bat](../desktop_launchers/04_E2E_Diagnostics.bat), [05_UFO_Admin_Terminal.bat](../desktop_launchers/05_UFO_Admin_Terminal.bat), [05_Launch_Interactive_Admin_Terminal.ps1](../desktop_launchers/05_Launch_Interactive_Admin_Terminal.ps1), [06_Run_UFO_E2E_Test.bat](../desktop_launchers/06_Run_UFO_E2E_Test.bat) (elevates via UAC, then `cd /d C:\ufo\ufo` + `scripts\smoke_test_e2e.bat` — verify both targets exist), [07_Configuration_Dashboard.bat](../desktop_launchers/07_Configuration_Dashboard.bat), [BankFidelity_Matrix.ps1](../desktop_launchers/BankFidelity_Matrix.ps1).
- Execute the safe subset (Matrix, 04 diagnostics, 07 dashboard) and capture exit codes/output.
- Fix broken launchers. Standard: every launcher either works or prints an actionable error and pauses — never silently closes.

### Phase C — Environment & config boot
- Diff [.env.example](../.env.example) against expectations encoded in [src/app/env_spec.rs](../src/app/env_spec.rs); report set/missing only for actual env.
- Run `cargo run --release -- doctor` and `cargo run --release -- verify-api-keys`; tabulate results.
- Verify boot-time availability detection ([src/app/config.rs](../src/app/config.rs) `ApiAvailability`) matches reality with keys absent (manual-only mode advertised correctly in UI/backend preferences per [src/app/modals.rs](../src/app/modals.rs)).
- **Fault-register check P0-2:** if `AppConfig::default()` still ships `passphrase: "DEV_PASSPHRASE"`, replace with empty default + loud fail-fast invariant on production paths ("DUAL_CORE_PASSPHRASE not set"), keep dev-mode shortening intact, add test: default config must NOT validate in non-dev mode. Root of trust context: [src/security/software_root.rs](../src/security/software_root.rs).
- **Font bootstrap offline resilience:** [src/app/fontcache.rs](../src/app/fontcache.rs) pins GitHub raw URLs by commit. Prove (unit test or offline simulation) that first-run with no network degrades gracefully to bundled [assets/Inter-Regular.ttf](../assets/Inter-Regular.ttf)/[assets/Inter-Bold.ttf](../assets/Inter-Bold.ttf)/system fonts — GUI must still boot, no panic. Fix if air-gap bricks startup.

### Phase D — Local LLM stack (Qwen 2.5 Coder 7B)
- Probe both candidate endpoints: Ollama `http://127.0.0.1:11434` (`GET /api/tags`) and llama-server `http://127.0.0.1:8080/v1/models` (see [scripts/start_llama_server.ps1](../scripts/start_llama_server.ps1)).
- **Fault-register check P1-5:** [src/ai/local_llm.rs](../src/ai/local_llm.rs) defaults to `:8080/v1` while skill/docs advertise `:11434`. Decide ONE story (prefer matching whatever stack actually runs on this machine; else align docs to code), then make default + `LOCAL_LLM_URL` + [docs/ADR-0004-Local-LLM-Support.md](../docs/ADR-0004-Local-LLM-Support.md) + [docs/NLP_CHAT.md](../docs/NLP_CHAT.md) + [QUICKSTART.md](../QUICKSTART.md) agree. UFO configs (`agents.yaml`, `system.yaml`, `mcp.yaml` under `C:\UFO\ufo`) must target `qwen2.5-coder-7b-instruct-q4_k_m`.
- Functional test with model up: `cargo run -- chat -i "AU Bank Statements/anz_example.pdf" "list all transactions"` — record latency budget. With model down: command must degrade gracefully within timeout (clear manual-only message; no hang, no panic). Fix [src/app/nlp_router.rs](../src/app/nlp_router.rs) failure path if it hangs.

### Phase E — MCP bridge hardening & handshake proof
- Inspect [install_mcp.ps1](../install_mcp.ps1) and `C:\UFO\ufo\client\mcp\configs\bankfidelity.json`: correct binary path/args; API keys piped dynamically, never stored in the JSON.
- Drive the stdio server ([src/ai/mcp.rs](../src/ai/mcp.rs)) directly with framed JSON-RPC: `initialize` → `tools/list` (must expose `balance_statement`, `modify_text`, `extract_data`, `verify_layout`) → `prompts/get` `bankfidelity_agent_instructions` (4 directives present) → `resources/list` (active task.md/walkthrough.md via brain dir) → `resources/read` `pdf-page://<absolute path to AU sample>?page=1`.
- **Fault-register check P0-3:** broken-pipe chaos test — close the reader mid-session; assert logged error + clean shutdown, NO panic. If `serde_json::to_string(...).unwrap()` / `writeln!(...).expect(...)` remain in the request loop, replace with non-panicking helpers (JSON-RPC error envelope fallback). Set `serverInfo.version` from `env!("CARGO_PKG_VERSION")`; use a real supported protocolVersion constant (not fabricated `"2026-07-28"`). Add regression test simulating closed stdout.
- Note: [scripts/mcp_server.py](../scripts/mcp_server.py) exists — determine whether it is a duplicate/legacy surface; if so mark deprecated in docs (do not delete without confirmation).

### Phase F — UFO native execution proof
- Verify `C:\UFO\ufo\python_env\python.exe` exists (as expected by [build_and_shortcut.ps1](../build_and_shortcut.ps1)); if missing, follow [scripts/setup_ufo.ps1](../scripts/setup_ufo.ps1) guidance and report.
- Run the elevated E2E smoke ([desktop_launchers/06_Run_UFO_E2E_Test.bat](../desktop_launchers/06_Run_UFO_E2E_Test.bat) → `C:\ufo\ufo\scripts\smoke_test_e2e.bat`). Success criteria: Notepad automation completes AND at least one MCP-dispatched BankFidelity tool call succeeds end-to-end. Capture transcript + screenshots as evidence.
- Produce a failure taxonomy table: python_env missing / model unreachable / MCP timeout / UAC denied / DPI-scaling mis-clicks — each mapped to its actionable error message or fix.

### Phase G — GUI operational readiness (the real user path)
- Launch EXCLUSIVELY via the regenerated Desktop shortcut (not `cargo run`). Measure time-to-window; confirm theme loads and empty state is safe ([src/app/gui.rs](../src/app/gui.rs), [src/app/theme.rs](../src/app/theme.rs)).
- Open an AU sample; with all cloud keys absent force the parser chain → interactive fallback modal must appear ([src/app/modals.rs](../src/app/modals.rs), [src/engine/interactive_fallback.rs](../src/engine/interactive_fallback.rs)); choose offline; job completes.
- Cancellation discipline: cancel mid-job; must terminate ≤2 s (**fault-register check P1-9:** busy-wait poll ~[src/app/runtime.rs:1623](../src/app/runtime.rs) — replace with condvar/channel wake if alive). Also check `block_in_place`+`block_on` (~line 6181) cannot run under current-thread runtime.
- Watchdog behavior on simulated hang ([src/app/watchdog.rs](../src/app/watchdog.rs)); preflight checks ([src/app/preflight.rs](../src/app/preflight.rs)) surface before expensive work.
- Evidence pack: screenshots via [take_screenshots.ps1](../take_screenshots.ps1), timings, log excerpts → `audit-evidence/lifecycle-phase1/`.

### Phase H — Targeted validation & report
- Targeted: `cargo test --test static_analysis --test runtime_smoke --test cli_startup_contract --test launch_fallback_test --test gui_app_state_tests`.
- Full gate (Constitution #8). Write the phase report; update findings register. End with explicit HANDOFF note stating what Prompt 2 may assume (toolchain healthy, shortcut works, config boots, MCP unpanickable, model endpoint decided).

---
---

# PROMPT 2 OF 4 — FIDELITY EDITING CORE

> Copy everything from the next line down. Send after Prompt 1 completed.

---

**MISSION: Make BankFidelity's editing core provably faithful — parsing every AU statement format deterministically, replicating typography at glyph level, applying edits through the correct engine tier with byte-level idempotency, and surviving malformed input without corruption. Every claim backed by executed extraction baselines, glyph-diff evidence, and green tests. Diagnose before you fix; pin every bug with a named regression test.**

Same workspace, same constitution as Prompt 1 (repeated below — it binds you fully).

## READ FIRST

1. [AGENTS.md](../AGENTS.md); bankfidelity project skill.
2. [plans/2026-08-25-phase1-launch-orchestration-report.md](2026-08-25-phase1-launch-orchestration-report.md) — build/toolchain state, known-live faults carried forward.
3. [plans/2026-08-24-full-context-audit-and-master-prompt.md](2026-08-24-full-context-audit-and-master-prompt.md) Part 1–2.
4. [findings_register.md](../findings_register.md).

## NON-NEGOTIABLE CONSTITUTION

(Identical to Prompt 1 §NON-NEGOTIABLE CONSTITUTION, items 1–9. It applies in full: single live runtime [src/app/runtime.rs](../src/app/runtime.rs) + [src/app/runtime/parser_chain.rs](../src/app/runtime/parser_chain.rs) only; secrets set/missing-only; fallback chain law; toolchain 1.89.0; confirmation list; diagnose-before-fix ≥5 hypotheses → ≤2 → instrument → confirm; durable fixes + named regression tests; validation gate `cargo fmt && cargo check && cargo test && cargo clippy --all-targets --all-features -- -D warnings`; AGENTS.md reporting into [plans/2026-08-25-phase2-fidelity-editing-report.md](2026-08-25-phase2-fidelity-editing-report.md) + [findings_register.md](../findings_register.md).)

## PHASED OPERATION PLAN

### Phase A — Corpus & ground-truth lock
- Inventory corpora: [AU Bank Statements/](../AU%20Bank%20Statements) (anz, bankwest, commbank_smartaccess, ing_orangeeveryday, macquarie, westpac_choicebasic, fallback.pdf, STATEMENT_example.zip), [examples/](../examples) (sample, mixed_content, multi_page_fonts), [anz_page_1.pdf](../anz_page_1.pdf), [tests/stress_pdfs/](../tests/stress_pdfs) incl. `test1..test4_ground_truth.json`, [tests/fixtures/synthetic_au_statement.pdf](../tests/fixtures/synthetic_au_statement.pdf).
- Produce deterministic extraction baselines per document using the bins: `cargo run --bin dump_text -- <pdf>` and `cargo run --bin validate -- <pdf>`; store JSON baselines under `audit-evidence/lifecycle-phase2/baselines/`. These become the regression floor for Phases B–D.
- Use [scripts/inspect_pdfs.py](../scripts/inspect_pdfs.py), [scripts/inspect_au_columns.py](../scripts/inspect_au_columns.py), [scripts/learn_au_templates.py](../scripts/learn_au_templates.py) to record per-bank column geometry facts.

### Phase B — Parser chain determinism & fallback law
- Keys-absent run over every corpus document: chain must degrade Reducto → Document AI → LlamaParse → OfflineHeuristic with per-stage timing logs; interactive fallback modal path and the 300 s choice timeout auto-continue verified ([src/engine/interactive_fallback.rs](../src/engine/interactive_fallback.rs), [src/app/modals.rs](../src/app/modals.rs), chain loop in [src/app/runtime.rs](../src/app/runtime.rs)/[src/app/runtime/parser_chain.rs](../src/app/runtime/parser_chain.rs)).
- Adapter contract tests with mocks stay green and gain coverage where thin: [tests/ai_backend_tests.rs](../tests/ai_backend_tests.rs), [tests/ai_mock_tests.rs](../tests/ai_mock_tests.rs), [tests/cascade_fallback.rs](../tests/cascade_fallback.rs), [tests/chaos_fallback.rs](../tests/chaos_fallback.rs).
- Offline parse accuracy vs ground truth per bank, fixing heuristics with committed fixtures for EVERY misparse: pay special attention to bankwest inline-year dates (see [python/test_bankwest_inline_year_dates.py](../python/test_bankwest_inline_year_dates.py)), commbank embedded-font reload ([python/test_commbank_embedded_reload.py](../python/test_commbank_embedded_reload.py)), multiline transaction rows ([python/test_multiline_transactions.py](../python/test_multiline_transactions.py)). Zero panics — [src/engine/offline_parser.rs](../src/engine/offline_parser.rs) catch_unwind stays.
- Consensus sanity ([src/engine/consensus.rs](../src/engine/consensus.rs)): craft a synthetic provider-disagreement case; ranking must be sensible and deterministic ([tests/consensus_matrix_tests.rs](../tests/consensus_matrix_tests.rs)).

### Phase C — Typography & font replication fidelity
- Per AU sample: font inventory ([src/engine/font_analysis.rs](../src/engine/font_analysis.rs)) → metrics ([src/engine/font_metrics.rs](../src/engine/font_metrics.rs)) → replication plan ([src/engine/font_replication.rs](../src/engine/font_replication.rs) + [python/font_replicator.py](../python/font_replicator.py)) → shaping correctness ([src/engine/font_shaping.rs](../src/engine/font_shaping.rs)). Inter fallback covers missing glyphs; charset coverage reported per document.
- Glyph-level round-trip proof: controlled `modify_text` on a non-transaction label; extract bboxes pre/post via [get_bbox.py](../get_bbox.py); assert Δposition ≤ documented epsilon; produce overlay diff PNGs under `audit-evidence/lifecycle-phase2/typography/`.
- OCR assist tier exercised: rasterize one page → [src/extractors/ocrs_engine.rs](../src/extractors/ocrs_engine.rs) with [models/text-detection.rten](../models/text-detection.rten)/[models/text-recognition.rten](../models/text-recognition.rten) → merged sensibly via [src/extractors/merger.rs](../src/extractors/merger.rs).

### Phase D — Edit-engine tiers & selector law
- Enforce selector rank PyMuPDF Pro → Native (Pdfium/lopdf) → Typst reconstruct ([src/pdf/selector.rs](../src/pdf/selector.rs)). Forced-degradation matrix (PRO key present/absent × engine availability) picks the correct tier every time ([tests/ranking_test.rs](../tests/ranking_test.rs)).
- Equivalence: identical edit via [src/pdf/pymupdf_engine.rs](../src/pdf/pymupdf_engine.rs) vs [src/pdf/native_engine.rs](../src/pdf/native_engine.rs) vs [src/engine/typst_engine.rs](../src/engine/typst_engine.rs) on the same source → outputs within documented tolerance; byte-idempotency (apply twice == apply once) per engine.
- Python worker contract end-to-end: [python/bridge_protocol.py](../python/bridge_protocol.py) ↔ [src/ai/python_protocol.rs](../src/ai/python_protocol.rs)/[src/ai/python_worker.rs](../src/ai/python_worker.rs); all python contract tests green — chunk resource preservation ([python/test_chunk_pdf_resource_preservation.py](../python/test_chunk_pdf_resource_preservation.py)), one-byte inplace stream ([python/test_one_byte_inplace_stream.py](../python/test_one_byte_inplace_stream.py)), profiled Type0 resources ([python/test_bankwest_profiled_type0_resource.py](../python/test_bankwest_profiled_type0_resource.py), [python/test_profiled_type0_resource.py](../python/test_profiled_type0_resource.py)), standard-14 fallback ([python/test_bankwest_standard14_fallback.py](../python/test_bankwest_standard14_fallback.py)), apply-many contracts ([python/test_apply_many_deletions.py](../python/test_apply_many_deletions.py), [python/test_apply_many_edits_contract.py](../python/test_apply_many_edits_contract.py)). Failures fixed at protocol level — never by skipping tests.

### Phase E — Structural integrity under edit
- Split/merge fidelity on multi-page statements: [src/engine/pdf_split_merge.rs](../src/engine/pdf_split_merge.rs), [tests/split_merge_fidelity.rs](../tests/split_merge_fidelity.rs), [scripts/split_merge_fidelity_check.py](../scripts/split_merge_fidelity_check.py) — resource dictionaries preserved, no orphaned xrefs.
- Malformed inputs fail safe: [tests/malformed_pdf_tests.rs](../tests/malformed_pdf_tests.rs) extended with at least two new corruption vectors (truncated xref, cyclic resource graph).

### Phase F — Targeted validation & report
- Targeted: `cargo test --test au_statements_deep_dive --test e2e_engine_tests --test engine_font_analysis_tests --test engine_font_replication_tests --test engine_font_shaping_tests --test native_characterization --test ranking_test --test font_cascade --test malformed_pdf_tests --test split_merge_fidelity`.
- Full gate. Write [plans/2026-08-25-phase2-fidelity-editing-report.md](2026-08-25-phase2-fidelity-editing-report.md); update findings register. HANDOFF note: what Prompt 3 may assume (baselines exist, parser chain deterministic, engines equivalent & idempotent).

---
---

# PROMPT 3 OF 4 — LEDGER TRUTH & VISUAL PROOF

> Copy everything from the next line down. Send after Prompt 2 completed.

---

**MISSION: Make BankFidelity's numbers and pixels undeniable — the exact-decimal balance engine must reconcile every ledger to the cent with forensic explanations of any imbalance; the multi-gate verification system must trip on every class of tampering and produce crash-safe, reproducible cryptographic evidence; and visual fidelity must be PROVEN by rendered pixel/SSIM diffs, not asserted. Diagnose before you fix; pin every bug with a named regression test.**

Same workspace, same constitution (repeated below — it binds you fully).

## READ FIRST

1. [AGENTS.md](../AGENTS.md); bankfidelity project skill.
2. Phase reports 1–2: [plans/2026-08-25-phase1-launch-orchestration-report.md](2026-08-25-phase1-launch-orchestration-report.md), [plans/2026-08-25-phase2-fidelity-editing-report.md](2026-08-25-phase2-fidelity-editing-report.md).
3. [audit-evidence/RANKED_FIDELITY_REPORT.md](../audit-evidence/RANKED_FIDELITY_REPORT.md), [audit-evidence/visual-review-findings.md](../audit-evidence/visual-review-findings.md) — refresh targets.
4. [findings_register.md](../findings_register.md).

## NON-NEGOTIABLE CONSTITUTION

(Identical to Prompt 1 §NON-NEGOTIABLE CONSTITUTION, items 1–9. Reporting goes to [plans/2026-08-25-phase3-ledger-visual-report.md](2026-08-25-phase3-ledger-visual-report.md) + [findings_register.md](../findings_register.md). Additional: rendering via pdfrest spends credits — local Pdfium is the default; use [src/ai/pdfrest.rs](../src/ai/pdfrest.rs) only if key is set AND user confirms.)

## PHASED OPERATION PLAN

### Phase A — Balance Engine Exactness Campaign
- **Target Files**: `src/engine/balance.rs`, `src/engine/date_adjust.rs`, `src/engine/number_format.rs`, `src/engine/financial_nlp.rs`, `src/app/nlp_router.rs`.
- **Implementation Details**:
  - Implement property tests for randomized ledgers over `rust_decimal` to ensure opening/closing continuity invariants.
  - Implement strict date semantics (month-end rollover, AU DD/MM ambiguity, bankwest year-boundary quirks).
  - Ensure number format parsing handles `$`, commas, parentheses (negatives), CR/DR suffixes, and trailing minus.
  - Fix diagnostics granularity on `test1..test4_ground_truth.json` mismatches to emit precise per-line expected vs actual deltas (no vague "imbalance detected").
  - Wire forensic explainer to `qwen2.5-coder-7b-instruct-q4_k_m` (local LLM via `127.0.0.1:11434`), streaming into `egui`.
- **Verification Steps**: `cargo test --test stress_pdfs_balance_test`.
- **Failure Recovery**: If local Qwen LLM is down, gracefully fallback to a manual-only notice without blocking the edit session or crashing the GUI.

### Phase B — Verification Gates & Evidence Ledger
- **Target Files**: `src/engine/verification.rs`, `src/engine/verification_v2.rs`, `src/engine/verification_content.rs`, `src/engine/verification_structural.rs`, `src/app/api_verification.rs`, `src/app/audit.rs`.
- **Implementation Details**:
  - Map all verification gates to the 8-gate evidence ledger (`src/app/audit.rs`).
  - Develop a tamper matrix inducing violations (altered amounts, dates, inserted/removed rows, font swaps, geometry shifts, metadata manipulation).
  - Enforce atomic evidence writes using temp files and atomic rename to ensure integrity under crash (CRC32 proof).
  - Consume thresholds solely from `assets/verification-calibration-v2.json`.
- **Verification Steps**: `cargo test --test verification_repeatability` to prove identical input yields byte-identical evidence JSON. Run tamper matrix tests asserting explicit trip failures.
- **Failure Recovery**: Any unhandled write failures must abort the operation safely and retain the last-good evidence JSON.

### Phase C — Visual Fidelity Proof Harness
- **Target Files**: `src/bin/test_pdfium.rs`, `tests/node/applitools_bridge.test.js`, `generate_10_screenshots.rs`, `take_screenshots.ps1`.
- **Implementation Details**:
  - Render pre/post edit at 300 DPI via LOCAL Pdfium. (Use `pdfrest.rs` strictly gated behind confirmation).
  - Compute pixel-diffs and SSIM against calibration thresholds. Untouched regions must have Δ=0 (within epsilon), edited regions must be glyph-accurate.
  - Execute `applitools_bridge.test.js` ONLY if `APPLITOOLS_API_KEY` is present.
- **Verification Steps**: Synthesize and output side-by-side PNG diffs to `audit-evidence/lifecycle-phase3/`.
- **Failure Recovery**: If Pdfium render panics, gracefully fail the test and report library linkage missing.

### Phase D — Adversarial Robustness
- **Target Files**: `tests/chaos_tests.rs`, `tests/concurrency_chaos.rs`, `tests/setting_combinations.rs`, `tests/integrity_regressions.rs`.
- **Implementation Details**:
  - Test concurrent mutations on a single PDF to verify strict serialization logic (no interleaved writes).
  - Test mid-write cancellations to prove retention of last-good file state.
- **Verification Steps**: `cargo test --test concurrency_chaos --test chaos_tests`.
- **Failure Recovery**: If deadlock is detected, panic the test and require thread-locking rewrite using `tokio::sync::Mutex` properly.

### Phase E — Targeted Validation & Report
- **Implementation Details**: Write `plans/2026-08-25-phase3-ledger-visual-report.md`; update `findings_register.md`. Provide HANDOFF note for Prompt 4 (verifying gates trustworthy, evidence pipeline crash-safe, visual diff harness reusable).
- **Verification Steps**: Run `cargo fmt && cargo check && cargo test --test engine_verification_tests --test verification_content_tests --test verification_structural_tests --test verification_repeatability --test api_mock_tests --test chaos_tests --test concurrency_chaos --test setting_combinations --test integrity_regressions && cargo clippy --all-targets --all-features -- -D warnings`.

---
---

# PROMPT 4 OF 4 — TRANSFER, TEMPLATE SYNTHESIS & FULL-LIFECYCLE CERTIFICATION

> Copy everything from the next line down. Send after Prompt 3 completed.

---

**MISSION: Complete the crown-jewel lifecycle — transferring complete transaction ledgers from any source statement PDF into another statement's format via intelligently built, correctly structured target-template PDFs — then certify the ENTIRE user journey (shortcut → UFO → edit → verify → transfer) with one unattended executable gauntlet and a consolidated certification report. The refined-template set must be COMPLETED (not just studied), the transfer matrix must round-trip truthfully leveraging the Reducto SDK for robust layout-agnostic schema extraction, and the whole system must leave behind one script whose green run IS the certificate. Diagnose before you fix; pin every bug with a named regression test.**

Same workspace, same constitution (repeated below — it binds you fully).

## READ FIRST

1. [AGENTS.md](../AGENTS.md); bankfidelity project skill; reducto skill.
2. Phase reports 1–3 (launch, editing, ledger/visual): [plans/](.) directory, dated 2026-08-25.
3. [docs/SOP.md](../docs/SOP.md), [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md).
4. [findings_register.md](../findings_register.md).

## NON-NEGOTIABLE CONSTITUTION

(Identical to Prompt 1 §NON-NEGOTIABLE CONSTITUTION, items 1–9. Reporting goes to [plans/2026-08-25-lifecycle-certification-report.md](2026-08-25-lifecycle-certification-report.md) + [findings_register.md](../findings_register.md). Two domain laws added: (a) **TransferTransactions/RunTransferTests strictly require Reducto API for the layout-mapping step — there is NO offline equivalent; if REDUCTO_API_KEY is absent, stop at that exact stage and report the blockage instead of faking it**. (b) Regenerating files under [stress_test_outputs/](../stress_test_outputs) and creating template renders is allowed; DELETING generated outputs/audits requires confirmation.)

## PHASED OPERATION PLAN

### Phase A — Template Intelligence (Complete the refined set)
- **Target Files**: `bank_templates/*.yaml`, `src/extractors/templates.rs`.
- **Implementation Details**:
  - Diff existing templates (e.g., `ing_orange_au.yaml` vs `.refined.yaml`) to document refinement additions (x-ranges, bands, regexes).
  - Create `anz_plus_au.refined.yaml`. Author NEW templates for `bankwest` and `commbank_smartaccess` using Phase-2 geometry facts.
  - Implement a rigorous Template Validator in `templates.rs` validating schema completeness, geometry sanity (columns fit page, non-overlapping bands), and regex compilability.
- **Verification Steps**: `cargo test --test template_validation`.
- **Failure Recovery**: Reject bad templates with field-level verbose errors pointing to exact lines in the YAML.

### Phase B — Target Template PDF Synthesis
- **Target Files**: `src/engine/typst_engine.rs`, `assets/generic_bank_statement.typ`, `src/engine/offline_parser.rs`, `src/extractors/geometry.rs`.
- **Implementation Details**:
  - Synthesize a pristine reference PDF per target type using the REFINED yaml + Typst engine. Save to `bank_templates/rendered/<bank>.pdf`.
  - Self-consistency loop: Ensure every synthesized template parses cleanly via `offline_parser`, passes structural verification, and perfectly matches its YAML geometry via `extract_data`.
- **Verification Steps**: Assert `extract_data(synthesized_pdf) == template_yaml_definition`.
- **Failure Recovery**: If rendering fails, emit detailed Typst trace compilation errors to facilitate YAML adjustments.

### Phase C — Transfer Engine Campaign (Powered by Reducto)
- **Target Files**: `src/engine/transfer.rs`, `src/ai/apply_report.rs`, `src/engine/transfer_test_harness.rs`, `tests/transfer_retry_tests.rs`.
- **Implementation Details**:
  - Implement layout-agnostic transaction extraction using Reducto API (`POST /extract`). Supply an explicit JSON schema modeling canonical ledger rows (date, amount, description, balance).
  - Use `settings.deep_extract: true` for flawless data mapping on difficult formats.
  - If `REDUCTO_API_KEY` is missing, halt execution and report the block (Constitution Law a).
  - Execute pairwise matrix (commbank↔bankwest, westpac→macquarie, ing→commbank, anz→westpac, fallback.pdf→each).
  - Output mapped PDFs to `stress_test_outputs/<source>__to__<target>.pdf`.
- **Verification Steps**: `cargo test --test au_transfer_stress`.
- **Failure Recovery**: On partial transfer failure, resume gracefully retaining already mapped rows.

### Phase D — Round-trip Truth & Validation
- **Target Files**: `tests/au_transfer_stress.rs`, `tests/e2e_segmented_pipeline.rs`, `tests/segment_mapping.rs`, `tests/segment_transaction.rs`.
- **Implementation Details**:
  - Assert that `extract_data(output)` yields a canonical ledger identical to the source (dates normalized, amounts exact, balances continuous).
  - Assert all structural verification gates pass against the TARGET template.
- **Verification Steps**: Render test assertions for each permutation. Add regressions for any mismatches in `findings_register.md`.
- **Failure Recovery**: Pinpoint failures to parser, template, mapper, or rebalance logic, generating explicit diagnostics.

### Phase E — CAPSTONE: Full-Lifecycle Certification Gauntlet (Unattended)
- **Target Files**: `scripts/run_lifecycle_certification.ps1`.
- **Implementation Details**:
  - Author a unified execution script: `build` → `doctor` → `verify-api-keys` → launch via desktop shortcut → complete MCP handshake → UFO dispatches `modify_text` on AU sample copy → verification gates green → one full transfer (commbank → synthesized westpac template) → round-trip check.
  - Utilize the MCP Bank Statement Protocol (Directive 3): `extract_data` -> `local_ai_chat` -> `modify_text` -> `verify_layout`.
- **Verification Steps**: Script must run start-to-finish completely unattended.
- **Failure Recovery**: If script exits non-zero, fail the certification explicitly with error logs bundled to `audit-evidence/lifecycle-certification/`.

### Phase F — Consolidation & Truth Pass
- **Implementation Details**: Merge all phase reports into `plans/2026-08-25-lifecycle-certification-report.md`. Reconcile versions to `2.0.0` in `README.md`, `QUICKSTART.md`, `CHANGELOG.md`. Propose root-litter cleanup (scratch.py, trace dumps). Refresh `audit-evidence/RANKED_FIDELITY_REPORT.md` with transfer scores.
- **Verification Steps**: Check `findings_register.md` to ensure zero P0/P1s exist.
- **Failure Recovery**: Retain artifacts indefinitely if tests fail.

### Phase G — Final Validation
- **Target Files**: `plans/2026-08-25-lifecycle-certification-report.md`.
- **Verification Steps**: Run `cargo test --test au_transfer_stress --test transfer_retry_tests --test e2e_segmented_pipeline --test segment_mapping --test segment_transaction --test engine_workflow_tests`. Run full cargo gate.
- **Failure Recovery**: Ensure green exit code before declaring the series complete.

---
*End of series. After Prompt 4's certification report exists, the lifecycle is certified: shortcut → UFO → Windows ops → BankFidelity editing → visual fidelity → cross-format transfer via synthesized target templates — all evidenced, all regressed, all green.*
