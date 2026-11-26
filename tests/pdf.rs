//! Builds a synthetic PDF in memory and runs the full extract + parse path.

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};

const LINES: [&str; 4] = [
    "STATEMENT PERIOD 01/06/2026 - 30/06/2026",
    "BALANCE BROUGHT FORWARD 2,000.00",
    "01/06/26 09:00 X1 ENET 200.00 1,800.00 Test Shop",
    "02/06/26 10:00 X2 ENET 300.00 2,100.00 Test Salary",
];

fn synthetic_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut operations = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 10.into()]),
        Operation::new("TL", vec![14.into()]),
        Operation::new("Td", vec![40.into(), 780.into()]),
    ];
    for line in LINES {
        operations.push(Operation::new("Tj", vec![Object::string_literal(line)]));
        operations.push(Operation::new("T*", vec![]));
    }
    operations.push(Operation::new("ET", vec![]));
    let content = Content { operations };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
            "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn synthetic_pdf_extracts_and_balances() {
    let statement =
        finc_statement_parsers::parse_pdf(&synthetic_pdf(), "", Some("scb_savings"), "").unwrap();
    assert_eq!(
        statement.balance_check,
        finc_statement_parsers::BalanceCheck::Passed
    );
    assert_eq!(statement.rows.len(), 2);
    assert_eq!(statement.closing_balance.unwrap().to_string(), "2100.00");
}

#[test]
fn garbage_bytes_are_a_pdf_error_not_a_panic() {
    assert!(finc_statement_parsers::parse_pdf(b"%PDF-1.4 not really", "", None, "").is_err());
}
