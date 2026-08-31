import re
import sys

def process_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    # We need to find places where we call `doc_ai.parse_entire_statement`, `client.parse_statement`, etc.
    # Actually, it's easier to use a regex to find all `client.parse_statement(&path).await`
    # and replace them. But there are multiple clients: LlamaParse, Document AI, Gemini, PyMuPDF.

    # 1. LlamaParse
    content = re.sub(
        r'client\.parse_statement\(([^)]+)\)\.await',
        r'crate::engine::pro_edit::perform_pro_edit("LlamaParse", async { client.parse_statement(\1).await.map_err(|e| anyhow::anyhow!(e)) }, watchdog_clone.clone()).await',
        content
    )

    # 2. Document AI (doc_ai or client)
    content = re.sub(
        r'doc_ai\.parse_entire_statement\(([^)]+)\)\.await',
        r'crate::engine::pro_edit::perform_pro_edit("DocumentAI", async { doc_ai.parse_entire_statement(\1).await.map_err(|e| anyhow::anyhow!(e)) }, watchdog_clone.clone()).await',
        content
    )
    content = re.sub(
        r'client\.parse_entire_statement\(([^)]+)\)\.await',
        r'crate::engine::pro_edit::perform_pro_edit("DocumentAI", async { client.parse_entire_statement(\1).await.map_err(|e| anyhow::anyhow!(e)) }, watchdog_clone.clone()).await',
        content
    )
    
    # 3. Gemini backend validation/vision calls
    content = re.sub(
        r'gemini\.validate_statement\(([^)]+)\)\.await',
        r'crate::engine::pro_edit::perform_pro_edit("Gemini", async { gemini.validate_statement(\1).await.map_err(|e| anyhow::anyhow!(e)) }, watchdog_clone.clone()).await',
        content
    )

    with open(filepath, 'w') as f:
        f.write(content)

if __name__ == '__main__':
    process_file('src/app/runtime.rs')
