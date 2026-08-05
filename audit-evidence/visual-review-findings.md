# CommBank X-Ray Visual Review Findings

The first-page and cloned middle-page contact sheets were inspected at matched rendering settings. In both samples, the **header, account-information block, table rules, footer, margins, and page chrome remain visually aligned** between the target template and transferred candidate.

The red/cyan overlay shows changes confined to transaction dates, descriptions, amounts, and balances. The canary’s reported blue outside-mask changes are therefore a **mask-construction false positive**: the permitted mask covered only fragments of detected rows, while full transaction descriptions and running-balance columns extended outside those rectangles. The verifier must mask the complete transaction-table envelope while retaining independent top/header and bottom/footer comparison.

The original CommBank first page already contains a nearly full-page image resource, so full-page-raster detection must compare **candidate versus source-template parity** rather than reject an image resource that pre-exists in the authorized source.

The transfer log explicitly records `Math verification PASSED`, `Optional math review ✓`, `Complete ... math: ✓, visual: ✓`, and `Transfer complete ✓`; provider-math recognition must include the application’s exact optional-review phrase.

ANZ page 23 was also inspected after explicit worst-page sampling. Its header, statement-period label, table columns, footer, and page number remain aligned; the residual blue diff consisted of the final transaction rows immediately below the last date-derived mask boundary. The permitted table envelope therefore needs a larger bottom continuation margin while still stopping above the footer.
