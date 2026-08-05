use lopdf::Document;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <pdf_path>", args[0]);
        return;
    }
    let path = &args[1];
    let doc = Document::load(path).unwrap();
    let page_id = doc.page_iter().next().unwrap();
    let content_bytes = doc.get_page_content(page_id).unwrap();
    let content = lopdf::content::Content::decode(&content_bytes).unwrap();
    for op in content.operations {
        if op.operator == "Tj" {
            if let Some(lopdf::Object::String(bytes, _)) = op.operands.first() {
                let text = String::from_utf8_lossy(bytes);
                println!("Tj: '{text}'");
            }
        } else if op.operator == "TJ" {
            if let Some(lopdf::Object::Array(ref arr)) = op.operands.first() {
                let mut combined = String::new();
                for item in arr {
                    if let lopdf::Object::String(bytes, _) = item {
                        combined.push_str(&String::from_utf8_lossy(bytes));
                    }
                }
                println!("TJ: '{combined}'");
            }
        }
    }
}
