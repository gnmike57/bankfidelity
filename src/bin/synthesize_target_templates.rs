//! CLI utility to synthesize pristine reference target PDFs from YAML definitions via Typst.

use dual_core_pdf_pipeline::ai::document_ai::BankStatement;
use dual_core_pdf_pipeline::engine::model::{Provenance, Transaction};
use dual_core_pdf_pipeline::engine::typst_engine::TypstEngine;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::fs;
use std::path::PathBuf;

fn make_tx(
    page: usize,
    line: usize,
    date: &str,
    desc: &str,
    debit: Option<Decimal>,
    credit: Option<Decimal>,
    bal: Decimal,
) -> Transaction {
    Transaction {
        page,
        line_on_page: line,
        date: date.to_string(),
        raw_text: desc.to_string(),
        debit,
        credit,
        running_balance: Some(bal),
        bbox: None,
        field_bboxes: Default::default(),
        provenance: Provenance::Manual,
        category: None,
        canonical: Default::default(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from("bank_templates/rendered");
    fs::create_dir_all(&out_dir)?;

    let engine = TypstEngine::new();

    let targets = vec![
        (
            "anz_plus_au",
            "ANZ Plus",
            "012-345 67890123",
            dec!(5420.50),
            dec!(6120.50),
            vec![
                make_tx(
                    1,
                    1,
                    "01/08/2026",
                    "Direct Credit - Payroll",
                    Some(dec!(1500.00)),
                    None,
                    dec!(6920.50),
                ),
                make_tx(
                    1,
                    2,
                    "03/08/2026",
                    "Coles Supermarkets",
                    None,
                    Some(dec!(125.40)),
                    dec!(6795.10),
                ),
                make_tx(
                    1,
                    3,
                    "05/08/2026",
                    "Woolworths Petrol",
                    None,
                    Some(dec!(85.00)),
                    dec!(6710.10),
                ),
                make_tx(
                    1,
                    4,
                    "10/08/2026",
                    "Transfer to Savings",
                    None,
                    Some(dec!(589.60)),
                    dec!(6120.50),
                ),
            ],
        ),
        (
            "bankwest_example",
            "Bankwest",
            "302-111 9876543",
            dec!(12000.00),
            dec!(11450.25),
            vec![
                make_tx(
                    1,
                    1,
                    "02/08/2026",
                    "Office Supplies Express",
                    None,
                    Some(dec!(245.50)),
                    dec!(11754.50),
                ),
                make_tx(
                    1,
                    2,
                    "04/08/2026",
                    "Client Payment - Invoice 104",
                    Some(dec!(850.00)),
                    None,
                    dec!(12604.50),
                ),
                make_tx(
                    1,
                    3,
                    "08/08/2026",
                    "ATO Business Activity Statement",
                    None,
                    Some(dec!(1154.25)),
                    dec!(11450.25),
                ),
            ],
        ),
        (
            "commbank_smartaccess_example",
            "Commonwealth Bank",
            "062-000 12345678",
            dec!(3250.00),
            dec!(3980.50),
            vec![
                make_tx(
                    1,
                    1,
                    "01 Aug 2026",
                    "SALARY PAYMENT ACME CORP",
                    Some(dec!(2100.00)),
                    None,
                    dec!(5350.00),
                ),
                make_tx(
                    1,
                    2,
                    "04 Aug 2026",
                    "NETFLIX AUSTRALIA SYDNEY",
                    None,
                    Some(dec!(22.99)),
                    dec!(5327.01),
                ),
                make_tx(
                    1,
                    3,
                    "09 Aug 2026",
                    "SYDNEY WATER UTILITIES",
                    None,
                    Some(dec!(346.51)),
                    dec!(4980.50),
                ),
                make_tx(
                    1,
                    4,
                    "12 Aug 2026",
                    "TRANSFER TO NETBANK SAVER",
                    None,
                    Some(dec!(1000.00)),
                    dec!(3980.50),
                ),
            ],
        ),
        (
            "ing_orange_au",
            "ING Orange Everyday",
            "923-100 55443322",
            dec!(450.00),
            dec!(1820.00),
            vec![
                make_tx(
                    1,
                    1,
                    "01/08/2026",
                    "Pay Anyone Transfer Received",
                    Some(dec!(2000.00)),
                    None,
                    dec!(2450.00),
                ),
                make_tx(
                    1,
                    2,
                    "03/08/2026",
                    "Bunnings Warehouse",
                    None,
                    Some(dec!(340.00)),
                    dec!(2110.00),
                ),
                make_tx(
                    1,
                    3,
                    "06/08/2026",
                    "JB Hi-Fi Electrical",
                    None,
                    Some(dec!(290.00)),
                    dec!(1820.00),
                ),
            ],
        ),
        (
            "macquarie_au",
            "Macquarie Bank",
            "182-500 88776655",
            dec!(15400.00),
            dec!(16250.00),
            vec![
                make_tx(
                    1,
                    1,
                    "02/08/2026",
                    "Dividend Reinvestment Macquarie",
                    Some(dec!(1250.00)),
                    None,
                    dec!(16650.00),
                ),
                make_tx(
                    1,
                    2,
                    "05/08/2026",
                    "Management Fee - Monthly",
                    None,
                    Some(dec!(400.00)),
                    dec!(16250.00),
                ),
            ],
        ),
        (
            "westpac_choice_basic_au",
            "Westpac Choice",
            "032-001 44556677",
            dec!(2890.00),
            dec!(3450.00),
            vec![
                make_tx(
                    1,
                    1,
                    "01/08/2026",
                    "DIRECT CREDIT SALARY",
                    Some(dec!(1800.00)),
                    None,
                    dec!(4690.00),
                ),
                make_tx(
                    1,
                    2,
                    "04/08/2026",
                    "TELSTRA TELECOM BILL",
                    None,
                    Some(dec!(140.00)),
                    dec!(4550.00),
                ),
                make_tx(
                    1,
                    3,
                    "08/08/2026",
                    "MORTGAGE OFFSET TRANSFER",
                    None,
                    Some(dec!(1100.00)),
                    dec!(3450.00),
                ),
            ],
        ),
    ];

    for (bank_id, bank_name, acc_num, open_bal, close_bal, txs) in targets {
        let stmt = BankStatement {
            total_pages: 1,
            transactions: txs,
            opening_balance: open_bal,
            closing_balance: close_bal,
            account_number: Some(acc_num.to_string()),
            bank_name: Some(bank_name.to_string()),
        };

        let out_pdf = out_dir.join(format!("{}.pdf", bank_id));
        println!(
            "[synthesis] Synthesizing pristine template: {}",
            out_pdf.display()
        );
        engine.reconstruct_pdf(&stmt, &out_pdf).await?;
        println!("[synthesis] Successfully created {}", out_pdf.display());
    }

    println!(
        "[synthesis] All pristine target templates synthesized successfully in {}",
        out_dir.display()
    );
    Ok(())
}
