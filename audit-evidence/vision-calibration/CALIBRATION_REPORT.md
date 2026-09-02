# BankFidelity Vision AI Sub-Pixel Calibration & Verification Report
**Timestamp:** 2026-09-02 03:18:46 UTC
**Verification Engine:** 300 DPI Dual-Rasterization & Pure-NumPy SSIM / PSNR Heatmap Analyzer

## Calibration & Fidelity Scorecard
| Bank Statement | Document Name | Typography Spans | Global SSIM | Global PSNR (dB) | Header Invariant SSIM | Calibration Status |
|---|---|---|---|---|---|---|
| **COMMBANK** | `commbank_smartaccess_example.pdf` | 112 | `0.998378` | `33.20` | `1.000000` | **CALIBRATED_PASSED** |
| **BANKWEST** | `bankwest_example.pdf` | 92 | `0.996274` | `29.65` | `1.000000` | **CALIBRATED_PASSED** |
| **ING** | `ing_orange_au.pdf` | 26 | `0.998062` | `32.52` | `1.000000` | **CALIBRATED_PASSED** |
| **MACQUARIE** | `macquarie_au.pdf` | 22 | `0.997973` | `32.52` | `1.000000` | **CALIBRATED_PASSED** |
| **WESTPAC** | `westpac_choice_basic_au.pdf` | 26 | `0.997959` | `32.33` | `1.000000` | **CALIBRATED_PASSED** |
| **ANZ_PLUS** | `anz_plus_au.pdf` | 30 | `0.998318` | `33.03` | `1.000000` | **CALIBRATED_PASSED** |

## Sub-Pixel Visual Invariant Verification
- **Header Invariant Policy:** Header areas (logos, bank signatures, metadata) maintained $SSIM \ge 0.999$, ensuring zero unintended drift.
- **Transaction Row Targeted In-Place Mutation:** Text was redacted and re-rendered at exact sub-pixel optical baseline origins with identical font sizes.
- **Heatmap Telemetry:** Difference heatmaps generated in `audit-evidence/vision-calibration/` confirm localized, surgical modification without full-page re-rasterization artifacts.

## Generated Evidence Artifacts
- Output Directory: `C:\bankfidelity\bankfidelity\audit-evidence\vision-calibration`
- **COMMBANK:** `orig_300dpi.png`, `edited_300dpi.png`, `diff_heatmap.png`
- **BANKWEST:** `orig_300dpi.png`, `edited_300dpi.png`, `diff_heatmap.png`
- **ING:** `orig_300dpi.png`, `edited_300dpi.png`, `diff_heatmap.png`
- **MACQUARIE:** `orig_300dpi.png`, `edited_300dpi.png`, `diff_heatmap.png`
- **WESTPAC:** `orig_300dpi.png`, `edited_300dpi.png`, `diff_heatmap.png`
- **ANZ_PLUS:** `orig_300dpi.png`, `edited_300dpi.png`, `diff_heatmap.png`