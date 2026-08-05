import re

with open("src/app/api_verification.rs", "r") as f:
    content = f.read()

# Remove verify_mindee call
content = re.sub(r'// 4\. Verify Mindee \(default parser\)\s*results\.push\(verify_mindee\(config\)\.await\);', '', content)

# Remove verify_mindee function
content = re.sub(r'async fn verify_mindee\(config: &AppConfig\) -> VerificationResult \{.*?\}\n', '', content, flags=re.DOTALL)

with open("src/app/api_verification.rs", "w") as f:
    f.write(content)
