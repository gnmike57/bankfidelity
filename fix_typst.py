import re

with open('src/engine/typst_engine.rs', 'r') as f:
    content = f.read()

old_func = """    fn generate_markup(&self, stmt: &BankStatement) -> String {
        let mut out = String::new();
        out.push_str("#set page(margin: 1in)\\n");
        // We embed Inter, so let's use it
        out.push_str("#set text(font: \\"Inter\\", size: 10pt)\\n\\n");
        out.push_str("= Bank Statement\\n\\n");

        if let Some(ref acc) = stmt.account_number {
            out.push_str(&format!("*Account Number:* {}\\n\\n", acc));
        }

        out.push_str(&format!(
            "*Opening Balance:* \\\\${}\\n\\n",
            stmt.opening_balance
        ));

        out.push_str("#table(\\n");
        out.push_str("  columns: (1fr, 3fr, 1fr, 1fr),\\n");
        out.push_str("  [**Date**], [**Description**], [**Debit**], [**Credit**],\\n");

        for tx in &stmt.transactions {
            let date = tx.date.clone();
            let desc = tx.raw_text.replace("[", "\\\\[").replace("]", "\\\\]");
            let debit = tx.debit.map(|d| format!("\\\\${}", d)).unwrap_or_default();
            let credit = tx.credit.map(|c| format!("\\\\${}", c)).unwrap_or_default();

            out.push_str(&format!(
                "  [{}], [{}], [{}], [{}],\\n",
                date, desc, debit, credit
            ));
        }
        out.push_str(")\\n\\n");

        out.push_str(&format!(
            "*Closing Balance:* \\\\${}\\n\\n",
            stmt.closing_balance
        ));

        out
    }"""

new_func = """    fn generate_markup(&self, stmt: &BankStatement) -> String {
        let mut out = String::new();
        out.push_str("#set page(margin: 1in)\n");
        out.push_str("#set text(font: \"Inter\", size: 10pt)\n");
        out.push_str("#set table(stroke: 0.5pt + luma(200))\n\n");
        out.push_str("= Bank Statement\n\n");

        out.push_str("#grid(columns: (1fr, 1fr),\n");
        if let Some(ref acc) = stmt.account_number {
            out.push_str(&format!("  [*Account Number:* {}],\n", acc));
        } else {
            out.push_str("  [],\n");
        }
        out.push_str(&format!("  align(right)[*Opening Balance:* \\${}]\n", stmt.opening_balance));
        out.push_str(")\n\n");

        out.push_str("#table(\n");
        out.push_str("  columns: (1fr, 3fr, 1fr, 1fr, 1fr),\n");
        out.push_str("  fill: (col, row) => if row == 0 { luma(240) } else { none },\n");
        out.push_str("  align: (col, row) => if col > 1 { right } else { left },\n");
        out.push_str("  [*Date*], [*Description*], [*Debit*], [*Credit*], [*Balance*],\n");

        for tx in &stmt.transactions {
            let date = tx.date.replace("[", "\\[").replace("]", "\\]");
            let desc = tx.raw_text.replace("[", "\\[").replace("]", "\\]");
            let debit = tx.debit.map(|d| format!("\\${:.2}", d)).unwrap_or_default();
            let credit = tx.credit.map(|c| format!("\\${:.2}", c)).unwrap_or_default();
            let bal = tx.running_balance.map(|b| format!("\\${:.2}", b)).unwrap_or_default();

            out.push_str(&format!(
                "  [{}], [{}], [{}], [{}], [{}],\n",
                date, desc, debit, credit, bal
            ));
        }
        out.push_str(")\n\n");

        out.push_str(&format!(
            "#align(right)[*Closing Balance:* \\${}]\n\n",
            stmt.closing_balance
        ));

        out
    }"""

content = content.replace(old_func, new_func)

with open('src/engine/typst_engine.rs', 'w') as f:
    f.write(content)
