# Phase 2: Core Engine Upgrade

In `runtime.rs`, under `Job::TransferTransactions`:
Currently, it uses `doc_ai` (or offline) to parse `source_pdf` and `target_pdf`.
Then it uses `gemini` to do `format_mapping`.
We need to:
1. Parse using DocAI, Mindee, and LlamaParse concurrently.
2. Cross-reference results and vote on consensus.

Wait, if we use all three, we'll need their clients instantiated.

Let's check `src/engine/transfer.rs` to see how AiFormatMapping works.
