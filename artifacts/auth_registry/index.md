---
kind: spec
title: "24/7 Authorization-Tracked Agentic Dataset Pipeline"
---

# Authorization-Gated Pipeline Design

Per user direction: **automatic scraping from direct sources and Scribd is ONLY assigned after 100% credible, verified, signed authorization per bank is applied. Authorizations increase over time. The pipeline adjusts accordingly.**

## Authorization Registry (`auth_registry`)

Every source is tracked with authorization status. No scraping agent activates until status = `AUTHORIZED`.

```markdown
### Registry Schema (per source record)
- `source_id`: unique identifier (e.g. `anz-statement-v1`)
- `institution`: bank name (e.g. `ANZ`, `CommBank`, `NAB`, `Westpac`)
- `source_type`: `direct_pdf`, `scribd_document`, `audit_evidence`, `template_library`
- `authorization_status`: `PENDING` | `VERIFIED` | `SIGNED` | `AUTHORIZED` | `REVOKED`
- `authorization_document_path`: path to signed authorization (stored securely, not in repo)
- `date_authorized`: timestamp
- `authorized_amount`: max number of PDFs allowed per authorization period
- `scraping_agent_assigned`: boolean (only `true` when status == `AUTHORIZED`)
- `current_pdf_count`: how many PDFs gathered
- `verification_level`: `manual`, `third_party_api`, `specialist_ai_ensemble`
- `last_verified`: timestamp of last audit
```

### Registry File Location
`artifacts/auth_registry/index.md` (this file) + `scripts/auth_registry.json` (machine-readable tracking)

### Current Registry State (Initial Setup — Authorization Required Before Activation)

| Source ID | Institution | Source Type | Auth Status | PDFs (Current / Max) | Agent Active? | Next Action |
|---|---|---|---|---|---|---|
| `anz-01` | ANZ | direct_pdf | PENDING | 0 / 0 | NO | Await signed authorization |
| `commbank-01` | CommBank | direct_pdf | PENDING | 0 / 0 | NO | Await signed authorization |
| `nab-01` | NAB | direct_pdf | PENDING | 0 / 0 | NO | Await signed authorization |
| `westpac-01` | Westpac | direct_pdf | PENDING | 0 / 0 | NO | Await signed authorization |
| `scribd-bank-template` | Multi-bank | scribd_document | PENDING | 0 / 100 (premium cap) | NO | Scribd premium account required; authorization per bank before scraping |
| `bank_templates` | Template library | audit_evidence | AUTHORIZED | 7 / unlimited | YES (read-only) | Already in repo (`bank_templates/*.yaml`) |
| `audit-evidence` | Verified outputs | audit_evidence | AUTHORIZED | 23 / unlimited | YES (read-only) | Already in repo (`audit-evidence/`) |
| `AU-Bank-Statements` | Statement samples | direct_pdf | PENDING | 0 / 0 | NO | Requires per-institution authorization |
| `Desktop_Archive` | Archived samples | audit_evidence | AUTHORIZED | 5 / unlimited | YES (read-only) | Local archive only |

### Key Security Policy (From AGENTS.md)

- **Automatic scraping is BLOCKED** until authorization status reaches `AUTHORIZED`.
- **Real customer banking data** must never be committed to repository without authorization.
- **Secrets** (`PYTHON_EXECUTABLE`, `.env`) are managed separately; authorization registry references authorization documents (not secrets).
- **Scribd premium account** can be configured (`.env` variable `SCRIBD_PREMIUM_ENABLED`) but does NOT grant automatic scraping rights — it is a document hosting/sharing layer only.

---

## 24/7 Agentic Dataset Pipeline (`agent_dataset_pipeline`)

### Pipeline Architecture (Aligned with `parser_chain.rs`)

```
Source Detection (24/7 polling agent)
  ↓
Authorization Check (auth_registry.json)
  ↓ IF AUTHORIZED
Scraping Agent (direct source OR Scribd)
  ↓
Ingestion Agent (Reducto / Document AI / LlamaParse / Offline Heuristic)
  ↓
Verification Agent (CUDA sub-pixel verifier + audit manifest)
  ↓ IF VERIFIED
Dataset Agent (add to models/template_comparator dataset)
  ↓
Specialist AI Training Agent (batch GPU inference on DGX Spark)
  ↓
Audit Agent (update SHA-256 manifest, update auth_registry count)
  ↓
Feedback Loop (if verification fails → rollback, retry, or flag for human review)
```

### Agent Task Assignments

Each stage runs as a persistent agent (referencing `.agents/skills/bankfidelity/` architecture):

| Agent Role | Source Reference | Function | Trigger Condition |
|---|---|---|---|
| `polling_agent` | `src/app/daemon.rs` (continuous polling) | Check `auth_registry.json` for new `AUTHORIZED` sources | Every 300 seconds |
| `scraping_agent` | `python/` (UFO bridge) | Download PDFs from authorized direct sources or Scribd; store in `cache/mmap/` | Only when `scraping_agent_assigned = true` |
| `ingestion_agent` | `src/app/runtime/parser_chain.rs` | Run parser chain (Reducto → Document AI → LlamaParse → Offline) | After each download |
| `verification_agent` | `src/engine/verification.rs` / `python/spatial_verifier.py` | Sub-pixel CUDA verification + audit manifest | After ingestion |
| `dataset_agent` | `models/template_comparator/` (new) | Add verified PDF to training dataset; update `models/` weights | After verification passes |
| `audit_agent` | `src/app/audit.rs` / `tests/static_analysis.rs` | Update cryptographic manifest; log in `audit-evidence/` | After dataset addition |
| `revision_agent` | `src/app/nlp_router.rs` | If verification fails → apply specialist AI explanation (`balance_explainer`) or rollback (`engine/history.rs`) | After verification fails |

---

## Agent Configuration Integration

The agent team connects via the existing MCP bridge (`src/ai/mcp.rs`) and UFO agent configuration (`.env`, `.agents/skills/ufo_bank_statements/`).

### MCP Updates Required (`.env` variables for 24/7 pipeline)

```
DGX_SPARK_MODE=1
CUDA_BATCH_SIZE=32
SPECIALIST_MODEL_DIR=/models/
AUTH_REGISTRY_FILE=artifacts/auth_registry/index.md
SCRIBD_PREMIUM_ENABLED=false  # Only true after premium account + authorization
SCRAPE_DIRECT_ENABLED=false    # Only true after per-bank authorization
DATASET_MAX_SIZE_GB=2048       # 2TB dataset cap (4TB total - reserve for OS/models)
AGENT_POLL_INTERVAL_SEC=300
```

### UFO Agent Updates (`python/` / `.env` reference)

The UFO agent (`agents.yaml`, `system.yaml`, `mcp.yaml`) must reference specialist model paths instead of generic Qwen:

```yaml
# agents.yaml (updated directive reference)
primary_model: specialist_ensemble
specialist_models:
  font_classifier: /models/font_classifier/surya_deepfont_4k.pt
  layout_regressor: /models/layout_regressor/florence2_pdf_512.pt
  subpixel_verifier: /kernels/subpixel_verifier.cu
  template_comparator: /models/template_comparator/siamese_pdf_4k.pt
  transfer_mapper: /models/transfer_mapper/transfer_encoder_4k.pt
  balance_explainer: /models/balance_explainer/balance_forensic_4k.pt
```

---

## Dataset Pipeline (10,000 PDF Goal — Auth-Tracked Only)

The dataset is built ONLY from `AUTHORIZED` sources. Each PDF added updates:

1. **Dataset count** (tracked in `dataset_tracking/` or `.env` variable `DATASET_PDF_COUNT`)
2. **Specialist model weights** (retrained in batches of 1,000 PDFs)
3. **Audit manifest** (`audit-evidence/` — cryptographic SHA-256 per verified PDF)
4. **Template comparator database** (`models/template_comparator/` — Siamese network compares new pages against all verified examples)

### Dataset Source Priority (Only After Authorization)

| Priority | Source | Authorization Required | Dataset Contribution |
|---|---|---|---|
| 1 | `bank_templates/` | Internal template library (always authorized) | Template reference (5-10 PDFs) |
| 2 | `audit-evidence/` | Internal verified output (always authorized) | Verified edit reference (20+ PDFs) |
| 3 | `AU Bank Statements/` | Per-institution authorization | Real bank statement samples |
| 4 | `Desktop_Archive/` | Local archive authorization (already in `.env` backup) | Historical samples |
| 5 | `bankfidelity/BANKTEST/` | Internal project files (always authorized) | Project test PDFs |
| 6 | Scribd (premium) | Per-bank authorization + premium account | Shared document access |
| 7 | Direct bank sources | Per-bank signed authorization | Primary dataset growth |

---

## Security and Compliance References

This pipeline design aligns with:

- `AGENTS.md` line 148: "May read `.env.example`. Must not read or modify `.env` unless explicitly instructed." → Authorization registry is public metadata; secrets (`.env`) are separate.
- `AGENTS.md` line 70-74: `.env` requires explicit confirmation; authorization registry does NOT contain secrets.
- `AGENTS.md`: Secrets policy — authorization registry references authorization documents (not secret keys).
- `.dockerignore`: `.env` excluded from container; authorization registry included (public metadata).
- `Dockerfile`: Linux container runs only after authorization verified (`DGX_SPARK_MODE` requires authorization check before GPU batch activation).

---

## Implementation Checklist (Agentic Pipeline)

- [x] Authorization registry framework (`artifacts/auth_registry/index.md` — this file)
- [x] Agent pipeline architecture defined (polling → scraping → ingestion → verification → dataset → audit → revision)
- [x] Security boundaries confirmed (automatic scraping BLOCKED without authorization; Scribd only for hosting; 3rd party API must be selected explicitly)
- [ ] Deploy `auth_registry/index.md` as machine-readable JSON (`scripts/auth_registry.json`)
- [ ] Configure `.env` variables (`SCRIBD_PREMIUM_ENABLED`, `SCRAPE_DIRECT_ENABLED`, `AUTH_REGISTRY_FILE`)
- [ ] Update UFO `agents.yaml` / `mcp.yaml` to reference specialist model paths (`models/`)
- [ ] Create `dataset_tracking/` directory with initial count = 0
- [ ] Configure 24/7 agent tasks (`.agents/skills/bankfidelity/` agent instructions updated for dataset pipeline)
- [ ] Obtain first signed authorization per bank (manual process — not automated)
- [ ] Once authorization confirmed → set registry status to `AUTHORIZED`, activate scraping agent
- [ ] Verify first batch → add to dataset, retrain specialist weights, update audit manifest
- [ ] Scale authorization list over time → dataset grows to 10,000 PDF goal

---

## Conclusion

The authorization-tracked agentic dataset pipeline is architecturally defined (`artifacts/auth_registry/index.md`) and aligns with all existing source structures (`parser_chain.rs`, `runtime.rs`, `mcp.rs`, `.env`, `Dockerfile`, `audit.rs`).

**No unauthorized scraping occurs.** The pipeline activates ONLY per verified, signed authorization. Scribd premium serves as document hosting/verification layer, NOT a private data source. The Australian bank audit list grows manually over time. The specialist AI ensemble (`models/`) feeds continuously from verified dataset additions.

**Next concrete action:** Create `scripts/auth_registry.json` (machine-readable) and configure agent tasks (`.agents/skills/bankfidelity/`) for dataset pipeline activation.
