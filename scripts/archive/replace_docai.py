import re

with open('src/ai/document_ai.rs', 'r') as f:
    content = f.read()

# Define the new struct definitions
struct_defs = """
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiResponse {
    document: DocAiDocument,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiDocument {
    #[serde(default)]
    pages: Vec<DocAiPage>,
    #[serde(default)]
    entities: Vec<DocAiEntity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiPage {
    dimension: Option<DocAiDimension>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiDimension {
    width: Option<f64>,
    height: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiEntity {
    #[serde(rename = "type")]
    entity_type: String,
    mention_text: Option<String>,
    confidence: Option<f32>,
    page_anchor: Option<DocAiPageAnchor>,
    #[serde(default)]
    properties: Vec<DocAiEntity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiPageAnchor {
    #[serde(default)]
    page_refs: Vec<DocAiPageRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiPageRef {
    #[serde(default)]
    page: Option<serde_json::Value>,
    bounding_poly: Option<DocAiBoundingPoly>,
}

impl DocAiPageRef {
    fn page_idx(&self) -> usize {
        match &self.page {
            Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) as usize,
            _ => 0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiBoundingPoly {
    #[serde(default)]
    normalized_vertices: Vec<DocAiVertex>,
    #[serde(default)]
    vertices: Vec<DocAiVertex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocAiVertex {
    x: Option<f32>,
    y: Option<f32>,
}

fn extract_string_property(entity: &DocAiEntity, kind: &str) -> Option<String> {
    entity.properties
        .iter()
        .find(|p| p.entity_type == kind)
        .and_then(|p| p.mention_text.as_deref())
        .map(|s| s.trim().to_string())
}

fn extract_number_property(entity: &DocAiEntity, kind: &str) -> Option<Decimal> {
    extract_string_property(entity, kind)
        .and_then(|s| s.replace(['$', ','], "").parse::<f64>().ok())
        .map(f64_to_dec)
}

fn bbox_from_bounding_poly(
    poly: &DocAiBoundingPoly,
    page_idx: usize,
    pages_dim: &[(f32, f32, String)],
) -> Option<[f32; 4]> {
    let is_norm = !poly.normalized_vertices.is_empty();
    let verts = if is_norm {
        &poly.normalized_vertices
    } else {
        &poly.vertices
    };

    if verts.is_empty() {
        return None;
    }

    let mut x0 = f32::MAX;
    let mut y0 = f32::MAX;
    let mut x1 = f32::MIN;
    let mut y1 = f32::MIN;

    for v in verts {
        let x = v.x.unwrap_or(0.0);
        let y = v.y.unwrap_or(0.0);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }

    if is_norm {
        if let Some(&(w, h, _)) = pages_dim.get(page_idx) {
            x0 *= w;
            x1 *= w;
            y0 *= h;
            y1 *= h;
        }
    }
    Some([x0, y0, x1, y1])
}

fn entity_page_and_bbox(
    entity: &DocAiEntity,
    pages_dim: &[(f32, f32, String)],
) -> (usize, Option<[f32; 4]>) {
    let Some(anchor) = &entity.page_anchor else { return (0, None); };
    let Some(first) = anchor.page_refs.first() else { return (0, None); };
    let page_idx = first.page_idx();
    let bbox = first.bounding_poly.as_ref().and_then(|p| bbox_from_bounding_poly(p, page_idx, pages_dim));
    (page_idx, bbox)
}

fn property_bbox(
    entity: &DocAiEntity,
    kinds: &[&str],
    pages_dim: &[(f32, f32, String)],
) -> Option<[f32; 4]> {
    for kind in kinds {
        for p in &entity.properties {
            if p.entity_type == *kind {
                let (_, bbox) = entity_page_and_bbox(p, pages_dim);
                if bbox.is_some() {
                    return bbox;
                }
            }
        }
    }
    None
}
"""

new_parse_fn = """
    fn parse_response_into_bank_statement(
        result: &serde_json::Value,
        real_page_dims: Option<&std::collections::HashMap<usize, (f32, f32)>>,
    ) -> Result<BankStatement, DocAiError> {
        let response: DocAiResponse = serde_json::from_value(result.clone())?;
        let total_pages = response.document.pages.len();

        let pages_dim: Vec<(f32, f32, String)> = response.document.pages
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if let Some(real_dims) = real_page_dims {
                    if let Some(&(rw, rh)) = real_dims.get(&i) {
                        return Ok((rw, rh, "points".to_string()));
                    }
                }

                let dim = p.dimension.as_ref().ok_or_else(|| {
                    DocAiError::Parse(serde::de::Error::custom(format!(
                        "Missing physical page dimensions for page {} and no real dimensions provided", i
                    )))
                })?;
                let w = dim.width.ok_or_else(|| {
                    DocAiError::Parse(serde::de::Error::custom("Missing width"))
                })? as f32;
                let h = dim.height.ok_or_else(|| {
                    DocAiError::Parse(serde::de::Error::custom("Missing height"))
                })? as f32;
                Ok((w, h, "points".to_string()))
            })
            .collect::<Result<Vec<_>, DocAiError>>()?;

        let mut transactions = Vec::new();
        let mut opening_balance = Decimal::new(0, 2);
        let mut closing_balance = Decimal::new(0, 2);
        let mut account_number = None;

        for entity in &response.document.entities {
            let (page_idx, row_bbox) = entity_page_and_bbox(entity, &pages_dim);
            let row_bbox = row_bbox.unwrap_or_default();
            let idx = transactions.len();
            let text = entity.mention_text.as_deref().unwrap_or_default().trim().to_string();
            let confidence = entity.confidence.unwrap_or(1.0);

            match entity.entity_type.as_str() {
                "table_item" => {
                    let date = extract_string_property(entity, "transaction_deposit_date")
                        .or_else(|| extract_string_property(entity, "transaction_withdrawal_date"))
                        .or_else(|| extract_string_property(entity, "transaction_date"))
                        .unwrap_or_default();

                    let description = extract_string_property(entity, "transaction_deposit_description")
                        .or_else(|| extract_string_property(entity, "transaction_withdrawal_description"))
                        .or_else(|| extract_string_property(entity, "transaction_description"))
                        .unwrap_or_else(|| text.clone());

                    let credit = extract_number_property(entity, "transaction_deposit");
                    let debit = extract_number_property(entity, "transaction_withdrawal");
                    let running_balance = extract_number_property(entity, "running_balance")
                        .or_else(|| extract_number_property(entity, "transaction_balance"));

                    if credit.is_none() && debit.is_none() && running_balance.is_none() {
                        continue;
                    }

                    let field_bboxes = FieldBboxes {
                        date: property_bbox(
                            entity,
                            &["transaction_deposit_date", "transaction_withdrawal_date", "transaction_date"],
                            &pages_dim,
                        ),
                        description: property_bbox(
                            entity,
                            &["transaction_deposit_description", "transaction_withdrawal_description", "transaction_description"],
                            &pages_dim,
                        ),
                        debit: property_bbox(entity, &["transaction_withdrawal", "debit"], &pages_dim),
                        credit: property_bbox(entity, &["transaction_deposit", "credit"], &pages_dim),
                        running_balance: property_bbox(entity, &["running_balance", "transaction_balance"], &pages_dim),
                    };

                    transactions.push(Transaction {
                        page: page_idx,
                        line_on_page: idx,
                        date,
                        raw_text: description,
                        debit,
                        credit,
                        running_balance,
                        bbox: row_bbox,
                        field_bboxes,
                        provenance: Provenance::DocumentAI { confidence },
                    });
                }
                "transaction" => {
                    let field_bboxes = FieldBboxes {
                        date: property_bbox(
                            entity,
                            &["transaction_date", "transaction_deposit_date", "transaction_withdrawal_date"],
                            &pages_dim,
                        ),
                        description: property_bbox(
                            entity,
                            &["transaction_description", "transaction_deposit_description", "transaction_withdrawal_description"],
                            &pages_dim,
                        ),
                        debit: property_bbox(entity, &["debit", "transaction_withdrawal"], &pages_dim),
                        credit: property_bbox(entity, &["credit", "transaction_deposit"], &pages_dim),
                        running_balance: property_bbox(entity, &["running_balance"], &pages_dim),
                    };

                    let date = extract_string_property(entity, "transaction_date")
                        .or_else(|| extract_string_property(entity, "transaction_deposit_date"))
                        .or_else(|| extract_string_property(entity, "transaction_withdrawal_date"))
                        .unwrap_or_default();

                    let description = extract_string_property(entity, "transaction_description")
                        .or_else(|| extract_string_property(entity, "transaction_deposit_description"))
                        .or_else(|| extract_string_property(entity, "transaction_withdrawal_description"))
                        .unwrap_or_else(|| text.clone());

                    transactions.push(Transaction {
                        page: page_idx,
                        line_on_page: idx,
                        date,
                        raw_text: description,
                        debit: extract_number_property(entity, "debit")
                            .or_else(|| extract_number_property(entity, "transaction_withdrawal")),
                        credit: extract_number_property(entity, "credit")
                            .or_else(|| extract_number_property(entity, "transaction_deposit")),
                        running_balance: extract_number_property(entity, "running_balance"),
                        bbox: row_bbox,
                        field_bboxes,
                        provenance: Provenance::DocumentAI { confidence },
                    });
                }
                "starting_balance" | "opening_balance" => {
                    if let Ok(v) = text.replace(['$', ','], "").parse::<f64>() {
                        opening_balance = f64_to_dec(v);
                    }
                }
                "ending_balance" | "closing_balance" => {
                    if let Ok(v) = text.replace(['$', ','], "").parse::<f64>() {
                        closing_balance = f64_to_dec(v);
                    }
                }
                "account_number" => {
                    if !text.is_empty() {
                        account_number = Some(text);
                    }
                }
                _ => {}
            }
        }

        Ok(BankStatement {
            total_pages,
            transactions,
            opening_balance,
            closing_balance,
            account_number,
        })
    }"""

def find_block(text, start_pattern, end_brace_count=0):
    match = re.search(start_pattern, text)
    if not match:
        return -1, -1
    start_idx = match.start()
    
    braces = 0
    in_block = False
    for i in range(match.end(), len(text)):
        if text[i] == '{':
            braces += 1
            in_block = True
        elif text[i] == '}':
            braces -= 1
        if in_block and braces == end_brace_count:
            return start_idx, i + 1
    return -1, -1

# 1. Replace parse_response_into_bank_statement
parse_start, parse_end = find_block(content, r'fn parse_response_into_bank_statement')
content = content[:parse_start] + new_parse_fn.strip() + content[parse_end:]

# 2. Replace helper functions at the bottom. We replace from `fn extract_string_property` to the end of `fn property_bbox`
helpers_start = content.find('fn extract_string_property')

# Find the end of `fn property_bbox`
_, bbox_end = find_block(content[helpers_start:], r'fn property_bbox')
helpers_end = helpers_start + bbox_end

content = content[:helpers_start] + struct_defs.strip() + "\n" + content[helpers_end:]

with open('src/ai/document_ai.rs', 'w') as f:
    f.write(content)
