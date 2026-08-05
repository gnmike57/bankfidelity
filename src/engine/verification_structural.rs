use crate::engine::verification::{VerificationGate, VerificationGateStatus};
use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone)]
struct PageSignature {
    media_box: [f32; 4],
    crop_box: [f32; 4],
    rotation: i64,
    content_nonempty: bool,
    text_nonempty: bool,
    text_anchors: BTreeSet<String>,
    fonts: BTreeSet<String>,
}

fn inherited_object<'a>(
    document: &'a Document,
    mut object_id: ObjectId,
    key: &[u8],
) -> Result<Option<&'a Object>, String> {
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(object_id) {
            return Err(format!(
                "page tree contains a parent cycle at {object_id:?}"
            ));
        }
        let dictionary = document
            .get_dictionary(object_id)
            .map_err(|error| format!("page node {object_id:?} is invalid: {error}"))?;
        if let Ok(object) = dictionary.get(key) {
            let (_, object) = document.dereference(object).map_err(|error| {
                format!(
                    "page node {object_id:?} {} cannot be dereferenced: {error}",
                    String::from_utf8_lossy(key)
                )
            })?;
            return Ok(Some(object));
        }
        object_id = match dictionary.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => parent,
            Err(_) => return Ok(None),
        };
    }
}

fn page_box(
    document: &Document,
    page_id: ObjectId,
    key: &[u8],
) -> Result<Option<[f32; 4]>, String> {
    let Some(object) = inherited_object(document, page_id, key)? else {
        return Ok(None);
    };
    let values = object.as_array().map_err(|error| {
        format!(
            "page {page_id:?} {} is not an array: {error}",
            String::from_utf8_lossy(key)
        )
    })?;
    if values.len() != 4 {
        return Err(format!(
            "page {page_id:?} {} has {} values, expected four",
            String::from_utf8_lossy(key),
            values.len()
        ));
    }
    let mut result = [0.0_f32; 4];
    for (index, value) in values.iter().enumerate() {
        result[index] = value.as_float().map_err(|error| {
            format!(
                "page {page_id:?} {} value {index} is not numeric: {error}",
                String::from_utf8_lossy(key)
            )
        })?;
    }
    Ok(Some(result))
}

fn page_rotation(document: &Document, page_id: ObjectId) -> Result<i64, String> {
    let Some(object) = inherited_object(document, page_id, b"Rotate")? else {
        return Ok(0);
    };
    object
        .as_i64()
        .map(|value| value.rem_euclid(360))
        .map_err(|error| format!("page {page_id:?} Rotate is not an integer: {error}"))
}

fn normalize_font_name(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name)
        .trim_start_matches('/')
        .to_string();
    if name.len() > 7
        && name.as_bytes()[6] == b'+'
        && name.as_bytes()[..6]
            .iter()
            .all(|byte| byte.is_ascii_uppercase())
    {
        name[7..].to_string()
    } else {
        name
    }
}

fn page_fonts(document: &Document, page_id: ObjectId) -> Result<BTreeSet<String>, String> {
    let fonts = document
        .get_page_fonts(page_id)
        .map_err(|error| format!("cannot resolve page {page_id:?} fonts: {error}"))?;
    Ok(fonts
        .values()
        .filter_map(|font| font.get(b"BaseFont").ok())
        .filter_map(|object| object.as_name().ok())
        .map(normalize_font_name)
        .collect())
}

fn text_anchors(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() >= 4)
        .filter(|token| token.chars().any(char::is_alphabetic))
        .map(|token| token.to_lowercase())
        .collect()
}

fn page_signatures(document: &Document) -> Result<Vec<PageSignature>, String> {
    document
        .get_pages()
        .into_iter()
        .map(|(page_number, page_id)| {
            let media_box = page_box(document, page_id, b"MediaBox")?
                .ok_or_else(|| format!("page {page_number} has no inherited MediaBox"))?;
            let crop_box = page_box(document, page_id, b"CropBox")?.unwrap_or(media_box);
            let content = document
                .get_page_content(page_id)
                .map_err(|error| format!("cannot read page {page_number} content: {error}"))?;
            let text = document
                .extract_text(&[page_number])
                .map_err(|error| format!("cannot extract page {page_number} text: {error}"))?;
            Ok(PageSignature {
                media_box,
                crop_box,
                rotation: page_rotation(document, page_id)?,
                content_nonempty: content.iter().any(|byte| !byte.is_ascii_whitespace()),
                text_nonempty: !text.trim().is_empty(),
                text_anchors: text_anchors(&text),
                fonts: page_fonts(document, page_id)?,
            })
        })
        .collect()
}

fn boxes_equal(left: [f32; 4], right: [f32; 4]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() <= 0.01)
}

fn object_text(document: &Document, object: &Object) -> Option<String> {
    let (_, object) = document.dereference(object).ok()?;
    match object {
        Object::String(bytes, _) | Object::Name(bytes) => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        Object::Integer(value) => Some(value.to_string()),
        Object::Real(value) => Some(value.to_string()),
        Object::Boolean(value) => Some(value.to_string()),
        _ => None,
    }
}

fn info_value(document: &Document, key: &[u8]) -> Option<String> {
    let info = document.trailer.get(b"Info").ok()?;
    let (_, info) = document.dereference(info).ok()?;
    let dictionary = info.as_dict().ok()?;
    object_text(document, dictionary.get(key).ok()?)
}

fn catalog_value(document: &Document, key: &[u8]) -> Option<String> {
    let root = document.trailer.get(b"Root").ok()?;
    let (_, root) = document.dereference(root).ok()?;
    let dictionary = root.as_dict().ok()?;
    object_text(document, dictionary.get(key).ok()?)
}

fn gate(id: &str, passed: bool, message: String) -> VerificationGate {
    VerificationGate::mandatory(
        id,
        if passed {
            VerificationGateStatus::Passed
        } else {
            VerificationGateStatus::Failed
        },
        message,
    )
}

pub fn verify_structural_invariants(
    original_path: &Path,
    edited_path: &Path,
) -> Result<Vec<VerificationGate>, String> {
    let original = Document::load(original_path)
        .map_err(|error| format!("cannot load original PDF structure: {error}"))?;
    let edited = Document::load(edited_path)
        .map_err(|error| format!("cannot load edited PDF structure: {error}"))?;
    let original_pages = page_signatures(&original)?;
    let edited_pages = page_signatures(&edited)?;
    let page_counts_match = original_pages.len() == edited_pages.len();
    let mut gates = vec![gate(
        "structure.page_count",
        page_counts_match,
        format!(
            "original pages={}, edited pages={}",
            original_pages.len(),
            edited_pages.len()
        ),
    )];

    let comparable = original_pages.len().min(edited_pages.len());
    let geometry_failures: Vec<usize> = (0..comparable)
        .filter(|index| {
            let source = &original_pages[*index];
            let candidate = &edited_pages[*index];
            !boxes_equal(source.media_box, candidate.media_box)
                || !boxes_equal(source.crop_box, candidate.crop_box)
                || source.rotation != candidate.rotation
        })
        .map(|index| index + 1)
        .collect();
    gates.push(gate(
        "structure.page_geometry",
        page_counts_match && geometry_failures.is_empty(),
        if geometry_failures.is_empty() {
            "MediaBox, CropBox, and rotation match on every page".into()
        } else {
            format!("page geometry differs on pages {geometry_failures:?}")
        },
    ));

    let presence_failures: Vec<usize> = (0..comparable)
        .filter(|index| {
            let source = &original_pages[*index];
            let candidate = &edited_pages[*index];
            (source.content_nonempty && !candidate.content_nonempty)
                || (source.text_nonempty && !candidate.text_nonempty)
        })
        .map(|index| index + 1)
        .collect();
    gates.push(gate(
        "structure.content_presence",
        page_counts_match && presence_failures.is_empty(),
        if presence_failures.is_empty() {
            "no source page became empty or textless".into()
        } else {
            format!("content is missing on pages {presence_failures:?}")
        },
    ));

    let mut identity_failures = Vec::new();
    let mut worst_anchor_recall = 1.0_f64;
    for index in 0..comparable {
        let source = &original_pages[index].text_anchors;
        if source.len() < 3 {
            continue;
        }
        let candidate = &edited_pages[index].text_anchors;
        let retained = source.intersection(candidate).count();
        let recall = retained as f64 / source.len() as f64;
        worst_anchor_recall = worst_anchor_recall.min(recall);
        if recall < 0.60 {
            identity_failures.push(index + 1);
        }
    }
    gates.push(gate(
        "structure.page_identity",
        page_counts_match && identity_failures.is_empty(),
        if identity_failures.is_empty() {
            format!("per-page stable-text anchor recall >= {worst_anchor_recall:.3}")
        } else {
            format!(
                "page identity/order anchor recall fell below 0.60 on pages {identity_failures:?}"
            )
        },
    ));

    let font_failures: Vec<usize> = (0..comparable)
        .filter(|index| {
            let source = &original_pages[*index].fonts;
            let candidate = &edited_pages[*index].fonts;
            !source.is_subset(candidate)
        })
        .map(|index| index + 1)
        .collect();
    gates.push(gate(
        "structure.font_resources",
        page_counts_match && font_failures.is_empty(),
        if font_failures.is_empty() {
            "every source page font family remains available".into()
        } else {
            format!("source font resources are missing on pages {font_failures:?}")
        },
    ));

    let mut metadata_mismatches = Vec::new();
    for key in [b"Title".as_slice(), b"Author", b"Subject", b"Keywords"] {
        if info_value(&original, key) != info_value(&edited, key) {
            metadata_mismatches.push(format!("Info.{}", String::from_utf8_lossy(key)));
        }
    }
    for key in [b"Lang".as_slice(), b"PageMode", b"PageLayout"] {
        if catalog_value(&original, key) != catalog_value(&edited, key) {
            metadata_mismatches.push(format!("Catalog.{}", String::from_utf8_lossy(key)));
        }
    }
    gates.push(gate(
        "structure.metadata_policy",
        metadata_mismatches.is_empty(),
        if metadata_mismatches.is_empty() {
            "stable Info and catalog metadata match policy".into()
        } else {
            format!(
                "stable metadata differs: {}",
                metadata_mismatches.join(", ")
            )
        },
    ));

    Ok(gates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_prefix_is_normalized() {
        assert_eq!(normalize_font_name(b"ABCDEF+Helvetica"), "Helvetica");
        assert_eq!(normalize_font_name(b"Helvetica"), "Helvetica");
    }

    #[test]
    fn text_anchor_selection_ignores_numeric_mutations() {
        let anchors = text_anchors("01/02/2026 Coffee Shop 123.45 900.00");
        assert_eq!(anchors, BTreeSet::from(["coffee".into(), "shop".into()]));
    }
}
