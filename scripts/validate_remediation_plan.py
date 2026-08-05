from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PLAN = ROOT / "docs" / "remediation" / "MASTER_PLAN.md"
FINDINGS = ROOT / "docs" / "remediation" / "FINDINGS.md"

errors: list[str] = []
plan = PLAN.read_text(encoding="utf-8")
findings_text = FINDINGS.read_text(encoding="utf-8")

finding_ids = set(
    re.findall(
        r"^\| ((?:COR|AUD|SEC|OPS|QA|UX|REL)-\d+) \| P[012] \|",
        findings_text,
        re.MULTILINE,
    )
)
if len(finding_ids) != 57:
    errors.append(f"expected 57 audited findings, found {len(finding_ids)}")
missing_findings = sorted(fid for fid in finding_ids if f"| {fid} |" not in plan)
if missing_findings:
    errors.append("missing finding mappings: " + ", ".join(missing_findings))

ticket_ids = re.findall(r"^\| ([A-Z]+-\d{3}) \|", plan, re.MULTILINE)
duplicates = sorted({ticket for ticket in ticket_ids if ticket_ids.count(ticket) > 1})
if duplicates:
    errors.append("duplicate ticket IDs: " + ", ".join(duplicates))
if len(ticket_ids) < 100:
    errors.append(f"expected at least 100 executable tickets, found {len(ticket_ids)}")

for gate in range(14):
    token = f"**Gate {gate:02d}" if gate < 10 else f"**Gate {gate}"
    if token not in plan:
        errors.append(f"missing gate {gate:02d}")

required_directives = [
    "Python is permanent and may not be eliminated",
    "mandatory Windows and macOS",
    "local LLM ships only if",
    "Critical data-integrity and active security blockers are never deferred",
    "Final non-blocking auditability, secrets, privacy",
    "Final complete rerun and production release",
]
for directive in required_directives:
    if directive not in plan:
        errors.append(f"missing directive: {directive}")

privacy_position = plan.find("Phase 12 — Final non-blocking auditability")
functional_position = plan.find("Phase 11 — Functional qualification")
final_position = plan.find("Phase 13 — Final complete rerun")
if not (0 <= functional_position < privacy_position < final_position):
    errors.append("privacy/final phase order is incorrect")

print("REMEDIATION PLAN VALIDATION")
print(f"findings={len(finding_ids)} mapped={len(finding_ids) - len(missing_findings)}")
print(f"tickets={len(ticket_ids)} unique={len(set(ticket_ids))}")
if errors:
    print("FAIL")
    for error in errors:
        print(f"- {error}")
    raise SystemExit(1)
print("PASS")
