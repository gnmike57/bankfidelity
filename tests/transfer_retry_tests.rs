use dual_core_pdf_pipeline::engine::model::{FieldBboxes, Provenance, Transaction};
use dual_core_pdf_pipeline::engine::transfer::{
    plan_transaction_transfer_deterministic, transaction_description,
};
use rust_decimal_macros::dec;

#[test]
fn provider_free_transfer_plans_exact_mapping() {
    let source = Transaction {
        page: 0,
        line_on_page: 0,
        date: "25/12/2023".into(),
        raw_text: "25/12/2023 COFFEE 10.00 90.00".into(),
        debit: None,
        credit: Some(dec!(10.00)),
        running_balance: Some(dec!(90.00)),
        bbox: Some([10.0, 20.0, 500.0, 35.0]),
        field_bboxes: FieldBboxes::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };
    let target = Transaction {
        page: 1,
        line_on_page: 4,
        date: "12/25/2023".into(),
        raw_text: "TARGET ROW".into(),
        debit: None,
        credit: Some(dec!(1.00)),
        running_balance: Some(dec!(99.00)),
        bbox: Some([10.0, 40.0, 500.0, 55.0]),
        field_bboxes: FieldBboxes {
            date: Some([10.0, 40.0, 70.0, 55.0]),
            description: Some([80.0, 40.0, 250.0, 55.0]),
            debit: Some([260.0, 40.0, 320.0, 55.0]),
            credit: Some([330.0, 40.0, 390.0, 55.0]),
            running_balance: Some([400.0, 40.0, 490.0, 55.0]),
        },
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };

    let plan = plan_transaction_transfer_deterministic(&[source], &[target], 2)
        .expect("provider-free exact-capacity mapping should succeed");
    assert_eq!(plan.strategy, "deterministic-local-exact-geometry-capacity");
    assert_eq!(plan.confidence, 1.0);
    assert_eq!(plan.mappings.len(), 1);
    assert_eq!(plan.mappings[0].target_page, 1);
    assert_eq!(plan.mappings[0].target_line, 0);
    assert_eq!(plan.mappings[0].converted_date, "12/25/2023");
    assert_eq!(plan.mappings[0].adapted_description, "COFFEE");
}

#[test]
fn transaction_description_strips_multi_token_date_prefix() {
    let transaction = Transaction {
        page: 0,
        line_on_page: 0,
        date: "19 Dec".into(),
        raw_text: "19 Dec Settlement Fee 200.00 20,124.91 CR".into(),
        debit: None,
        credit: Some(dec!(200.00)),
        running_balance: Some(dec!(20124.91)),
        bbox: None,
        field_bboxes: FieldBboxes::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };
    assert_eq!(
        transaction_description(&transaction).unwrap(),
        "Settlement Fee"
    );
}

#[test]
fn transaction_description_strips_dotted_table_leaders() {
    let transaction = Transaction {
        page: 1,
        line_on_page: 29,
        date: "24 Sep".into(),
        raw_text: "24 Sep 2022 Anthony McIver Some assistance....................................................................... ...................... 93.00 1,234.56"
            .into(),
        debit: None,
        credit: Some(dec!(93.00)),
        running_balance: Some(dec!(1234.56)),
        bbox: None,
        field_bboxes: FieldBboxes::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };
    assert_eq!(
        transaction_description(&transaction).unwrap(),
        "Anthony McIver Some assistance"
    );
}

#[test]
fn transaction_description_strips_year_after_yearless_nab_date() {
    let transaction = Transaction {
        page: 0,
        line_on_page: 0,
        date: "8 Jul".into(),
        raw_text: "8 Jul 2022 AA1M7692502345501T Jobseeker Pymt 146.30 0.00".into(),
        debit: None,
        credit: Some(dec!(146.30)),
        running_balance: Some(dec!(0.00)),
        bbox: None,
        field_bboxes: FieldBboxes::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };
    assert_eq!(
        transaction_description(&transaction).unwrap(),
        "AA1M7692502345501T Jobseeker Pymt"
    );
}

#[test]
fn transaction_description_normalizes_pdf_presentation_ligatures() {
    let transaction = Transaction {
        page: 0,
        line_on_page: 20,
        date: "14 Aug".into(),
        raw_text: "14 Aug 2022 Land Titles Of\u{fb01}ce 25.00 100.00".into(),
        debit: None,
        credit: Some(dec!(25.00)),
        running_balance: Some(dec!(100.00)),
        bbox: None,
        field_bboxes: FieldBboxes::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };
    assert_eq!(
        transaction_description(&transaction).unwrap(),
        "Land Titles Office"
    );
}

#[test]
fn transaction_description_preserves_merchant_currency_suffix() {
    let transaction = Transaction {
        page: 5,
        line_on_page: 8,
        date: "13/09/23".into(),
        raw_text: "13/09/23 Debit Card Purchase Airwallet London Gbr Eur 30.00 400.00".into(),
        debit: Some(dec!(30.00)),
        credit: None,
        running_balance: Some(dec!(400.00)),
        bbox: None,
        field_bboxes: FieldBboxes::default(),
        provenance: Provenance::Computed,
        category: None,
        canonical: Default::default(),
    };
    assert_eq!(
        transaction_description(&transaction).unwrap(),
        "Debit Card Purchase Airwallet London Gbr Eur"
    );
}
