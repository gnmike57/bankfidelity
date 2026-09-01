#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dual_core_pdf_pipeline::extractors::templates::BankTemplate;
use regex::Regex;
use std::fs;
use std::path::Path;

#[test]
fn test_all_bank_templates_validity() {
    let template_dir = Path::new("bank_templates");
    assert!(template_dir.exists(), "bank_templates directory must exist");

    let entries = fs::read_dir(template_dir).expect("Failed to read bank_templates directory");
    let mut count = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
            let template: BankTemplate = serde_yaml::from_str(&content)
                .unwrap_or_else(|e| panic!("Invalid YAML structure in {}: {}", path.display(), e));

            // Validate non-empty id
            assert!(
                !template.id.is_empty(),
                "Template ID cannot be empty in {}",
                path.display()
            );

            // Validate header signatures
            assert!(
                !template.header_signatures.is_empty(),
                "Template {} must have at least one header signature",
                template.id
            );

            // Validate amount regex is compilable
            assert!(
                Regex::new(&template.amount_regex).is_ok(),
                "Template {} amount regex '{}' failed to compile",
                template.id,
                template.amount_regex
            );

            // Validate column x-ranges
            for (col, range) in &template.column_x_ranges {
                assert!(
                    range[0] >= 0.0 && range[1] > range[0],
                    "Template {} column '{}' range {:?} is invalid (must be x0 >= 0 and x1 > x0)",
                    template.id,
                    col,
                    range
                );
            }

            count += 1;
        }
    }

    assert!(
        count >= 8,
        "Expected at least 8 valid bank templates, found {}",
        count
    );
    println!(
        "[template_validation] Successfully validated {} bank templates",
        count
    );
}
