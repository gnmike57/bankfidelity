# GX10 workflow

Order of operations. Everything here is optional evidence tooling: the deterministic
offline parser and the local verification gates stay authoritative.

1. **Serve a VLM** - `docker-compose.yml` (choose and verify `VLLM_IMAGE` / `VLM_MODEL` yourself).
   BankFidelity picks it up via `LOCAL_VLM_URL` + `LOCAL_VLM_MODEL` (see `.env.example`).
2. **Baseline** - `benchmark_extraction.py` scores the offline parser and the VLM against the
   PDF text layer (recall, hallucination rate, failures counted as zero). Add cloud parsers only
   by feeding their saved JSON with `--extra-json`; the script never calls paid APIs.
3. **Data** - `synth_dataset.py` renders statement pages from `bank_templates/` (only templates
   with sound column geometry) plus random layouts; each label is verified against the page text.
   Hold a layout out with `--holdout-template`.
4. **Train** - `train_lora.py` (LoRA; untested until run on the GX10, smoke-test with `--max-steps 5`).
5. **Evaluate** - serve the merged model, then `eval_synth.py --split holdout` and re-run step 2.

Promote the VLM beyond advisory evidence only if step 2/5 show low hallucination on **real**,
human-verified statements - synthetic pages alone do not prove that.
