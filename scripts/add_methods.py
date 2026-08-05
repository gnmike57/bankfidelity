"""Inject new methods (apply_many_edits, clone_pages, remove_pages) into native_engine.rs."""
import sys

with open('src/pdf/native_engine.rs', encoding='utf-8') as f:
    content = f.read()

new_methods = '''
    fn apply_many_edits(
        &self,
        input: &std::path::Path,
        output: &std::path::Path,
        edits_json: &str,
        _font_path: Option<&std::path::Path>,
    ) -> Result<usize, EngineError> {
        let edits: Vec<serde_json::Value> = serde_json::from_str(edits_json)
            .map_err(|e| EngineError::ApplyFailed(format!("Invalid edits JSON: {}", e)))?;

        let mut doc =
            lopdf::Document::load(input).map_err(|e| EngineError::LoadFailed(format!("{e}")))?;

        let mut applied_count = 0;
        let mut modified_pages = std::collections::HashSet::new();

        let mut edits_by_page: std::collections::HashMap<usize, Vec<&serde_json::Value>> = std::collections::HashMap::new();
        for edit in &edits {
            if let Some(page) = edit["page"].as_u64() {
                edits_by_page.entry(page as usize).or_default().push(edit);
            }
        }

        let pages = doc.get_pages();

        for (page_idx, page_edits) in edits_by_page {
            let page_id = *pages.get(&(page_idx as u32 + 1)).ok_or_else(|| {
                EngineError::ApplyFailed(format!(
                    "Page {} not found",
                    page_idx
                ))
            })?;

            let content_bytes = doc.get_page_content(page_id).unwrap_or_default();
            if content_bytes.is_empty() {
                continue;
            }

            let mut content = match lopdf::content::Content::decode(&content_bytes) {
                Ok(c) => c,
                Err(e) => return Err(EngineError::ApplyFailed(format!("Failed to decode content: {e}"))),
            };

            let mut tm = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];
            let mut tlm = tm;
            let mut font_size: f32 = 12.0;
            let mut in_text = false;

            for op in &mut content.operations {
                match op.operator.as_str() {
                    "BT" => {
                        in_text = true;
                        tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                        tlm = tm;
                    }
                    "ET" => {
                        in_text = false;
                    }
                    "Tf" if in_text => {
                        if op.operands.len() >= 2 {
                            font_size = operand_to_f32(&op.operands[1]).unwrap_or(12.0);
                        }
                    }
                    "Tm" if in_text => {
                        if op.operands.len() >= 6 {
                            for (i, operand) in op.operands.iter().enumerate().take(6) {
                                tm[i] = operand_to_f32(operand).unwrap_or(0.0);
                            }
                            tlm = tm;
                        }
                    }
                    "Td" | "TD" if in_text => {
                        if op.operands.len() >= 2 {
                            let tx = operand_to_f32(&op.operands[0]).unwrap_or(0.0);
                            let ty = operand_to_f32(&op.operands[1]).unwrap_or(0.0);
                            tlm[4] += tx;
                            tlm[5] += ty;
                            tm = tlm;
                        }
                    }
                    "T*" if in_text => {
                        tlm[5] -= font_size;
                        tm = tlm;
                    }
                    "Tj" | "TJ" if in_text => {
                        let x = tm[4];
                        let y = tm[5];
                        for edit in &page_edits {
                            if let Some(rect) = edit["rect"].as_array() {
                                if rect.len() == 4 {
                                    let bbox = [
                                        rect[0].as_f64().unwrap_or(0.0) as f32,
                                        rect[1].as_f64().unwrap_or(0.0) as f32,
                                        rect[2].as_f64().unwrap_or(0.0) as f32,
                                        rect[3].as_f64().unwrap_or(0.0) as f32,
                                    ];
                                    if x >= bbox[0] - 1.0
                                        && y >= bbox[1] - 1.0
                                        && x <= bbox[2] + 1.0
                                        && y <= bbox[3] + 1.0
                                    {
                                        if let Some(new_text) = edit["new_text"].as_str() {
                                            op.operator = "Tj".to_string();
                                            op.operands = vec![lopdf::Object::String(
                                                new_text.as_bytes().to_vec(),
                                                lopdf::StringFormat::Literal,
                                            )];
                                            applied_count += 1;
                                            modified_pages.insert(page_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            if modified_pages.contains(&page_id) {
                let new_content_bytes = content
                    .encode()
                    .map_err(|e| EngineError::ApplyFailed(format!("Failed to encode content: {e}")))?;

                doc.change_page_content(page_id, new_content_bytes)
                    .map_err(|e| EngineError::ApplyFailed(format!("Failed to update page: {e}")))?;
            }
        }

        doc.save(output)
            .map_err(|e| EngineError::ApplyFailed(format!("Failed to save: {e}")))?;

        Ok(applied_count)
    }

    fn clone_pages(
        &self,
        input: &std::path::Path,
        output: &std::path::Path,
        page_indices: Vec<usize>,
    ) -> Result<usize, EngineError> {
        let mut doc = lopdf::Document::load(input)
            .map_err(|e| EngineError::LoadFailed(format!("{e}")))?;
        
        let pages = doc.get_pages();
        let mut cloned = 0;
        
        for &idx in &page_indices {
            if let Some(&page_id) = pages.get(&(idx as u32 + 1)) {
                if let Ok(page_dict) = doc.get_object(page_id) {
                    let page_dict_clone = page_dict.clone();
                    let new_page_id = doc.add_object(page_dict_clone);
                    if doc.insert_page((doc.get_pages().len() as u32) + 1, new_page_id).is_ok() {
                        cloned += 1;
                    }
                }
            }
        }
        
        doc.save(output).map_err(|e| EngineError::ApplyFailed(format!("Failed to save: {e}")))?;
        Ok(cloned)
    }

    fn remove_pages(
        &self,
        input: &std::path::Path,
        output: &std::path::Path,
        page_indices: Vec<usize>,
    ) -> Result<usize, EngineError> {
        let mut doc = lopdf::Document::load(input)
            .map_err(|e| EngineError::LoadFailed(format!("{e}")))?;
        
        let mut page_nums = Vec::new();
        for &idx in &page_indices {
            page_nums.push(idx as u32 + 1);
        }
        
        doc.delete_pages(&page_nums);
        doc.save(output).map_err(|e| EngineError::ApplyFailed(format!("Failed to save: {e}")))?;
        Ok(page_nums.len())
    }
'''

target = '        })\n    }\n}\n'
if target in content:
    content = content.replace(target, '        })\n    }\n' + new_methods + '}\n')
    with open('src/pdf/native_engine.rs', 'w', encoding='utf-8') as f:
        f.write(content)
    print('Inserted new methods successfully.')
else:
    print('Failed to find target in native_engine.rs')
