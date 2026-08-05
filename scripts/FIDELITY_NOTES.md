# Fidelity work — 20-item multi-stage plan

> **Context (v1.0.0):** All 20 items below are complete and integrated. The verification pipeline now includes multi-layer checking (SSIM + Tile-max + Perceptual Hash + optional pdfRest + optional Applitools Eyes + optional Gemini Vision). See `src/engine/verification.rs` for the current implementation. Configurable via Backend Preferences UI (`visual_diff_threshold`, `max_visual_attempts`).

Verification harnesses (scripts/):
- `align_diag.py` — round-trips a span to its original value, finds best (dx,dy)
  shift; reports zero-shift + best-aligned 600 DPI L1 vs the untouched original.
  Lower = more faithful. This is the primary regression gate.
- `fidelity_compare.py` / `ink_analysis.py` / `inspect_case.py` / `ascii_view.py`
  — supporting diagnostics.
- `python/_baseline_integration.py` — committed-HEAD copy of the integration
  module, used for before/after comparison. DELETE before finishing.

## Stage A — embedded-font reuse (items 1,2,3,4)  ✅ DONE + verified
- `_extract_font_buffer`, `_resolve_embedded_font`, `_fallback_standard14` added.
- Non-standard fonts: re-embed original glyph program by buffer; measure with a
  pymupdf.Font over the same bytes. Standard-14: emit with the matching builtin
  code (most faithful) — NOT treated as fallback.
- Redaction annot no longer auto-draws text; single font-faithful re-emit.
- Result (align_diag, new vs base, zero-shift L1, lower better):
  - HelveticaNeue-Light2: 0.043→0.030, 0.127→0.112, 0.054→0.038
  - Times Roman/Bold: 0.140→0.130, 0.115→0.086, 0.088→0.061, 0.059→0.032
  - INGMe: 0.102→0.094, 0.076→0.056, 0.067→0.049
  - Westpac Helvetica: 0.252→0.249, 0.089→0.082, 0.023→0.016
  All cases improved or held. No regressions.

## Stage B — kerning & spacing (items 5,6,7)  ✅ DONE
- All condensing flows through `_insert_kerned_text` (no more silent-overflow Tc stub).
- `h_scale` (horizontal scaling) applied before hard tracking once -0.5pt/gap cap hit.
- Per-glyph origin reuse extended to matching prefix+suffix runs (item 7).

## Stage C — placement & baseline (items 8,9)  ✅ DONE
- `_span_writing_dir` (item 8): cursor advances along baseline dir; rotated
  text uses morph. `_snap_origin_phase` (item 9): right-align origin snapped
  to original sub-pixel phase at 600 DPI grid. `aligned`==`zero` confirms no
  residual drift to recover.

## Stage D — colour fidelity (items 10,11)  ✅ DONE
- `_native_fill_color`: emit in Gray/RGB/CMYK native space (item 10).
- Removed "accessible red" substitution; exact colour preserved (item 11).
  Contrast guard now hue-preserving, last-resort only (<0.12 luminance gap).

## Stage E — background & line art (items 12,13,14)  ✅ DONE + verified
- `_tight_glyph_bbox` colour-aware (paper-distance + fg-proximity), item 12.
- `_sample_patch` median sampling for classify_background, item 13.
- `_vector_strokes_through`/`_redraw_strokes` capture+restore cap/join/dash/
  opacity, item 14. Removed orphaned dead tightening block.
- Result: further gains. Westpac '- $202,359.19' 0.082->0.063;
  '- $1,728.52' now 0.0000 (perfect). All other cases held or improved.

## Stage F — font synthesis fidelity (items 15,16)  ✅ DONE + verified
- `_host_class_advances` + `_char_class` + `_shift_glyph_x`: injected donor
  glyphs adopt the host's per-class advance (tabular digits share one width)
  and are re-centred in it (item 15).
- `_infer_weight_width` + `_donor_os2` + scored `_pick_local_donor`: donor
  chosen by OS/2 weight+width proximity, not just name (item 16).
- Verified: test_stage_f.py (helpers) + test_stage_f_e2e.py (donor '7' at
  native 950 normalized to host 600). Existing font_cascade.rs contract held.

## Stage G — verification tightening (items 17,18,19,20)  ✅ DONE + verified
- verification.rs rewritten:
  - #17 tile-max (24px) score blending luminance + Sobel gradient; gate is
    worst tile OUTSIDE intended regions (was whole-page average < 0.02).
  - #18 edited neighbourhoods scored at 600 DPI via render_region_gray.
  - #19 pinned_render_config: same engine + fixed AA (no LCD subpixel) for
    BOTH original and edited.
  - #20 region_fidelity_score: positive gradient-residual check of the
    replacement glyphs after best-shift alignment (max_edit_region_score),
    instead of blanket-masking the edit away.
  - VerificationReport gains max_tile_score + max_edit_region_score
    (#[serde(default)], back-compatible). visual_diff_score still populated.
- Verified: 4 new unit tests, incl. proof tile-max catches a localized drift
  the whole-page average hides. Full lib suite: 117 passed.

## ALL 20 ITEMS COMPLETE. Cleanup: removed python/_baseline_integration.py.
## Kept reusable harness: scripts/align_diag.py, fidelity_compare.py,
## test_stage_f*.py. Scratch probes can be removed.
