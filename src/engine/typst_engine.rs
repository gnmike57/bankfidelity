use crate::ai::document_ai::BankStatement;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum TypstEngineError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Typst Compilation Error: {0}")]
    Typst(String),
}

#[derive(Default)]
pub struct TypstEngine;

impl TypstEngine {
    pub fn new() -> Self {
        Self
    }

    /// Reconstructs a bank statement as a brand new PDF using Typst in-process.
    pub async fn reconstruct_pdf(
        &self,
        statement: &BankStatement,
        output_path: &Path,
    ) -> Result<(), TypstEngineError> {
        tracing::info!("[typst_engine] Starting in-process PDF reconstruction");
        let markup = self.generate_markup(statement);

        // We use spawn_blocking because typst compilation is CPU intensive
        let out_path = output_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let world = ReconstructWorld::new(markup);

            match typst::compile(&world).output {
                Ok(document) => {
                    // Generate PDF
                    let pdf_bytes = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
                        .map_err(|e| {
                            TypstEngineError::Typst(format!("PDF generation failed: {:?}", e))
                        })?;

                    std::fs::write(&out_path, pdf_bytes).map_err(TypstEngineError::Io)?;
                    tracing::info!("[typst_engine] Successfully compiled PDF in-process");
                    Ok(())
                }
                Err(diags) => {
                    let errs = diags
                        .into_iter()
                        .map(|d| d.message.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    tracing::error!("[typst_engine] Typst compilation failed: {}", errs);
                    Err(TypstEngineError::Typst(errs))
                }
            }
        })
        .await
        .map_err(|e| TypstEngineError::Typst(format!("Panic in typst thread: {}", e)))?
    }

    fn generate_markup(&self, stmt: &BankStatement) -> String {
        let bank_name = stmt.bank_name.as_deref().unwrap_or("Generic");
        match bank_name {
            "Chase" => self.generate_chase_markup(stmt),
            "Bank of America" => self.generate_bofa_markup(stmt),
            _ => self.generate_generic_markup(stmt),
        }
    }

    fn generate_generic_markup(&self, stmt: &BankStatement) -> String {
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
        out.push_str(&format!(
            "  align(right)[*Opening Balance:* \\${}]\n",
            stmt.opening_balance
        ));
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
            let credit = tx
                .credit
                .map(|c| format!("\\${:.2}", c))
                .unwrap_or_default();
            let bal = tx
                .running_balance
                .map(|b| format!("\\${:.2}", b))
                .unwrap_or_default();

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
    }

    fn generate_chase_markup(&self, stmt: &BankStatement) -> String {
        let mut out = String::new();
        out.push_str("#set page(margin: 1in)\n");
        out.push_str("#set text(font: \"Inter\", size: 9pt)\n");
        out.push_str(
            "#set table(stroke: (x, y) => if y == 0 { (bottom: 1pt + black) } else { none })\n\n",
        );

        out.push_str("#align(center)[= CHASE]\n");
        out.push_str("#align(center)[== Checking Summary]\n\n");

        if let Some(ref acc) = stmt.account_number {
            out.push_str(&format!("*Account:* {}\n\n", acc));
        }

        out.push_str(&format!(
            "*Beginning Balance:* \\${}\n",
            stmt.opening_balance
        ));
        out.push_str(&format!(
            "*Ending Balance:* \\${}\n\n",
            stmt.closing_balance
        ));

        out.push_str("#table(\n");
        out.push_str("  columns: (1fr, 4fr, 1.5fr, 1.5fr, 1.5fr),\n");
        out.push_str("  align: (col, row) => if col > 1 { right } else { left },\n");
        out.push_str("  [*Date*], [*Description*], [*Amount*], [*Credit*], [*Balance*],\n");

        for tx in &stmt.transactions {
            let date = tx.date.replace("[", "\\[").replace("]", "\\]");
            let desc = tx.raw_text.replace("[", "\\[").replace("]", "\\]");
            let debit = tx.debit.map(|d| format!("\\${:.2}", d)).unwrap_or_default();
            let credit = tx
                .credit
                .map(|c| format!("\\${:.2}", c))
                .unwrap_or_default();
            let bal = tx
                .running_balance
                .map(|b| format!("\\${:.2}", b))
                .unwrap_or_default();

            out.push_str(&format!(
                "  [{}], [{}], [{}], [{}], [{}],\n",
                date,
                desc,
                credit,
                debit,
                bal // Chase usually has amounts out vs amounts in
            ));
        }
        out.push_str(")\n\n");

        out
    }

    fn generate_bofa_markup(&self, stmt: &BankStatement) -> String {
        let mut out = String::new();
        out.push_str("#set page(margin: 0.8in)\n");
        out.push_str("#set text(font: \"Inter\", size: 10pt)\n");
        out.push_str("#set table(stroke: 0.2pt + luma(100))\n\n");

        out.push_str("#text(size: 14pt, fill: rgb(\"#E31837\"))[*Bank of America*]\n\n");

        if let Some(ref acc) = stmt.account_number {
            out.push_str(&format!("*Account Number:* {}\n\n", acc));
        }

        out.push_str("#box(fill: luma(240), inset: 8pt)[\n");
        out.push_str("  *Your Account at a Glance*\n");
        out.push_str(&format!(
            "  - Beginning Balance: \\${}\n",
            stmt.opening_balance
        ));
        out.push_str(&format!(
            "  - Ending Balance: \\${}\n",
            stmt.closing_balance
        ));
        out.push_str("]\n\n");

        out.push_str("== Transactions\n\n");

        out.push_str("#table(\n");
        out.push_str("  columns: (1fr, 3fr, 1fr, 1fr, 1fr),\n");
        out.push_str("  fill: (col, row) => if row == 0 { rgb(\"#E31837\") } else { none },\n");
        out.push_str("  align: (col, row) => if col > 1 { right } else { left },\n");
        out.push_str("  text(fill: white)[*Date*], text(fill: white)[*Description*], text(fill: white)[*Deposits*], text(fill: white)[*Withdrawals*], text(fill: white)[*Balance*],\n");

        for tx in &stmt.transactions {
            let date = tx.date.replace("[", "\\[").replace("]", "\\]");
            let desc = tx.raw_text.replace("[", "\\[").replace("]", "\\]");
            let debit = tx.debit.map(|d| format!("\\${:.2}", d)).unwrap_or_default();
            let credit = tx
                .credit
                .map(|c| format!("\\${:.2}", c))
                .unwrap_or_default();
            let bal = tx
                .running_balance
                .map(|b| format!("\\${:.2}", b))
                .unwrap_or_default();

            out.push_str(&format!(
                "  [{}], [{}], [{}], [{}], [{}],\n",
                date, desc, debit, credit, bal
            ));
        }
        out.push_str(")\n\n");

        out
    }
}

// Minimal Typst World for in-process compilation
use typst::diag::{FileError, FileResult};
use typst::foundations::Datetime;
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::World;

struct ReconstructWorld {
    library: typst::utils::LazyHash<typst::Library>,
    book: typst::utils::LazyHash<FontBook>,
    fonts: Vec<Font>,
    source: Source,
}

impl ReconstructWorld {
    fn new(source_text: String) -> Self {
        let font_data = include_bytes!("../../assets/Inter-Regular.ttf");
        let font = Font::new(typst::foundations::Bytes::new(font_data.to_vec()), 0)
            .expect("Failed to parse Inter-Regular");

        let font_bold_data = include_bytes!("../../assets/Inter-Bold.ttf");
        let font_bold = Font::new(typst::foundations::Bytes::new(font_bold_data.to_vec()), 0)
            .expect("Failed to parse Inter-Bold");

        let fonts = vec![font, font_bold];
        let book = typst::utils::LazyHash::new(FontBook::from_fonts(&fonts));
        use typst::LibraryExt;
        let library = typst::utils::LazyHash::new(typst::Library::builder().build());
        let source = Source::new(FileId::new(None, VirtualPath::new("main.typ")), source_text);

        Self {
            library,
            book,
            fonts,
            source,
        }
    }
}

impl World for ReconstructWorld {
    fn library(&self) -> &typst::utils::LazyHash<typst::Library> {
        &self.library
    }
    fn book(&self) -> &typst::utils::LazyHash<FontBook> {
        &self.book
    }
    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_path_buf(),
            ))
        }
    }

    fn file(&self, id: FileId) -> FileResult<typst::foundations::Bytes> {
        Err(FileError::NotFound(
            id.vpath().as_rootless_path().to_path_buf(),
        ))
    }

    fn font(&self, id: usize) -> Option<Font> {
        self.fonts.get(id).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        None
    }
}
