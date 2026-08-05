# Beta Testing & Bug Reporting Guide

Welcome to the Beta Testing phase of the Bank Statement Fidelity Editor!

## Automated Telemetry & Bug Reports

To ensure a smooth transition to v1.0.0, the application now includes an integrated automated bug reporting tool and a repair loop system to capture crash data and hard failures natively.

### 1. Manual Bug Reporting
If you notice UI issues, logic bugs, or mathematical discrepancies that the system does not catch:
1. Click the **"🐛 Report Bug"** button located on the right side of the top navigation bar.
2. Fill out a description with steps to reproduce the issue.
3. Keep the "Attach recent application logs" and "Attach recent audit trail" boxes checked to send diagnostic files directly to the developers.
4. Click **"🚀 Submit to Developer"**.

### 2. The Interactive Repair Loop
If the background pipeline crashes (for example, due to malformed PDFs or API connection failures), an "Operation Failed" modal will immediately capture the screen.
Instead of silently crashing, you will be offered an automated repair loop where you can choose to:
- **Retry** the operation
- **Synthesize** a fallback solution
- **Submit Bug Report** (This will automatically pre-fill the exact Rust trace/error string in your feedback modal!)

## Where does this data go?
All bug reports and attached logs are packaged securely as JSON and dispatched via HTTP POST to the configured `WEBHOOK_URL` in your `.env` file. By default, this sends the tail end of your `app.log` straight to the development team's Discord/Slack channel for rapid iteration.
