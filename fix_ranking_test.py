import re

with open("tests/ranking_test.rs", "r") as f:
    content = f.read()

content = re.sub(r'use dual_core_pdf_pipeline::ai::mindee::MindeeClient;\n', '', content)
content = re.sub(r'\s+let mindee = MindeeClient::from_app_config\(&cfg\);\n', '', content)
content = re.sub(r'\s+if let Ok\(client\) = &mindee \{\n\s+if let Ok\(stmt\) = client\.parse_statement\(path\)\.await \{\n\s+named_stmts\.push\(\("Mindee", stmt\)\);\n\s+\}\n\s+\}\n', '', content, flags=re.DOTALL)
content = re.sub(r'\s+"Mindee" => stats\.mindee_wins \+= 1,\n', '', content)
content = re.sub(r'\s+println!\("Mindee: \{\}", stats\.mindee_wins\);\n', '', content)

with open("tests/ranking_test.rs", "w") as f:
    f.write(content)
