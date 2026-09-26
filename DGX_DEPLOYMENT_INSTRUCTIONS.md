---
kind: spec
title: "DGX Spark Deployment Package — Complete Instructions"
---

# DGX Spark Deployment (Brand New Setup — September 2026)

## Package Contents (This Package)

This ZIP (`Desktop_DGX_Deployment_Package.zip`) contains:

- `AUTH_REGISTRY_ARCHITECTURE.md` — 24/7 authorization-gated dataset pipeline framework
- `SOTA_AUDIT_ARCHITECTURE.md` — Full SOTA audit (specialist AI ensemble, DGX Spark, 50-edit cascade, sub-pixel CUDA)
- `ARCHITECTURE_FINAL_END_GOAL.md` — Updated architecture with end goal (1200 DPI, authorization pipeline, dataset feeding specialist AI)
- `DGX_SPARK_DEPLOYMENT_SPEC.md` — Full DGX Spark spec (RustRover setup, version lock 1.89.0, CUDA 12.4, ARM64 build, dependency spec)
- `.env` — Real configuration file (backed up from `Desktop_Archive/Projects/bank-statement-fidelity-editor-main/.env` and desktop copy; contains PYMUPDF_PRO_KEY, OPENROUTER_API_KEY, REDUCTO_API_KEY, etc. — DO NOT COMMIT TO PUBLIC REPO; for laptop transfer only)
- Source edits applied: `modals.rs` (slider 1200), `gui.rs` (default DPI 600), `calibration-v2.json` (base 600, adaptive ON, finer tiles, stricter thresholds)

---

## DGX Spark Hardware (Brand New)

- **Platform:** NVIDIA DGX Spark
- **CPU:** GB10 Grace (20-core ARM Neoverse V2)
- **GPU:** 6144 CUDA Cores (Blackwell)
- **Memory:** 128GB Unified Memory
- **Storage:** 4TB NVMe Gen5
- **OS:** Ubuntu 24.04 LTS ARM64

---

## Setup Steps (In Order — No Omissions)

### Step 1: Unzip This Package

Extract to your DGX Spark working directory (e.g., `/data/` or `~/work/`):

```bash
unzip Desktop_DGX_Deployment_Package.zip -d ~/work/
```

### Step 2: Configure .env for DGX Spark

Edit `.env` (included in package — contains real keys from Desktop_Archive). Add/update:

```bash
# DGX Spark mode
DGX_SPARK_MODE=1
CUDA_BATCH_SIZE=32
SPECIALIST_MODEL_DIR=/models/

# Linux Python paths (update from Windows paths if needed)
PYTHON_EXECUTABLE=/usr/bin/python3
PYO3_PYTHON=/usr/bin/python3
PYTHONPATH=/data/python_env/lib/site-packages:/usr/lib/python3/dist-packages

# Dataset pipeline
AUTH_REGISTRY_FILE=artifacts/auth_registry/index.md
SCRIBD_PREMIUM_ENABLED=false  # Set to true ONLY after premium account + authorization
SCRAPE_DIRECT_ENABLED=false  # Set to true ONLY after per-bank authorization
DATASET_MAX_SIZE_GB=2048
AGENT_POLL_INTERVAL_SEC=300

# CUDA environment
CUDA_VISIBLE_DEVICES=0
TORCH_CUDA_ARCH_LIST=8.6
```

**Security:** This `.env` contains real API keys. It is for your laptop transfer only. Do not commit to public repositories. If deploying to a shared DGX, use environment injection or secrets manager instead.

### Step 3: Install System Dependencies (DGX Spark — Ubuntu 24.04 ARM64)

```bash
sudo apt-get update && sudo apt-get install -y --no-install-recommends \
    build-essential pkg-config libssl-dev libfontconfig1-dev libfreetype6-dev \
    libmupdf-dev tesseract-ocr libleptonica-dev libglib2.0-dev libgtk-3-dev \
    ca-certificates curl git wget libgl1 libxcb-render0 libxcb-shape0 \
    libxcb-xfixes0 libxkbcommon0 libegl1 libwayland-egl1 \
    libnvidia-gl-550-server  # DGX Spark GPU driver

# CUDA Toolkit (if not pre-installed on DGX)
wget https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/aarch64/cuda-keyring_1.1-1_all.deb
sudo apt install ./cuda-keyring_1.1-1_all.deb
sudo apt-get update
sudo apt-get install -y cuda-toolkit-12-4 cuda-cudart-12-4
```

### Step 4: Install Rust Toolchain (Version Lock: 1.89.0)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.89.0 --profile minimal
source ~/.cargo/env
rustup component add rustfmt clippy
rustup override set 1.89.0
```

### Step 5: Clone / Copy Repository to DGX Spark

Copy the repository (with the applied edits) to the DGX Spark:

```bash
# From your laptop (after copying .env and source edits)
scp -r ~/work/bank-statement-fidelity-editor user@dgx-spark-ip:/data/
```

Or clone directly on DGX:

```bash
cd /data/
git clone <repository-url> bankfidelity
git checkout traycer/snappy-lynx  # Current branch with 1200 DPI + authorization framework
```

Apply the included source edits (already in the package): `modals.rs` slider 1200, `gui.rs` default 600, `calibration-v2.json` refined.

### Step 6: Build for ARM64 + CUDA

```bash
cd /data/bankfidelity
rustup override set 1.89.0
# Add ARM64 target
rustup target add aarch64-unknown-linux-gnu
# Build with release profile
cargo build --release --target aarch64-unknown-linux-gnu
# Verify
cargo check --target aarch64-unknown-linux-gnu
```

### Step 7: Configure Python Environment for DGX Spark (CUDA-Aware)

```bash
python3 -m venv /data/.venv_e2e
source /data/.venv_e2e/bin/activate
pip install --upgrade pip
pip install pymupdf pymupdfpro fonttools pillow ONNXRuntime>=1.19.0 numpy>=2.0.0 torch>=2.5.0 python-dotenv
# Verify CUDA availability from Python
python -c "import torch; print('CUDA available:', torch.cuda.is_available()); print('CUDA device count:', torch.cuda.device_count())"
```

### Step 8: Deploy Specialist Model Weights

Deploy the 6 specialist model directories to DGX Spark:

```bash
mkdir -p /models/font_classifier /models/layout_regressor /models/template_comparator \
         /models/transfer_mapper /models/balance_explainer
# Copy model weight files (must be downloaded/deployed separately due to size)
# Example: cp /path/to/downloaded/surya_deepfont_4k.pt /models/font_classifier/
# Example: cp /path/to/downloaded/florence2_pdf_512.pt /models/layout_regressor/
# Example: cp /path/to/downloaded/siamese_pdf_4k.pt /models/template_comparator/
# Example: cp /path/to/downloaded/transfer_encoder_4k.pt /models/transfer_mapper/
# Example: cp /path/to/downloaded/balance_forensic_4k.pt /models/balance_explainer/
```

**Note:** Model weights (`.pt` files) are not included in this ZIP due to size (~2.5GB total). They must be deployed separately. The skeleton directories (`models/`) are included.

### Step 9: Compile CUDA Kernel (`kernels/subpixel_verifier.cu`)

```bash
# Compile CUDA kernel for DGX Spark (CUDA 8.6 architecture)
nvcc -c -arch=sm_60 kernels/subpixel_verifier.cu -o /models/subpixel_verifier/subpixel_verifier.o
# Or compile to .ptx for runtime loading
nvcc -ptx kernels/subpixel_verifier.cu -o /models/subpixel_verifier/subpixel_verifier.ptx
```

### Step 10: Configure Environment Variables (`.env`)

Copy the included `.env` file and confirm DGX Spark variables are set:

```bash
cp .env.example .env  # Then apply the included .env (already contains real keys)
# Confirm DGX variables
cat .env | grep -E "DGX_SPARK_MODE|CUDA_BATCH_SIZE|SPECIALIST_MODEL_DIR|AUTH_REGISTRY_FILE|SCRIBD_PREMIUM_ENABLED|SCRAPE_DIRECT_ENABLED"
```

Expected output (with real `.env` applied):

```
DGX_SPARK_MODE=1
CUDA_BATCH_SIZE=32
SPECIALIST_MODEL_DIR=/models/
AUTH_REGISTRY_FILE=artifacts/auth_registry/index.md
SCRIBD_PREMIUM_ENABLED=false
SCRAPE_DIRECT_ENABLED=false
```

**Security reminder:** `SCRAPE_DIRECT_ENABLED` and `SCRIBD_PREMIUM_ENABLED` remain `false` until per-bank authorization (`auth_registry/index.md`) reaches `AUTHORIZED`. No automatic scraping activates without authorization.

### Step 11: Launch DGX Spark Pipeline

```bash
# GUI mode (with DGX Spark environment)
DGX_SPARK_MODE=1 CUDA_BATCH_SIZE=32 ./target/release/dual-core-pdf-pipeline gui

# CLI balance mode (with CUDA verification enabled)
DGX_SPARK_MODE=1 ./target/release/dual-core-pdf-pipeline balance \
    -i bankfidelity/BANKTEST/project.uiproj \
    -o output/verified_statement.pdf \
    --auto-approve

# Launch DGX-specific launcher (if created from launch_dgx.sh template in docs)
bash launch_dgx.sh gui
```

---

## Migration Checklist (From Current Laptop to DGX Spark)

- [x] Branch sync completed (`master` 9d5df0e, `main` 179aa83, `audit` 5df3b71, `traycer/snappy-lynx` f71b15d)
- [x] `.msi` removed (`BankStatementFidelityEditor-v2.0.0-windows-x86_64.msi` deleted from repo)
- [x] Desktop `.env` backed up (`Desktop_DGX_Deployment_Package.zip` includes `.env` — 4505 bytes with `PYMUPDF_PRO_KEY`, `OPENROUTER_API_KEY`, `REDUCTO_API_KEY`, etc.)
- [x] Linux binary targets (`launch.sh` updated; `launch_dgx.sh` reference in spec)
- [x] 1200 DPI slider (`modals.rs`) + default (`gui.rs`) + calibration (`assets/verification-calibration-v2.json`)
- [x] Authorization framework (`artifacts/auth_registry/index.md`)
- [x] SOTA audit (`artifacts/sota-audit/index.md`)
- [x] DGX Spark spec (`docs/DGX_SPARK_SPEC.md`)
- [x] Architecture updated (`docs/ARCHITECTURE.md`)
- [x] Concrete artifacts: `kernels/` (CUDA skeleton), `models/` (6 specialist dirs), `auth_registry/` (pipeline framework)
- [ ] Copy `.env` from ZIP to DGX Spark working directory (secure transfer only)
- [ ] Build ARM64 binary (`cargo build --release --target aarch64-unknown-linux-gnu`)
- [ ] Build Docker image (`docker build -t bankfidelity-dgx:latest .`)
- [ ] Verify `tests/static_analysis.rs` passes on DGX build
- [ ] Confirm `audit-evidence/` manifests intact
- [ ] Confirm `bankfidelity/BANKTEST/` loads correctly
- [ ] Confirm `.env` secrets (`DUAL_CORE_PASSPHRASE`, `PYTHON_EXECUTABLE`) set on DGX before first run
- [ ] Deploy model weights (`models/*.pt`)
- [ ] Compile CUDA kernel (`kernels/subpixel_verifier.cu`)
- [ ] Configure Python environment (`.venv_e2e` or `/data/.venv_e2e` with `torch`, `ONNXRuntime`)
- [ ] Create machine-readable authorization registry (`scripts/auth_registry.json`)
- [ ] Obtain first signed bank authorization (manual process per institution)
- [ ] Activate dataset pipeline (set `SCRAPE_DIRECT_ENABLED=true` and `SCRIBD_PREMIUM_ENABLED=true` ONLY after authorization reaches `AUTHORIZED`)
- [ ] Confirm agent polling interval (`AGENT_POLL_INTERVAL_SEC=300` in `.env`)
- [ ] Confirm dataset tracking (`dataset_tracking/` initial count = 0; grows to 10,000 verified PDFs)

---

# Version Lock (September 2026) — Confirmed

- `rust-toolchain.toml`: `channel = "1.89.0"`
- `Cargo.lock`: full dependency lock (304544 bytes)
- `Cargo.toml`: `version = "0.5.1"` (confirmed in `docs/CHANGELOG.md` line 368)
- Python: 3.12 (`.venv_e2e/pyvenv.cfg` line 5)
- Docker: `rust:1.89-bookworm` (line 17 of `Dockerfile`)
- CUDA: 12.4 (DGX Spark build target)
- RustRover: 2026.1+ (IDE configuration spec in `docs/DGX_SPARK_SPEC.md`)

This package is ready for deployment. All source edits, artifacts, and documentation updates are included. The `.env` is included for laptop transfer only and must be secured before any shared deployment.
