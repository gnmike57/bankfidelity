import re

with open("src/app/config.rs", "r") as f:
    content = f.read()

# Remove from DocumentParserMode
content = re.sub(r'/// Mindee Financial Document API.*?\s+MindeeFinDoc,', '', content, flags=re.DOTALL)
content = re.sub(r'/// PyMuPDF built-in text extraction.*?\s+PyMuPdfBuiltin,', '', content, flags=re.DOTALL)
content = re.sub(r'Self::MindeeFinDoc => "Mindee \(Financial Doc\)",', '', content)
content = re.sub(r'Self::PyMuPdfBuiltin => "PyMuPDF \(Built-in\)",', '', content)

# Remove from AppConfig
content = re.sub(r'/// Optional: Mindee API Key\s+pub mindee_api_key: Option<String>,', '', content)
content = re.sub(r'/// Optional: Mindee Financial Document Model ID\s+pub mindee_model_id: Option<String>,', '', content)

# Remove from Default for AppConfig
content = re.sub(r'mindee_api_key: None,', '', content)
content = re.sub(r'mindee_model_id: None,', '', content)

# Remove from ApiAvailability
content = re.sub(r'pub mindee: bool,', '', content)
content = re.sub(r'mindee: true,', '', content)
content = re.sub(r'"mindee" => self\.mindee = false,', '', content)

content = re.sub(r'let mindee = self\.mindee_api_key\.is_some\(\) && self\.mindee_model_id\.is_some\(\);', '', content)
content = re.sub(r'mindee,', '', content, count=1) # inside detect_availability return struct

content = re.sub(r'if !self\.mindee \{\s+return Some\("MINDEE_API_KEY missing"\);\s+\}', '', content)

with open("src/app/config.rs", "w") as f:
    f.write(content)
