//! Every fixture here is synthetic: invented names, all-zero account and card
//! numbers, and round amounts. Never paste text from a real statement.

use finc_statement_parsers::{BalanceCheck, ParseError, Statement, parse_lines, text_lines};

fn parse(key: &str, text: &str) -> Result<Statement, ParseError> {
    parse_with(key, text, "")
}

fn parse_with(key: &str, text: &str, card: &str) -> Result<Statement, ParseError> {
    parse_lines(Some(key), &text_lines(text).unwrap(), card)
}

/// Asserts a balanced parse and returns `(date, amount)` per row.
fn rows(key: &str, text: &str) -> Vec<(String, String)> {
    let statement = parse(key, text).unwrap_or_else(|e| panic!("{key}: {e}"));
    assert_eq!(statement.balance_check, BalanceCheck::Passed, "{key}");
    statement
        .rows
        .iter()
        .map(|row| (row.transaction_date.clone(), row.amount.to_string()))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(d, a)| ((*d).to_owned(), (*a).to_owned()))
        .collect()
}

pub const BBL: &str = "Statement Period 01/06/2026 - 30/06/2026
01/06/26 B/F 1,000.00
02/06/26 TRF FROM SOMCHAI TESTER 500.00 1,500.00
03/06/26 ATM WITHDRAWAL 200.00 1,300.00";

pub const SCB: &str = "STATEMENT PERIOD 01/06/2026 - 30/06/2026
BALANCE BROUGHT FORWARD 2,000.00
01/06/26 09:00 X1 ENET 200.00 1,800.00 Test Shop
02/06/26 10:00 X2 ENET 300.00 2,100.00 Test Salary";

const GSB: &str = "01/06/2569 - 30/06/2569
ยอดยกมา 1,000.00
05/06/2569 ฝากเงิน 500.00 1,500.00 0001 0000001
06/06/2569 ถอนเงิน 200.00 1,300.00
ยอดยกไป 1,300.00";

const KBANK_SAVINGS: &str = "01/06/2026 - 30/06/2026
01-06-26 Beginning Balance 1,000.00
02-06-26 08:00 Transfer Deposit 500.00 1,500.00 From SOMCHAI TESTER
03-06-26 09:30 Payment 250.00 1,250.00 Test Merchant";

const KKP: &str = "Period 01/06/2026 - 30/06/2026
01/06/2026 Balance Forward 1,000.00
02/06/2026 Transfer In 500.00 1,500.00
03/06/2026 Bill Payment 100.00 1,400.00";

const CARDX: &str = "CLOSING DATE (D/M/Y) MINIMUM PAYMENT PAYMENT DATE (D/M/Y)
10/06/26 4,000.00 1,000.00 30/06/26
PREVIOUS BALANCE 2,000.00
15/05 16/05 TEST ONLINE STORE 1,500.00
20/05 21/05 TEST HOTEL USD 50.00 1,500.00
01/06 01/06 PAYMENT - THANK YOU -1,000.00
TOTAL BALANCE 4,000.00";

const KBANK_CARD: &str = "STATEMENT DATE 25/06/2026
TEST CARD 0000 00XX XXXX 0000
PREVIOUS BALANCE 2,000.00
30/05/26 01/06/26 TEST CAFE 200.00
10/06/26 11/06/26 TEST AIRLINE JPY 10,000.00 2,300.00
15/06/26 15/06/26 PAYMENT THANK YOU -1,000.00
TOTAL BALANCE 3,500.00
OTHER CARD 0000 00XX XXXX 0001
PREVIOUS BALANCE 50.00
12/06/26 12/06/26 TEST BOOKS 100.00
TOTAL BALANCE 150.00";

const KTC: &str = "MR. SOMCHAI TESTER 0000-00XX-XXXX-0000
ยอดเรียกเก็บรอบที่แล้ว 600.00
01/06/26 02/06/26 TEST SHOP BANGKOK 300.00
05/06/26 05/06/26 TEST REFUND -100.00
ยอดที่ต้องชำาระ/ชำาระเกิน (-) 800.00";

const KRUNGSRI_CARD: &str = "(MR. SOMCHAI TESTER) 0000 00XX XXXX 0000
ยอดเรียกเก็บของรอบบัญชีก่อน 1,000.00
01/06/26 02/06/26 TEST APPLIANCE 9,000.00 3/10 1,000.00
03/06/26 04/06/26 TEST MART 500.00
Total Payment Due For Credit Card 2,500.00";

const KRUNGSRI_SAVINGS: &str = "ชื่อบัญชี SOMCHAI TESTER
01/06/2026 - 30/06/2026
01/06/2026 08:00:00 ฝากเงินสด 1,000.00 1,500.00 ATM
ผู้โอน : TEST SENDER
02/06/2026 09:00:00 โอนเงินพร้อมเพย์ 200.00 1,300.00 MOBILE";

const KTB: &str = "Account Name SOMCHAI TESTER Account Type SAVINGS
Account Number 000-0-00000-0
ยอดยกมา 1,000.00
02/06/26 TRANSFER IN 500.00 1,500.00
03/06/26 WITHDRAW 300.00 1,200.00 0001";

const TTB: &str = "000-0-00000-0
15 มิ.ย. 69 12:00 TEST PAYMENT -200.00 1,400.00 MERCHANT
10 มิ.ย. 69 10:00 รับเงินโอน +600.00 1,600.00 SOMCHAI TESTER";

const UOB_CARD: &str = "STATEMENT DATE 25 JUN 2026
ACCOUNT NUMBER 0000-0000
PREVIOUS BALANCE 2,000.00
28 MAY 27 MAY TEST BOOKSHOP 400.00
05 JUN 03 JUN TEST HOTEL SINGAPORE SGD 100.00 2,600.00
10 JUN 10 JUN PAYMENT THANK YOU 1,000.00 CR
TOTAL BALANCE - UOB TEST 4,000.00";

const UOB_SAVINGS: &str = "Period: 01 Jun 2026 to 30 Jun 2026
000-000-000-0
01 Jun 01 Jun BALANCE B/F 1,000.00
02 Jun 02 Jun TRF DEP TEST 500.00 1,500.00
03 Jun 03 Jun TRF WD TEST 300.00 1,200.00
Total 300.00 500.00 1,200.00";

#[test]
fn savings_parsers_sign_rows_from_running_balances() {
    let up_down = |a: &str, b: &str| pairs(&[("2026-06-02", a), ("2026-06-03", b)]);
    assert_eq!(rows("bbl_savings", BBL), up_down("500.00", "-200.00"));
    assert_eq!(
        rows("scb_savings", SCB),
        pairs(&[("2026-06-01", "-200.00"), ("2026-06-02", "300.00")])
    );
    assert_eq!(
        rows("gsb_savings", GSB),
        pairs(&[("2026-06-05", "500.00"), ("2026-06-06", "-200.00")])
    );
    assert_eq!(
        rows("kbank_savings", KBANK_SAVINGS),
        up_down("500.00", "-250.00")
    );
    assert_eq!(rows("kkp_savings", KKP), up_down("500.00", "-100.00"));
    assert_eq!(rows("ktb_savings", KTB), up_down("500.00", "-300.00"));
    assert_eq!(
        rows("uob_savings", UOB_SAVINGS),
        up_down("500.00", "-300.00")
    );
    assert_eq!(
        rows("krungsri_savings", KRUNGSRI_SAVINGS),
        pairs(&[("2026-06-01", "1000.00"), ("2026-06-02", "-200.00")])
    );
    // TTB prints newest first; output is chronological.
    assert_eq!(
        rows("ttb_savings", TTB),
        pairs(&[("2026-06-10", "600.00"), ("2026-06-15", "-200.00")])
    );
}

#[test]
fn card_parsers_make_spend_negative_and_payments_positive() {
    assert_eq!(
        rows("cardx_card", CARDX),
        pairs(&[
            ("2026-05-15", "-1500.00"),
            ("2026-05-20", "-1500.00"),
            ("2026-06-01", "1000.00"),
        ])
    );
    assert_eq!(
        rows("ktc_card", KTC),
        pairs(&[("2026-06-01", "-300.00"), ("2026-06-05", "100.00")])
    );
    assert_eq!(
        rows("krungsri_card", KRUNGSRI_CARD),
        pairs(&[("2026-06-02", "-1000.00"), ("2026-06-03", "-500.00")])
    );
    assert_eq!(
        rows("uob_card", UOB_CARD),
        pairs(&[
            ("2026-05-27", "-400.00"),
            ("2026-06-03", "-2600.00"),
            ("2026-06-10", "1000.00"),
        ])
    );
    let first = parse_with("kbank_card", KBANK_CARD, "test card").unwrap();
    assert_eq!(first.balance_check, BalanceCheck::Passed);
    assert_eq!(first.rows.len(), 3);
    assert_eq!(first.opening_balance.unwrap().to_string(), "-2000.00");
    assert_eq!(first.closing_balance.unwrap().to_string(), "-3500.00");
    let other = parse_with("kbank_card", KBANK_CARD, "other").unwrap();
    assert_eq!(other.rows.len(), 1);
    assert_eq!(other.rows[0].amount.to_string(), "-100.00");
}

#[test]
fn declared_period_is_separate_from_observed_rows() {
    let scb = parse("scb_savings", SCB).unwrap();
    assert_eq!(scb.period_start.as_deref(), Some("2026-06-01"));
    assert_eq!(scb.period_end.as_deref(), Some("2026-06-30"));
    assert_eq!(scb.observed_start.as_deref(), Some("2026-06-01"));
    assert_eq!(scb.observed_end.as_deref(), Some("2026-06-02"));
    let gsb = parse("gsb_savings", GSB).unwrap();
    assert_eq!(gsb.period_end.as_deref(), Some("2026-06-30"), "BE year");
    let cardx = parse("cardx_card", CARDX).unwrap();
    assert_eq!(cardx.period_start.as_deref(), Some("2026-05-11"));
    assert_eq!(cardx.period_end.as_deref(), Some("2026-06-10"));
}

#[test]
fn foreign_amounts_use_the_currency_exponent() {
    let cardx = parse("cardx_card", CARDX).unwrap();
    assert_eq!(cardx.rows[1].foreign_currency.as_deref(), Some("USD"));
    assert_eq!(cardx.rows[1].foreign_amount.unwrap().to_string(), "50.00");
    let kbank = parse_with("kbank_card", KBANK_CARD, "test").unwrap();
    assert_eq!(kbank.rows[1].foreign_currency.as_deref(), Some("JPY"));
    assert_eq!(kbank.rows[1].foreign_amount.unwrap().to_string(), "10000");
    let uob = parse("uob_card", UOB_CARD).unwrap();
    assert_eq!(uob.rows[1].foreign_amount.unwrap().to_string(), "100.00");
}

#[test]
fn rows_that_do_not_reconcile_fail_the_parse() {
    let wrong_closing = SCB.replace("2,100.00 Test Salary", "2,150.00 Test Salary");
    // The running balance jumps by 350 for a 300 row: not a success.
    assert!(matches!(
        parse("scb_savings", &wrong_closing),
        Err(ParseError::Unbalanced)
    ));
    let wrong_total = KTC.replace("(-) 800.00", "(-) 850.00");
    assert!(matches!(
        parse("ktc_card", &wrong_total),
        Err(ParseError::Unbalanced)
    ));
}

#[test]
fn unreadable_amounts_fail_instead_of_being_skipped_or_substituted() {
    for thai in ["๒๐๐.๐๐", "300.๐๐", "๓00.00"] {
        let text = KTC.replace("300.00", thai);
        assert!(
            matches!(parse("ktc_card", &text), Err(ParseError::MalformedAmount)),
            "{thai}"
        );
    }
    for bad in ["+1,000.00", "1,000.000", "1,000.005"] {
        let text = CARDX.replace(
            "PREVIOUS BALANCE 2,000.00",
            &format!("PREVIOUS BALANCE {bad}"),
        );
        assert!(
            matches!(parse("cardx_card", &text), Err(ParseError::MalformedAmount)),
            "{bad}"
        );
    }
    // An in-cycle KBank row whose amount cannot be read fails the statement,
    // even when the readable rows would still reconcile.
    let text = KBANK_CARD.replace("TEST CAFE 200.00", "TEST CAFE 200.0O");
    assert!(matches!(
        parse_with("kbank_card", &text, "test card"),
        Err(ParseError::MalformedAmount)
    ));
}

#[test]
fn text_without_rows_is_not_a_success() {
    for key in finc_statement_parsers::PARSER_KEYS {
        let result = parse(key, "SOMCHAI TESTER\nnothing to see here");
        assert!(result.is_err(), "{key}");
    }
    assert!(matches!(
        parse("nope", SCB),
        Err(ParseError::UnknownParser(_))
    ));
}

#[test]
fn auto_detection_picks_the_balanced_parser() {
    let detect = |text: &str| {
        parse_lines(None, &text_lines(text).unwrap(), "")
            .map(|s| s.parser)
            .unwrap_or_default()
    };
    for (key, text) in [
        ("bbl_savings", BBL),
        ("scb_savings", SCB),
        ("gsb_savings", GSB),
        ("kbank_savings", KBANK_SAVINGS),
        ("kkp_savings", KKP),
        ("cardx_card", CARDX),
        ("ktc_card", KTC),
        ("krungsri_card", KRUNGSRI_CARD),
        ("krungsri_savings", KRUNGSRI_SAVINGS),
        ("ktb_savings", KTB),
        ("ttb_savings", TTB),
        ("uob_card", UOB_CARD),
        ("uob_savings", UOB_SAVINGS),
    ] {
        assert_eq!(detect(text), key);
    }
}

#[test]
fn json_amounts_are_exact_strings() {
    let json = serde_json::to_value(parse("scb_savings", SCB).unwrap()).unwrap();
    assert_eq!(json["format"], "finc-statement/v1");
    assert_eq!(json["opening_balance"], "2000.00");
    assert_eq!(json["rows"][0]["amount"], "-200.00");
    assert_eq!(json["rows"][0]["balance_after"], "1800.00");
    assert_eq!(json["balance_check"], "passed");
}
