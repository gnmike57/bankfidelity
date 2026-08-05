# Document AI Training Handoff

> **Updated:** v1.0.0 — The app now includes in-app Document AI admin via `Job::ListDocAiVersions`, `Job::TrainDocAiVersion`, `Job::DeployDocAiVersion`, `Job::UndeployDocAiVersion`, and `Job::SetDefaultDocAiVersion`. These can replace several of the manual Console steps below once labelling is complete.

## What's already done

- **GCS buckets**
  - `gs://docai-training-1056864635772/` — bucket bound to the existing dataset
  - `gs://esoteric-energy-495909-t7-docai-hns/` — backup HNS bucket (in case we need to re-init)
- **PDFs uploaded** (split where needed to fit Document AI's 15-page training cap):
  ```
  gs://.../au-bank-statements-split/
    807466413-AccountStatement-2026-03-11 2.pdf       (4 pages)
    ANZ Plus Statement March 2026.pdf                  (1 page)
    IA_Bank_Statement_202602.pdf                       (2 pages)
    ING OrangeEveryday.pdf                             (10 pages)
    Westpac BusinessOne (part 1 of 2).pdf              (15 pages)
    Westpac BusinessOne (part 2 of 2).pdf              (3 pages)
    Westpac ChoiceBasic (part 1 of 4).pdf              (15 pages)
    Westpac ChoiceBasic (part 2 of 4).pdf              (15 pages)
    Westpac ChoiceBasic (part 3 of 4).pdf              (15 pages)
    Westpac ChoiceBasic (part 4 of 4).pdf              (12 pages)
  ```
  Total 10 docs, all under the per-doc page cap.
- **Local Rust app**: 5 AU bank templates added under `bank_templates/`:
  - `anz_plus_au.yaml`
  - `ing_orange_au.yaml`
  - `macquarie_au.yaml`
  - `westpac_business_one_au.yaml` (column ranges learned from real layout)
  - `westpac_choice_basic_au.yaml` (column ranges learned from real layout)

## What's pending — manual steps in the Document AI Console

API-driven import keeps hitting `storage.folders.create` permission denial because Google's Bank Statement Parser dataset writer wants HNS-managed folders, and the originally-bound bucket isn't HNS. The Console UI handles the ceremony correctly. Do these:

1. Open **https://console.cloud.google.com/ai/document-ai/locations/us/processors/27af46e84ea84975/train**
2. Click **Import documents** → **Cloud Storage**
3. Source: `gs://docai-training-1056864635772/au-bank-statements-split/`
4. Auto-split: train 80% / test 20%
5. Click Import. The Console picks up where the API failed because it has different internal IAM bindings.
6. Once imported, click any document → **Label** → draw boxes around each:
   - `account_number`, `bank_name`, `client_address`, `client_name`
   - `statement_start_date`, `statement_end_date`
   - `starting_balance`, `ending_balance`
   - For each transaction row: a `table_item` containing `transaction_deposit_date`, `transaction_deposit_description`, and either `transaction_deposit` or `transaction_withdrawal`
7. Mark each labelled doc as **Labelled** (top-right toggle).
8. When at least 8 docs are labelled, click **Train new version**. Choose a name like `au-bank-v1`.
9. Training takes 1–6 hours. You'll get an email.
10. After training, **Manage versions** → set `au-bank-v1` as default.

## How the Rust app picks up the new version

No code changes needed. The processor URL stays the same; setting `au-bank-v1` as the default version routes future calls to it automatically. If you want to keep both pretrained and trained side-by-side, set `DOCUMENT_AI_PROCESSOR_VERSION` in `.env` and add a `processor_version` field to `DocumentAiConfig` (one-line change in `src/ai/document_ai.rs::process_url_v1`).

## If the Console import also fails

Worst case, here's the workaround: **re-initialise the dataset** against the HNS bucket. This destroys any docs already imported.

```powershell
# 1. Mark dataset unmanaged (only works after deleting the processor's existing dataset binding via the Console)
# 2. Re-init against HNS bucket:
$tok = (gcloud auth application-default print-access-token).Trim()
$url = "https://us-documentai.googleapis.com/v1beta3/projects/252607032233/locations/us/processors/27af46e84ea84975/dataset?updateMask=gcsManagedConfig"
$body = '{ "name": "projects/252607032233/locations/us/processors/27af46e84ea84975/dataset", "gcsManagedConfig": { "gcsPrefix": { "gcsUriPrefix": "gs://esoteric-energy-495909-t7-docai-hns/dataset/" } } }'
Invoke-WebRequest -Uri $url -Method Patch -Headers @{ Authorization = "Bearer $tok"; "Content-Type" = "application/json"; "x-goog-user-project" = "esoteric-energy-495909-t7" } -Body $body
```
