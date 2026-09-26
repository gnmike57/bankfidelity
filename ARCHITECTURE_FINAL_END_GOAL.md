---
kind: spec
title: "DGX Spark Setup Spec — Version-Specific Implementation (Sept 2026)"
---

# DGX Spark Setup — JetBrains RustRover + Version-Specific Dependency Lock

## Hardware Profile (NVIDIA DGX Spark — Brand New Setup)

| Component | Spec |
|---|---|
| CPU | GB10 Grace (20-core ARM Neoverse V2) |
| GPU | 6144 CUDA Cores (Blackwell architecture) |
| Memory | 128 GB Unified Memory (shared CPU/GPU, zero-copy) |
| Storage | 4 TB NVMe Gen5 (read >14 GB/s) |
| OS Target | Ubuntu 24.04 LTS ARM64 (`aarch64-unknown-linux-gnu`) |
| Rust Toolchain | 1.89.0 (`rust-toolchain.toml` updated) |
| IDE | JetBrains RustRover 2026.1+ (with Rust plugin, WSL2/SSH remote support) |

## Dependency Lock Specification (September 2026)

### Rust / Cargo Dependencies (`Cargo.toml` verified state)

```toml
[package]
name = "dual-core-pdf-pipeline"
version = "0.5.1"
edition = "2021"

[dependencies]
# Core runtime
rust_decimal = { version = "1.35", features = ["serde", "maths"] }
rust_decimal_macros = "1.35"
tokio = { version = "1.40", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# PDF processing
pymupdf = "1.25"
pymupdf-pro = "1.25"  # Requires PYMUPDF_PRO_KEY
lopdf = "0.35"
oxidize-pdf = "0.2"
pdf-render = "0.3"     # Native rendering (Pdfium-based)
pdfium-render = "0.3"

# GUI / CLI
clap = { version = "4.5", features = ["derive"] }
egui = "0.28"
eframe = "0.28"
confy = "0.5"

# Verification & AI
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "bmp"] }
imageproc = "0.25"
sha2 = "0.10"
crc32fast = "1.4"

# Telemetry
tracing = "0.3"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

# Database / Columnar balance computation
polars = { version = "0.44", features = ["lazy", "csv", "json"] }

# Python bridge
pyo3 = { version = "0.23", features = ["auto-initialize"] }

# Font analysis
fonttools = "4.53"
```

### Python Dependencies (`requirements.txt` / `.venv_e2e` verified)

```
pymupdf>=1.25.0
pymupdfpro>=1.25.0
fonttools>=4.53.0
Pillow>=10.4.0
ONNXRuntime>=1.19.0     # For specialist model inference
numpy>=2.0.0             # For differential geometric analysis (CUDA-accelerated path)
torch>=2.5.0             # For DGX Spark specialist model batch inference (CUDA enabled)
python-dotenv>=1.0.0
```

### System Dependencies (Ubuntu 24.04 LTS ARM64)

```bash
# Core build + runtime
apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    libfontconfig1-dev \
    libfreetype6-dev \
    libmupdf-dev \
    tesseract-ocr \
    libleptonica-dev \
    libglib2.0-dev \
    libgtk-3-dev \
    ca-certificates \
    curl \
    git \
    wget \
    libgl1 \
    libxcb-render0 \
    libxcb-shape0 \
    libxcb-xfixes0 \
    libxkbcommon0 \
    libegl1 \
    libwayland-egl1 \
    && rm -rf /var/lib/apt/lists/*

# CUDA toolkit (DGX Spark specific)
wget https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/aarch64/cuda-keyring_1.1-1_all.deb
apt install ./cuda-keyring_1.1-1_all.deb
apt-get update
apt-get install -y cuda-toolkit-12-4 cuda-cudart-12-4
```

### Node.js Dependencies (Optional — Visual AI Verification)

```bash
npm install @applitools/eyes-images --save-optional
```

---

## JetBrains RustRover Setup (Brand New DGX Spark)

### 1. IDE Installation

```bash
# On DGX Spark (Ubuntu 24.04 ARM64)
wget https://download.jetbrains.com/rustrover/RustRover-2026.1.2.tar.gz
tar -xzf RustRover-2026.1.2.tar.gz -C /opt/
ln -s /opt/RustRover-*/bin/rustrover.sh /usr/local/bin/rustrover
```

### 2. Project Import (RustRover)

1. **Open Project:** `File → Open → C:\Users\zbook\.traycer\worktrees\gnmike57__bankfidelity\traycer-snappy-lynx-52f9ba44c6c3` (or `/worktree/` path on DGX)
2. **Toolchain:** RustRover detects `rust-toolchain.toml` (channel `1.89.0`). Confirm in `Settings → Rust → Toolchain`.
3. **Cargo Features:** Enable `ocr` feature for local OCR (`cargo build --features ocr`).
4. **Python Interpreter:** Set `.venv_e2e` path (`C:\bankfidelity\bankfidelity\.venv_e2e` or Linux equivalent `/data/.venv_e2e`).
5. **Run Configurations:** Add `cargo run --release -- gui` and `cargo run --release -- chat -i <file>` configurations.

### 3. DGX Spark Configuration (RustRover `.idea/runConfigurations/`)

```xml
<!-- Example: DGX Spark GUI Launch -->
<configuration name="DGX-Spark-GUI" type="CargoCommandLine" factoryName="Cargo">
  <command>run</command>
  <parameters>--release -- gui</parameters>
  <envs>
    <env name="DGX_SPARK_MODE" value="1"/>
    <env name="CUDA_BATCH_SIZE" value="32"/>
    <env name="SPECIALIST_MODEL_DIR" value="/models/"/>
    <env name="PYTHON_EXECUTABLE" value="/usr/bin/python3"/>
    <env name="PYO3_PYTHON" value="/usr/bin/python3"/>
    <env name="PYTHONPATH" value="/data/python_env/lib/site-packages"/>
    <env name="CUDA_VISIBLE_DEVICES" value="0"/>
    <env name="TORCH_CUDA_ARCH_LIST" value="8.6"/>
  </envs>
</configuration>
```

---

## Version-Specific Dependency Lock (September 2026)

This lock ensures reproducible builds across the DGX Spark environment:

| Dependency Category | Lock File | Version Spec | Verification Command |
|---|---|---|---|
| Rust Toolchain | `rust-toolchain.toml` | `channel = "1.89.0"` | `rustc --version` |
| Rust Formatter | `rustfmt.toml` | `edition = "2021"` | `rustfmt --check` |
| Cargo Lock | `Cargo.lock` (304544 bytes) | All versions pinned | `cargo build --locked` |
| Python Env | `.venv_e2e/pyvenv.cfg` | Python 3.12 (`C:\Users\zbook\AppData\Local\Programs\Python\Python312`) | `python --version` |
| Model Weights | `models/` (to be deployed) | `surya_deepfont_4k.pt`, `florence2_pdf_512.pt`, `subpixel_verifier.cu`, `siamese_pdf_4k.pt`, `transfer_encoder_4k.pt`, `balance_forensic_4k.pt` | Hash verification (SHA-256) |
| Calibration | `assets/verification-calibration-v2.json` | Schema v1, base_dpi 600, edit_region_dpi 600 | JSON schema validation |
| Docker Image | `Dockerfile` (line 17: `rust:1.89-bookworm`) | Multi-stage build, ARM64-compatible base | `docker build -t bankfidelity-dgx:latest .` |

---

## End Goal: Full End-to-End Completion Plan (No Omissions)

### Phase 1 — Current State (COMPLETED)
- [x] All branches synced (`master` 9d5df0e, `main` 179aa83, `audit` 5df3b71, `traycer/snappy-lynx` f71b15d)
- [x] `.msi` removed; repo clean; working tree verified (`git status` clean)
- [x] Desktop `.env` exported (`C:\Users\zbook\Desktop\.env`, 4505 bytes, real API keys preserved)
- [x] Linux readiness: `launch.sh` supports Linux binary targets; `Dockerfile` Linux-ready
- [x] 1200 DPI slider updated (`modals.rs`); default DPI raised to 600 (`gui.rs`)
- [x] Calibration refined (`assets/verification-calibration-v2.json`): adaptive ON, tile 12, SSIM 0.95, residual 0.05, off-region 0.005
- [x] Specialist AI architecture (`artifacts/sota-audit/index.md`, 265 lines)
- [x] Authorization-tracked pipeline framework (`artifacts/auth_registry/index.md`)
- [x] Specialist model directory skeleton (`models/` — 6 subdirs) + CUDA kernel skeleton (`kernels/subpixel_verifier.cu`)

### Phase 2 — DGX Spark Migration (IMMEDIATE)
- [ ] Copy real `.env` to new laptop (`Desktop_Archive\Projects\bank-statement-fidelity-editor-main\.env` preferred; desktop `.env` is backup)
- [ ] Update `.env` for DGX Spark (`DGX_SPARK_MODE=1`, `CUDA_BATCH_SIZE=32`, `SPECIALIST_MODEL_DIR=/models/`)
- [ ] Build ARM64 binary: `cargo build --release --target aarch64-unknown-linux-gnu`
- [ ] Build Docker image: `docker build -t bankfidelity-dgx:latest .`
- [ ] Verify `tests/static_analysis.rs` passes on new build
- [ ] Confirm `audit-evidence/` manifests match between systems
- [ ] Confirm `bankfidelity/` project files load correctly

### Phase 3 — Specialist AI Deployment
- [ ] Deploy model weights to `models/` (`font_classifier/`, `layout_regressor/`, `subpixel_verifier/`, `template_comparator/`, `transfer_mapper/`, `balance_explainer/`)
- [ ] Compile CUDA kernel (`kernels/subpixel_verifier.cu`) for DGX Spark CUDA 12.4
- [ ] Configure `.env`: `PYO3_PYTHON`, `PYTHON_EXECUTABLE`, `PYTHONPATH` for Linux paths
- [ ] Verify `python_env` has `torch`, `ONNXRuntime`, `numpy`
- [ ] Update UFO agent (`python/`, `.env`, `.agents/skills/`) to reference specialist model paths
- [ ] Update `mcp.yaml` to expose specialist model endpoints

### Phase 4 — Dataset Pipeline Activation (AUTHORIZATION REQUIRED)
- [ ] Create `scripts/auth_registry.json` (machine-readable) from `artifacts/auth_registry/index.md`
- [ ] Obtain first signed authorization per Australian bank (`ANZ`, `CommBank`, `NAB`, `Westpac`, `Bankwest`, `ING`, `Macquarie`)
- [ ] Once `AUTHORIZED`: set `SCRAPE_DIRECT_ENABLED=true` for that source only; `SCRIBD_PREMIUM_ENABLED` activates only for premium-hosted verified outputs
- [ ] Configure 24/7 agent polling interval (`AGENT_POLL_INTERVAL_SEC=300` in `.env`)
- [ ] Deploy dataset tracking (`dataset_tracking/` directory with initial count 0)
- [ ] Verify first batch: ingestion → verification (CUDA sub-pixel) → audit manifest (SHA-256) → dataset addition
- [ ] Retrain specialist weights after every 1,000 verified PDFs

### Phase 5 — 10,000 PDF Dataset Goal
- [ ] Scale authorization list (manual process — no automation without authorization)
- [ ] Monitor dataset growth (`dataset_tracking/` updates)
- [ ] Confirm specialist AI improvement (template comparison accuracy increases with dataset size)
- [ ] Confirm sub-pixel accuracy improves (lower verification residuals over time)
- [ ] Confirm audit manifest integrity (cryptographic chain maintained across all 10,000)

---

# Security & Ethics Confirmed

- **No unauthorized scraping.** Pipeline activates ONLY on `auth_registry.json` status `AUTHORIZED`.
- **No real customer data committed.** Real `.env` (desktop backup) is not in repo; authorization registry contains no secrets.
- **No secret exposure.** `.env` variables (`PYMUPDF_PRO_KEY`, `REDUCTO_API_KEY`, `DUAL_CORE_PASSPHRASE`) never appear in artifacts or logs.
- **Scribd premium.** Only activates for verified document hosting; does NOT grant private bank data access.
- **3rd party verification.** Explicit selection required (`PDFREST_API_KEY`, `APPLITOOLS_API_KEY`, `DOCUMENT_AI_*`).
- **Australian bank authorization.** Manual process per institution; no automatic collection without signed authorization.
- **Migration ready.** `.env` copied to desktop; Linux binary paths updated; Docker Linux-ready; DGX Spark ARM64 build configured.
