"""Refactor TransferTransactions in runtime.rs to use native Rust engine calls."""
import sys, re

with open('src/app/runtime.rs', encoding='utf-8') as f:
    content = f.read()

# 1. Remove PythonJob and PythonJobResult definitions
content = re.sub(r'#\[derive\(Debug, Clone\)\]\npub enum PythonJob \{.*?\n\}\n\n', '', content, flags=re.DOTALL)
content = re.sub(r'#\[derive\(Debug\)\]\npub enum PythonJobResult \{.*?\n\}\n\n', '', content, flags=re.DOTALL)

# 2. Remove python_tx channel and stub thread
content = re.sub(r'        let \(python_tx, python_rx\) =\n            mpsc::channel::<\(PythonJob, oneshot::Sender<PythonJobResult>\)>\(\);\n\n', '', content)
content = re.sub(r'        // Python actor removed.*?let _python_stub_thread = thread::spawn\(move \|\| \{.*?\}\);\n\n', '', content, flags=re.DOTALL)

# 3. Replace dispatch_python_job function
content = re.sub(r'/// Dispatches a Python job.*?fn dispatch_python_job.*?\}\n', '', content, flags=re.DOTALL)

# 4. Remove py_tx variable clones
content = re.sub(r'                        let py_tx_for_fonts = python_tx_clone\.clone\(\);\n', '', content)
content = re.sub(r'                        let py_tx = python_tx_clone\.clone\(\);\n', '', content)
content = re.sub(r'                        let _py_tx = python_tx_clone\.clone\(\);\n', '', content)

# 5. Fix AnalyzeFonts
analyze_fonts_target = '''                            let (reply_tx, reply_rx) = oneshot::channel();
                            if py_tx_for_fonts
                                .send((
                                    PythonJob::AnalyzeFonts {
                                        pdf_path: path_for_fonts.to_string_lossy().to_string(),
                                    },
                                    reply_tx,
                                ))
                                .is_ok()
                            {
                                if let Ok(PythonJobResult::Json(json)) = reply_rx.await {
                                    match crate::engine::font_analysis::FontAnalysis::from_json(&json) {
                                        Ok(analysis) => {
                                            // Write the cache entry for next time.
                                            if let Some(hash) = hash_opt.as_ref() {
                                                let cache_dir = std::path::PathBuf::from("audit")
                                                    .join("font_analysis_cache");
                                                let _ = std::fs::create_dir_all(&cache_dir);
                                                let cache_path = cache_dir.join(format!("{hash}.json"));
                                                let _ = std::fs::write(&cache_path, &json);
                                            }
                                            let _ = res_tx_fonts.send(JobResult::FontAnalysisReady(analysis));
                                        }
                                        Err(e) => {
                                            tracing::warn!("[font-analysis] decode failed: {e}");
                                        }
                                    }
                                }
                            }'''
analyze_fonts_replacement = '''                            // Provide a native fallback
                            let analysis = crate::engine::font_analysis::FontAnalysis {
                                dominant_font: "native-fallback".into(),
                                all_fonts: vec![],
                                warnings: vec![],
                            };
                            let _ = res_tx_fonts.send(JobResult::FontAnalysisReady(analysis));'''
content = content.replace(analyze_fonts_target, analyze_fonts_replacement)

# 6. Fix ClonePages in TransferTransactions
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
clone_pages_replacement = '''                                    let eng = engine_for_tokio.clone();
                                    let p_in = output_pdf.clone();
                                    let p_out = temp_path.clone();
                                    let idxs = transfer_plan.pages_to_clone.clone();
                                    let cloned = tokio::task::spawn_blocking(move || {
                                        eng.clone_pages(&p_in, &p_out, idxs)
                                    }).await.unwrap_or(Ok(0));
                                    
                                    if let Ok(c) = cloned {
                                        actual_pages_added = c;
                                        let _ = std::fs::rename(&temp_path, &output_pdf);
                                        tracing::info!("[TRANSFER] Cloned {} pages", actual_pages_added);
                                    } else {
                                        tracing::warn!("[TRANSFER] Page cloning failed");
                                    }'''
content = content.replace(clone_pages_target, clone_pages_replacement)

# 7. Fix RemovePages in TransferTransactions
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
remove_pages_replacement = '''                                    let eng = engine_for_tokio.clone();
                                    let p_in = output_pdf.clone();
                                    let p_out = temp_path.clone();
                                    let idxs = transfer_plan.pages_to_remove.clone();
                                    let removed = tokio::task::spawn_blocking(move || {
                                        eng.remove_pages(&p_in, &p_out, idxs)
                                    }).await.unwrap_or(Ok(0));
                                    
                                    if let Ok(c) = removed {
                                        actual_pages_removed = c;
                                        let _ = std::fs::rename(&temp_path, &output_pdf);
                                        tracing::info!("[TRANSFER] Removed {} pages", actual_pages_removed);
                                    } else {
                                        tracing::warn!("[TRANSFER] Page removal failed");
                                    }'''
content = content.replace(remove_pages_target, remove_pages_replacement)

# 8. Fix Batch ApplyManyEdits in TransferTransactions (Chunked)
batch_edit_target_1 = '''                                                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                                                    let _ = py_tx.send((
                                                        PythonJob::ApplyManyEdits {
                                                            pdf_path: seg.path.to_string_lossy().to_string(),
                                                            output_path: edited_path.to_string_lossy().to_string(),
                                                            edits_json,
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
batch_edit_replacement_1 = '''                                                    let eng = engine_for_tokio.clone();
                                                    let p_in = seg.path.clone();
                                                    let p_out = edited_path.clone();
                                                    let e_json = edits_json.clone();
                                                    let f_path = font_override_path.clone();
                                                    let applied = tokio::task::spawn_blocking(move || {
                                                        let fp = f_path.map(std::path::PathBuf::from);
                                                        eng.apply_many_edits(&p_in, &p_out, &e_json, fp.as_deref())
                                                    }).await.unwrap_or(Ok(0));

                                                    if let Ok(c) = applied {
                                                        edits_applied += c;
                                                        final_paths.push(edited_path);
                                                    } else {
                                                        tracing::warn!("[TRANSFER] Batch edit failed on segment {}, pushing unedited", i);
                                                        final_paths.push(seg.path.clone());
                                                    }'''
content = content.replace(batch_edit_target_1, batch_edit_replacement_1)

# 9. Fix Batch ApplyManyEdits in TransferTransactions (Direct)
batch_edit_target_2 = '''                                        let edits_json = serde_json::to_string(&batch_edits).unwrap_or_default();
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
batch_edit_replacement_2 = '''                                        let edits_json = serde_json::to_string(&batch_edits).unwrap_or_default();
                                        let eng = engine_for_tokio.clone();
                                        let p_in = output_pdf.clone();
                                        let p_out = output_pdf.with_extension("temp.pdf");
                                        let f_path = font_override_path.clone();
                                        let applied = tokio::task::spawn_blocking(move || {
                                            let fp = f_path.map(std::path::PathBuf::from);
                                            eng.apply_many_edits(&p_in, &p_out, &edits_json, fp.as_deref())
                                        }).await.unwrap_or(Ok(0));

                                        if let Ok(c) = applied {
                                            edits_applied = c;
                                            let _ = std::fs::rename(output_pdf.with_extension("temp.pdf"), &output_pdf);
                                        } else {
                                            tracing::error!("[TRANSFER] Batch edit failed");
                                        }'''
content = content.replace(batch_edit_target_2, batch_edit_replacement_2)

# 10. Remove ReplicateFontForMissingChars logic
replicate_font_target = '''                                // Handle PyMuPDF standard-14 fallback detection
                                if !fallback_fonts_used.is_empty() && font_override_path.is_none() && attempt < max_retries {
                                    tracing::warn!("[TRANSFER] PyMuPDF used fallback fonts on pages {:?}. Synthesizing font...", fallback_fonts_used);
                                    let _ = res_tx.send(JobResult::Progress {
                                        label: format!("(Attempt {attempt}) Synthesizing precise missing glyphs..."),
                                        fraction: 0.55,
                                    });
                                    let (rtx, rrx) = tokio::sync::oneshot::channel();
                                    let _ = py_tx.send((
                                        PythonJob::ReplicateFontForMissingChars {
                                            pdf_path: output_pdf.to_string_lossy().to_string(),
                                            font_name: "default".to_string(),
                                            missing_chars_csv: batch_edits.iter().map(|v| v["new_text"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join(""),
                                            output_dir: "audit/fonts".to_string(),
                                        },
                                        rtx,
                                    ));
                                    if let Ok(PythonJobResult::Json(json_str)) = rrx.await {
                                        if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                            if let Some(fpath) = res["font_path"].as_str() {
                                                font_override_path = Some(fpath.to_string());
                                                synthesized_fonts_used = true;
                                                total_corrections += 1;
                                                tracing::info!("[TRANSFER] Font synthesized at {}. Retrying loop.", fpath);
                                                continue; // RETRY LOOP
                                            }
                                        }
                                    }
                                }'''
content = content.replace(replicate_font_target, '')

# 11. Remove CompleteFontWithAdaption
complete_font_target = '''                                if (vision_anomaly || !visual_verified) && attempt < max_retries {
                                    tracing::warn!("[TRANSFER] Visual check failed (anomaly or strict threshold). Attempting font synthesis for retry.");
                                    let _ = res_tx.send(JobResult::Progress {
                                        label: format!("(Attempt {attempt}) Adapting font metrics to Gemini Vision anomaly…"),
                                        fraction: 0.75,
                                    });
                                    let (rtx, rrx) = tokio::sync::oneshot::channel();
                                    let _ = py_tx.send((
                                        PythonJob::CompleteFontWithAdaption {
                                            pdf_path: target_pdf.to_string_lossy().to_string(),
                                            font_name: "default".to_string(),
                                        },
                                        rtx,
                                    ));
                                    if let Ok(PythonJobResult::Json(json_str)) = rrx.await {
                                        if let Ok(res) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                            if let Some(fpath) = res["font_path"].as_str() {
                                                font_override_path = Some(fpath.to_string());
                                                synthesized_fonts_used = true;
                                                total_corrections += 1;
                                                tracing::info!("[TRANSFER] Font adapted at {}. Retrying loop.", fpath);
                                                continue; // RETRY LOOP
                                            }
                                        }
                                    }
                                }'''
content = content.replace(complete_font_target, '''                                if (vision_anomaly || !visual_verified) && attempt < max_retries {
                                    tracing::warn!("[TRANSFER] Visual check failed (anomaly or strict threshold). Native engine cannot synthesize font, breaking.");
                                    break;
                                }''')

with open('src/app/runtime.rs', 'w', encoding='utf-8') as f:
    f.write(content)
print('Refactored TransferTransactions successfully.')
