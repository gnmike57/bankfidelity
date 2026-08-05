#!/usr/bin/env python3
"""Apply hybrid native-first-with-Python-fallback logic to src/app/runtime.rs.

A *verified*, *idempotent*, step-by-step source transformer.

Each transformation is an independently checked step with one of three states:

  APPLIED   the stub target was found and successfully replaced;
  ALREADY   the target is gone and the migrated result is already present (no-op);
  MISSING   neither the stub target nor the migrated result was found.

Unlike the previous version, this script never prints blanket "success":

  * it reports the exact state of every step (so it is debuggable step-by-step);
  * it writes a timestamped .bak backup before saving;
  * it only writes the file when a step actually changed something;
  * `--dry-run` analyses without writing; `--strict` makes any MISSING step a
    hard, non-zero-exit failure (useful for CI).

Usage:
    python scripts/build_hybrid.py [--file PATH] [--dry-run] [--strict]
                                   [--no-backup] [-v]
"""
import argparse
import datetime
import re
import shutil
import sys

_VERBOSE = False
_steps = []  # list of (name, status, detail)


def _log(msg):
    print(msg, flush=True)


def _record(name, status, detail=""):
    _steps.append((name, status, detail))
    icon = {"APPLIED": "[+]", "ALREADY": "[=]", "MISSING": "[!]"}.get(status, "[?]")
    line = f"  {icon} {name}: {status}"
    if detail and (_VERBOSE or status == "MISSING"):
        line += f" -- {detail}"
    _log(line)


def apply_regex_step(name, content, pattern, replacement, applied_marker):
    """Replace regex `pattern` with `replacement`, verifying the outcome.

    `applied_marker` is a substring present once the migration has been applied;
    it lets us recognise an already-migrated file instead of failing on it."""
    matches = re.findall(pattern, content, flags=re.DOTALL)
    if matches:
        new_content = re.sub(pattern, replacement, content, flags=re.DOTALL)
        if new_content == content:
            _record(name, "MISSING", "pattern matched but substitution changed nothing")
            return content
        _record(name, "APPLIED", f"{len(matches)} match(es) replaced")
        return new_content
    if applied_marker and applied_marker in content:
        _record(name, "ALREADY", f"marker present: {applied_marker!r}")
        return content
    _record(name, "MISSING", "stub pattern not found and no applied-marker present")
    return content


def apply_literal_step(name, content, target, replacement):
    """Replace a literal `target` block with `replacement`, verifying outcome."""
    if target in content:
        n = content.count(target)
        _record(name, "APPLIED", f"{n} occurrence(s) replaced")
        return content.replace(target, replacement)
    if replacement in content:
        _record(name, "ALREADY", "migrated hybrid block already present")
        return content
    _record(name, "MISSING", "stub target block not found and hybrid block absent")
    return content


parser = argparse.ArgumentParser(
    description="Verified, idempotent native-first hybrid transformer for runtime.rs"
)
parser.add_argument("--file", default="src/app/runtime.rs", help="Rust file to transform")
parser.add_argument("--dispatch", default="extracted_dispatch.rs", help="real dispatch fn source")
parser.add_argument("--thread", default="extracted_thread.rs", help="real python-actor thread source")
parser.add_argument("--dry-run", action="store_true", help="analyse only; do not write")
parser.add_argument("--strict", action="store_true", help="treat any MISSING step as a failure")
parser.add_argument("--no-backup", action="store_true", help="do not write a .bak backup")
parser.add_argument("-v", "--verbose", action="store_true", help="show detail for every step")
args = parser.parse_args()
_VERBOSE = args.verbose

_log("== build_hybrid: native-first hybrid transform ==")
_log(f"  target file : {args.file}")
_log(f"  mode        : {'DRY-RUN' if args.dry_run else 'WRITE'}")
_log("")

try:
    with open(args.file, "r", encoding="utf-8") as f:
        original = f.read()
    with open(args.dispatch, "r", encoding="utf-8") as f:
        real_dispatch = f.read()
    with open(args.thread, "r", encoding="utf-8") as f:
        real_thread = f.read()
except OSError as e:
    _log(f"[x] Could not read inputs: {e}")
    sys.exit(2)

content = original

# 1. Replace the stub thread with the real Python-actor thread
stub_thread_pattern = r'        // Python actor removed\n        let _python_stub_thread = thread::spawn\(move \|\| \{.*?\n        \}\);\n'
content = apply_regex_step(
    "1. thread stub -> real python actor", content,
    stub_thread_pattern, real_thread, "_python_actor_thread",
)

# 2. Replace the dispatch_python_job stub with the real dispatch function
stub_dispatch_pattern = r'/// Dispatches a Python job\nfn dispatch_python_job.*?\}\n'
content = apply_regex_step(
    "2. dispatch_python_job stub -> real", content,
    stub_dispatch_pattern, real_dispatch, "python actor channel disconnected",
)

# 3. Hybridize ClonePages in TransferTransactions
clone_pages_target = '''                                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                    let _ = py_tx.send((
                                        PythonJob::ClonePages {
                                            pdf_path: output_pdf.to_string_lossy().to_string(),
                                            output_path: temp_path.to_string_lossy().to_string(),
                                            page_indices: transfer_plan.pages_to_clone.clone(),
                                        },
                                        reply_tx,
                                    ));
                                    match reply_rx.await {
                                        Ok(PythonJobResult::Json(json_str)) => {
                                            if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                if res["success"].as_bool().unwrap_or(false) {
                                                    actual_pages_added = res["cloned"].as_u64().unwrap_or(0) as usize;
                                                    let _ = std::fs::rename(&temp_path, &output_pdf);
                                                }
                                            }
                                            tracing::info!("[TRANSFER] Cloned {} pages", actual_pages_added);
                                        }
                                        other => tracing::warn!("[TRANSFER] Page cloning failed: {:?}", other),
                                    }'''
clone_pages_hybrid = '''                                    let eng = engine_for_tokio.clone();
                                    let p_in = output_pdf.clone();
                                    let p_out = temp_path.clone();
                                    let idxs = transfer_plan.pages_to_clone.clone();
                                    let native_res = tokio::task::spawn_blocking(move || {
                                        eng.clone_pages(&p_in, &p_out, idxs)
                                    }).await.unwrap_or(Ok(0));

                                    if let Ok(c) = native_res {
                                        if c > 0 {
                                            actual_pages_added = c;
                                            let _ = std::fs::rename(&temp_path, &output_pdf);
                                            tracing::info!("[TRANSFER] (Native) Cloned {} pages", actual_pages_added);
                                        }
                                    }
                                    
                                    if actual_pages_added == 0 {
                                        tracing::warn!("[TRANSFER] Native ClonePages failed or returned 0. Falling back to Python.");
                                        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                        let _ = py_tx.send((
                                            PythonJob::ClonePages {
                                                pdf_path: output_pdf.to_string_lossy().to_string(),
                                                output_path: temp_path.to_string_lossy().to_string(),
                                                page_indices: transfer_plan.pages_to_clone.clone(),
                                            },
                                            reply_tx,
                                        ));
                                        match reply_rx.await {
                                            Ok(PythonJobResult::Json(json_str)) => {
                                                if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                    if res["success"].as_bool().unwrap_or(false) {
                                                        actual_pages_added = res["cloned"].as_u64().unwrap_or(0) as usize;
                                                        let _ = std::fs::rename(&temp_path, &output_pdf);
                                                    }
                                                }
                                                tracing::info!("[TRANSFER] (Python) Cloned {} pages", actual_pages_added);
                                            }
                                            other => tracing::warn!("[TRANSFER] (Python) Page cloning failed: {:?}", other),
                                        }
                                    }'''
content = apply_literal_step(
    "3. ClonePages -> native-first hybrid", content,
    clone_pages_target, clone_pages_hybrid,
)

# 4. Hybridize RemovePages
remove_pages_target = '''                                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                    let _ = py_tx.send((
                                        PythonJob::RemovePages {
                                            pdf_path: output_pdf.to_string_lossy().to_string(),
                                            output_path: temp_path.to_string_lossy().to_string(),
                                            page_indices: transfer_plan.pages_to_remove.clone(),
                                        },
                                        reply_tx,
                                    ));
                                    match reply_rx.await {
                                        Ok(PythonJobResult::Json(json_str)) => {
                                            if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                if res["success"].as_bool().unwrap_or(false) {
                                                    actual_pages_removed = res["removed"].as_u64().unwrap_or(0) as usize;
                                                    let _ = std::fs::rename(&temp_path, &output_pdf);
                                                }
                                            }
                                            tracing::info!("[TRANSFER] Removed {} pages", actual_pages_removed);
                                        }
                                        other => tracing::warn!("[TRANSFER] Page removal failed: {:?}", other),
                                    }'''
remove_pages_hybrid = '''                                    let eng = engine_for_tokio.clone();
                                    let p_in = output_pdf.clone();
                                    let p_out = temp_path.clone();
                                    let idxs = transfer_plan.pages_to_remove.clone();
                                    let native_res = tokio::task::spawn_blocking(move || {
                                        eng.remove_pages(&p_in, &p_out, idxs)
                                    }).await.unwrap_or(Ok(0));

                                    if let Ok(c) = native_res {
                                        if c > 0 {
                                            actual_pages_removed = c;
                                            let _ = std::fs::rename(&temp_path, &output_pdf);
                                            tracing::info!("[TRANSFER] (Native) Removed {} pages", actual_pages_removed);
                                        }
                                    }

                                    if actual_pages_removed == 0 {
                                        tracing::warn!("[TRANSFER] Native RemovePages failed or returned 0. Falling back to Python.");
                                        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                        let _ = py_tx.send((
                                            PythonJob::RemovePages {
                                                pdf_path: output_pdf.to_string_lossy().to_string(),
                                                output_path: temp_path.to_string_lossy().to_string(),
                                                page_indices: transfer_plan.pages_to_remove.clone(),
                                            },
                                            reply_tx,
                                        ));
                                        match reply_rx.await {
                                            Ok(PythonJobResult::Json(json_str)) => {
                                                if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                    if res["success"].as_bool().unwrap_or(false) {
                                                        actual_pages_removed = res["removed"].as_u64().unwrap_or(0) as usize;
                                                        let _ = std::fs::rename(&temp_path, &output_pdf);
                                                    }
                                                }
                                                tracing::info!("[TRANSFER] (Python) Removed {} pages", actual_pages_removed);
                                            }
                                            other => tracing::warn!("[TRANSFER] (Python) Page removal failed: {:?}", other),
                                        }
                                    }'''
content = apply_literal_step(
    "4. RemovePages -> native-first hybrid", content,
    remove_pages_target, remove_pages_hybrid,
)

# 5. Hybridize ApplyManyEdits (Chunked)
apply_many_target_1 = '''                                                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                                    let _ = py_tx.send((
                                                        PythonJob::ApplyManyEdits {
                                                            pdf_path: seg.path.to_string_lossy().to_string(),
                                                            output_path: edited_path.to_string_lossy().to_string(),
                                                            edits_json: edits_json.clone(),
                                                            font_path: font_override_path.clone(),
                                                        },
                                                        reply_tx,
                                                    ));
                                                    match reply_rx.await {
                                                        Ok(PythonJobResult::Json(json_str)) => {
                                                            if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                                if res["success"].as_bool().unwrap_or(false) {
                                                                    edits_applied += res["applied"].as_u64().unwrap_or(0) as usize;
                                                                    if let Some(flags) = res["review_flags"].as_array() {
                                                                        for f in flags {
                                                                            if let Some(pg) = f.as_u64() {
                                                                                if let Some(gp) = map.to_global(i, pg as usize) {
                                                                                    fallback_fonts_used.push(gp);
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            final_paths.push(edited_path);
                                                        }
                                                        _ => {
                                                            tracing::warn!("[TRANSFER] Batch edit failed on segment {}, pushing unedited", i);
                                                            final_paths.push(seg.path.clone());
                                                        }
                                                    }'''
apply_many_hybrid_1 = '''                                                    let eng = engine_for_tokio.clone();
                                                    let p_in = seg.path.clone();
                                                    let p_out = edited_path.clone();
                                                    let e_json = edits_json.clone();
                                                    let f_path = font_override_path.clone();
                                                    
                                                    let native_res = tokio::task::spawn_blocking(move || {
                                                        let fp = f_path.map(std::path::PathBuf::from);
                                                        eng.apply_many_edits(&p_in, &p_out, &e_json, fp.as_deref())
                                                    }).await.unwrap_or(Ok(0));

                                                    let mut segment_applied = 0;
                                                    if let Ok(c) = native_res {
                                                        segment_applied = c;
                                                    }
                                                    
                                                    if segment_applied > 0 {
                                                        tracing::info!("[TRANSFER] (Native) Batch edit segment {} succeeded", i);
                                                        edits_applied += segment_applied;
                                                        final_paths.push(edited_path);
                                                    } else {
                                                        tracing::warn!("[TRANSFER] Native ApplyManyEdits failed or returned 0. Falling back to Python.");
                                                        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                                        let _ = py_tx.send((
                                                            PythonJob::ApplyManyEdits {
                                                                pdf_path: seg.path.to_string_lossy().to_string(),
                                                                output_path: edited_path.to_string_lossy().to_string(),
                                                                edits_json: edits_json.clone(),
                                                                font_path: font_override_path.clone(),
                                                            },
                                                            reply_tx,
                                                        ));
                                                        match reply_rx.await {
                                                            Ok(PythonJobResult::Json(json_str)) => {
                                                                if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                                    if res["success"].as_bool().unwrap_or(false) {
                                                                        edits_applied += res["applied"].as_u64().unwrap_or(0) as usize;
                                                                        if let Some(flags) = res["review_flags"].as_array() {
                                                                            for f in flags {
                                                                                if let Some(pg) = f.as_u64() {
                                                                                    if let Some(gp) = map.to_global(i, pg as usize) {
                                                                                        fallback_fonts_used.push(gp);
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                final_paths.push(edited_path);
                                                            }
                                                            _ => {
                                                                tracing::warn!("[TRANSFER] (Python) Batch edit failed on segment {}, pushing unedited", i);
                                                                final_paths.push(seg.path.clone());
                                                            }
                                                        }
                                                    }'''
content = apply_literal_step(
    "5. ApplyManyEdits (chunked) -> native-first hybrid", content,
    apply_many_target_1, apply_many_hybrid_1,
)

# 6. Hybridize ApplyManyEdits (Direct)
apply_many_target_2 = '''                                        let edits_json = serde_json::to_string(&batch_edits).unwrap_or_default();
                                        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                        let _ = py_tx.send((
                                            PythonJob::ApplyManyEdits {
                                                pdf_path: output_pdf.to_string_lossy().to_string(),
                                                output_path: output_pdf.with_extension("temp.pdf").to_string_lossy().to_string(),
                                                edits_json,
                                                font_path: font_override_path.clone(),
                                            },
                                            reply_tx,
                                        ));

                                        match reply_rx.await {
                                            Ok(PythonJobResult::Json(json_str)) => {
                                                if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                    if res["success"].as_bool().unwrap_or(false) {
                                                        edits_applied = res["applied"].as_u64().unwrap_or(0) as usize;
                                                        if let Some(flags) = res["review_flags"].as_array() {
                                                            for f in flags {
                                                                if let Some(pg) = f.as_u64() {
                                                                    fallback_fonts_used.push(pg as usize);
                                                                }
                                                            }
                                                        }
                                                        let _ = std::fs::rename(output_pdf.with_extension("temp.pdf"), &output_pdf);
                                                    }
                                                }
                                            }
                                            Ok(PythonJobResult::Error(e)) => tracing::error!("[TRANSFER] Batch edit failed: {}", e),
                                            _ => tracing::error!("[TRANSFER] Batch edit failed with unexpected result"),
                                        }'''
apply_many_hybrid_2 = '''                                        let edits_json = serde_json::to_string(&batch_edits).unwrap_or_default();
                                        let eng = engine_for_tokio.clone();
                                        let p_in = output_pdf.clone();
                                        let p_out = output_pdf.with_extension("temp.pdf");
                                        let f_path = font_override_path.clone();
                                        
                                        let native_res = tokio::task::spawn_blocking(move || {
                                            let fp = f_path.map(std::path::PathBuf::from);
                                            eng.apply_many_edits(&p_in, &p_out, &edits_json, fp.as_deref())
                                        }).await.unwrap_or(Ok(0));

                                        if let Ok(c) = native_res {
                                            if c > 0 {
                                                edits_applied = c;
                                                let _ = std::fs::rename(output_pdf.with_extension("temp.pdf"), &output_pdf);
                                                tracing::info!("[TRANSFER] (Native) Batch edit succeeded");
                                            }
                                        }

                                        if edits_applied == 0 {
                                            tracing::warn!("[TRANSFER] Native ApplyManyEdits failed or returned 0. Falling back to Python.");
                                            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                            let _ = py_tx.send((
                                                PythonJob::ApplyManyEdits {
                                                    pdf_path: output_pdf.to_string_lossy().to_string(),
                                                    output_path: output_pdf.with_extension("temp.pdf").to_string_lossy().to_string(),
                                                    edits_json,
                                                    font_path: font_override_path.clone(),
                                                },
                                                reply_tx,
                                            ));

                                            match reply_rx.await {
                                                Ok(PythonJobResult::Json(json_str)) => {
                                                    if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                                        if res["success"].as_bool().unwrap_or(false) {
                                                            edits_applied = res["applied"].as_u64().unwrap_or(0) as usize;
                                                            if let Some(flags) = res["review_flags"].as_array() {
                                                                for f in flags {
                                                                    if let Some(pg) = f.as_u64() {
                                                                        fallback_fonts_used.push(pg as usize);
                                                                    }
                                                                }
                                                            }
                                                            let _ = std::fs::rename(output_pdf.with_extension("temp.pdf"), &output_pdf);
                                                            tracing::info!("[TRANSFER] (Python) Batch edit succeeded");
                                                        }
                                                    }
                                                }
                                                Ok(PythonJobResult::Error(e)) => tracing::error!("[TRANSFER] (Python) Batch edit failed: {}", e),
                                                _ => tracing::error!("[TRANSFER] (Python) Batch edit failed with unexpected result"),
                                            }
                                        }'''
content = apply_literal_step(
    "6. ApplyManyEdits (direct) -> native-first hybrid", content,
    apply_many_target_2, apply_many_hybrid_2,
)

# --- Summary + verified, safe write -------------------------------------
_log("")
applied = [s for s in _steps if s[1] == "APPLIED"]
already = [s for s in _steps if s[1] == "ALREADY"]
missing = [s for s in _steps if s[1] == "MISSING"]
changed = content != original

_log("== Summary ==")
_log(f"  steps     : {len(_steps)}")
_log(f"  applied   : {len(applied)}")
_log(f"  already   : {len(already)}")
_log(f"  missing   : {len(missing)}")
_log(f"  changed   : {changed}")

if args.strict and missing:
    _log("")
    _log(f"[x] {len(missing)} step(s) MISSING under --strict; refusing to continue.")
    sys.exit(1)

if not changed:
    _log("")
    _log("[=] Nothing to write: file already matches the desired hybrid state.")
    sys.exit(0)

if args.dry_run:
    _log("")
    _log("[i] DRY-RUN: changes detected but not written.")
    sys.exit(0)

if not args.no_backup:
    ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    backup = f"{args.file}.{ts}.bak"
    shutil.copy2(args.file, backup)
    _log(f"  backup    : {backup}")

with open(args.file, "w", encoding="utf-8") as f:
    f.write(content)
_log(f"[+] Wrote hybrid logic to {args.file} ({len(applied)} step(s) applied).")
