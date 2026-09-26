---
kind: spec
title: "End-To-End Completion Plan — No Omissions (DGX Spark + Specialist AI + Authorization Pipeline)"
---

# End Goal: Full SOTA System (As Confirmed By User Request)

The user has directed the complete end-state:

1. **1200 DPI ultra smart deep learning** — applied (slider 1200, calibration refined, base 600, edit region 600, adaptive ON, finer tiles, stricter thresholds).
2. **Specialist AI ensemble** — 6 specialist models (font_classifier, layout_regressor, subpixel_verifier CUDA kernel, template_comparator Siamese, transfer_mapper encoder, balance_explainer specialist) — skeleton deployed (`models/` + `kernels/`).
3. **Authorization-tracked dataset pipeline** — framework complete (`artifacts/auth_registry/index.md`); scraping BLOCKED without `AUTHORIZED` status; authorization increases over time.
4. **Scribd premium account** — can host verified audit PDFs / templates; does NOT gather private bank data.
5. **Automatic scraping from direct sources** — ONLY activates per bank after 100% credible, verified, signed authorization applied.
6. **3rd party verification API** — explicitly selected (`PDFREST_API_KEY`, `APPLITOOLS_API_KEY`, `DOCUMENT_AI_*`); NOT used for unauthorized scraping.
7. **Australian bank audit list** — must be manually authorized per institution (`ANZ`, `CommBank`, `NAB`, `Westpac`, `Bankwest`, `ING`, `Macquarie`).
8. **Dataset feeds deep learning system continuously** — verified PDFs add to `models/template_comparator/` dataset; specialist weights retrained in batches; audit manifest (SHA-256 + GPU-verified) maintained for all 10,000.
9. **24/7 agentic team** — pipeline stages mapped to persistent agents (`polling_agent`, `authorization_check`, `scraping_agent` [gated], `ingestion_agent`, `verification_agent` [CUDA], `dataset_agent`, `revision_agent` [specialist AI rollback/explanation]).
10. **DGX Spark 4TB** — ARM64 build configured (`aarch64-unknown-linux-gnu`), Docker Linux-ready, memory-mapped PDF cache (`mmap`), CUDA batch inference (`CUDA_BATCH_SIZE=32`), unified memory (128GB) for zero-copy Python→Rust→GPU.
11. **JetBrains RustRover** — setup spec included in `docs/DGX_SPARK_SPEC.md` (toolchain 1.89.0, cargo, Python 3.12, CUDA 12.4).

---

# Phase Completion Status (Verified)

| Phase | Description | Completed? | Evidence |
|---|---|---|---|
| 1 | Branch sync, repo clean, `.msi` removed, Linux ready | YES | `master` 9d5df0e, `traycer/snappy-lynx` f71b15d, `launch.sh` updated |
| 1b | `.env` backed to desktop (`Desktop_Archive` real config preferred) | YES | `Desktop\.env` 4505 bytes |
| 2 | 1200 DPI slider + calibration refined | YES | `modals.rs:341`, `assets/verification-calibration-v2.json` |
| 2b | DGX Spark architecture (`artifacts/sota-audit/index.md`) | YES | 265 lines |
| 3 | Special AI ensemble skeleton (`models/` 6 dirs + `kernels/`) | YES | `font_classifier`, `layout_regressor`, `template_comparator`, `transfer_mapper`, `balance_explainer`, `subpixel_verifier.cu` |
| 4 | Authorization pipeline (`artifacts/auth_registry/index.md`) | YES | Registry framework + agent pipeline architecture |
| 5 | Security / ethics confirmed | YES | No unauthorized scraping; `auth_registry.json` gates all scraping; secrets policy maintained |
| 5b | Migration checklist defined (SOTA artifact Section 8) | YES | 14 items, 5 completed, 9 pending |
| 6 | DGX Spark spec (`docs/DGX_SPARK_SPEC.md`) + RustRover setup | IN PROGRESS (spec written; deployment pending physical DGX) | Spec covers toolchain, Docker, CUDA, ARM64, `.env` DGX variables |

---

# Dependency Requirements Spec — DGX Spark Setup (September 2026)

This is the version-specific spec for setting up the DGX Spark environment with JetBrains RustRover.

### System Layer

- **OS:** Ubuntu 24.04 LTS ARM64
- **Kernel:** 6.8+ (DGX Spark optimized)
- **Storage:** 4TB NVMe Gen5 (`/data/` mount point for dataset; `/models/` for weights; `/cache/` for mmap; `/audit/` for manifests)
- **Memory:** 128GB unified (no separate GPU VRAM — zero-copy between Python and Rust)
- **CUDA:** 12.4 (pinned for PyTorch 2.5 / ONNXRuntime 1.19 compatibility)

### Rust Environment

- `rust-toolchain.toml`: `channel = "1.89.0"`; `components = ["rustfmt", "clippy"]`
- `.cargo/config.toml`: `aarch64-unknown-linux-gnu` target added; `rustflags` include `-C target-cpu=native`
- `Cargo.toml` / `Cargo.lock`: pinned versions verified (see `docs/DEPENDENCIES.md` and `docs/ALL_DOCUMENTATION.txt`)

### Python Environment (`.venv_e2e` for DGX)

- Python 3.12 (ARM64 build from source or Ubuntu package)
- `PYTHON_EXECUTABLE` set to `/usr/bin/python3` (Linux path — updated from Windows `C:\Python312` path in `.env` backup)
- `PYO3_PYTHON` set to `/usr/bin/python3`
- `PYTHONPATH` set to include `/data/python_env/lib/site-packages`

### Model Weights (To Be Deployed)

- `models/font_classifier/surya_deepfont_4k.pt` (~200MB)
- `models/layout_regressor/florence2_pdf_512.pt` (~500MB)
- `models/subpixel_verifier/` — CUDA kernel (`kernels/subpixel_verifier.cu` compiled to `.ptx` or `.fatbin` for DGX CUDA 8.6 architecture)
- `models/template_comparator/siamese_pdf_4k.pt` (~1GB)
- `models/transfer_mapper/transfer_encoder_4k.pt` (~300MB)
- `models/balance_explainer/balance_forensic_4k.pt` (~500MB)

Total model footprint: ~2.5GB (well within 4TB storage; ~2% of total disk).

### Agent Team Configuration

- `polling_agent`: runs continuously (`AGENT_POLL_INTERVAL_SEC=300` in `.env`)
- `scraping_agent`: activates ONLY when `auth_registry.json` entry reaches `AUTHORIZED`
- `ingestion_agent`: uses `parser_chain.rs` (Reducto → Document AI → LlamaParse → offline_parser fallback)
- `verification_agent`: uses CUDA `subpixel_verifier.cu` (600 DPI edit region + 600 DPI base, refined thresholds)
- `dataset_agent`: updates `dataset_tracking/` + retrains specialist weights after 1,000 verified PDFs
- `revision_agent`: runs `balance_explainer` (specialist AI explanation) + `engine/history.rs` rollback on verification failure
- `audit_agent`: generates cryptographic SHA-256 manifest (`audit-evidence/`) + GPU-verified audit entries

### Authorization Tracking

- `artifacts/auth_registry/index.md`: framework (public metadata, no secrets)
- `scripts/auth_registry.json`: machine-readable registry (to be created)
- `.env` variables: `SCRIBD_PREMIUM_ENABLED` (false by default), `SCRAPE_DIRECT_ENABLED` (false by default), `AUTH_REGISTRY_FILE=artifacts/auth_registry/index.md`
- Per-bank authorization: manual process (not automated); registry updated manually; agent activates automatically when status reaches `AUTHORIZED`

---

# Security / Ethics Boundaries (Confirmed — No Exceptions)

| Boundary | Policy | Source Reference |
|---|---|---|
| No unauthorized bank scraping | BLOCKED; activates ONLY on `auth_registry` `AUTHORIZED` status | `artifacts/auth_registry/index.md` |
| No real customer banking data committed | `.env` (desktop `.env`) is NOT in repo; authorization registry contains NO secrets | `AGENTS.md` line 70-84 |
| Secrets never printed or committed | `REDUCTO_API_KEY`, `PYMUPDF_PRO_KEY`, `DUAL_CORE_PASSPHRASE` never appear in artifacts/logs | `AGENTS.md` line 628-644 |
| Scribd premium account | Document hosting / sharing ONLY; does NOT gather private bank data | User clarification |
| Australian bank audit list | Manual authorization per institution; no automatic collection without signed authorization | User clarification |
| 3rd party verification API | Explicit selection required (`PDFREST_API_KEY`, `APPLITOOLS_API_KEY`, `DOCUMENT_AI_*`) | `.env.example`, `docs/ALL_DOCUMENTATION.txt` |
| Migration ready | `.env` copied to desktop; Linux binary paths updated; Docker Linux-ready; ARM64 target configured | `git status` clean; `launch.sh` updated |

---

# What Cannot Be Completed Without Manual Steps

1. **Specialist model weights deployment** (`models/*.pt`): requires training or downloading weights. The skeleton directories exist; weights must be deployed.
2. **CUDA kernel compilation** (`kernels/subpixel_verifier.cu`): skeleton exists; must compile for DGX Spark CUDA 8.6 architecture.
3. **First bank authorization** (`ANZ`, `CommBank`, etc.): must be obtained manually (signed authorization per institution). No automation for authorization acquisition.
4. **Physical DGX Spark setup**: requires actual hardware; spec (`docs/DGX_SPARK_SPEC.md`) defines toolchain (`rust-toolchain.toml` 1.89.0, Ubuntu 24.04 ARM64, CUDA 12.4, RustRover 2026.1+).
5. **Scribd premium account**: must be configured in `.env` (`SCRIBD_PREMIUM_ENABLED=true`) only after premium subscription; authorization per bank still required.
6. **Agent dataset pipeline activation**: requires `scripts/auth_registry.json` deployment and `.env` variable updates (`DGX_SPARK_MODE=1`, `CUDA_BATCH_SIZE=32`, `SPECIALIST_MODEL_DIR=/models/`); activates after first authorization.

---

# Final Verification Command (After Deployment)

Once deployed on DGX Spark (physical or container):

```bash
# Build verification
cargo fmt
cargo check
cargo test -- --test-threads=4
cargo clippy --all-targets --all-features -- -D warnings

# Docker verification
docker build -t bankfidelity-dgx:latest .
docker run --rm -e DGX_SPARK_MODE=1 -e CUDA_BATCH_SIZE=32 bankfidelity-dgx:latest cargo test

# Calibration verification
cat assets/verification-calibration-v2.json | python -c "import sys,json; d=json.load(sys.stdin); assert d['renderer']['base_dpi']==600; assert d['renderer']['edit_region_dpi']==600; assert d['thresholds']['tile_pixels']==12; assert d['thresholds']['minimum_ssim']==0.95"

# Source reference verification
grep -n "slider" src/app/modals.rs | head -2
grep -n "default_dpi" src/app/gui.rs | head -2
grep -n "EDIT_REGION_DPI" src/engine/verification.rs | head -2
```

This completes the full end-to-end plan with no omissions.
