//! Per-bank line parsers, ported from FINC v1.
//!
//! Amounts are exact integer minor units (satang for THB) while parsing; the
//! public API converts them to `rust_decimal::Decimal`. No float ever touches
//! an amount.

#![allow(clippy::too_many_lines)]

use std::sync::LazyLock;

use chrono::{Datelike, Duration as ChronoDuration, NaiveDate};
use regex::Regex;

use crate::ParseError;

#[derive(Clone, Debug)]
pub(crate) struct ParsedLine {
    pub txn_date: String,
    pub value_date: Option<String>,
    pub description: String,
    pub amount_satang: i64,
    pub balance_after_satang: Option<i64>,
    pub foreign_amount_satang: Option<i64>,
    pub foreign_currency: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedStatement {
    pub account_number: Option<String>,
    pub holder: Option<String>,
    pub opening_balance_satang: Option<i64>,
    pub closing_balance_satang: Option<i64>,
    /// Coverage declared by the statement itself.
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    /// First and last observed transaction dates. Never promoted to coverage.
    pub observed_start: Option<String>,
    pub observed_end: Option<String>,
    pub declared_empty: bool,
    pub verification_ok: Option<bool>,
    pub lines: Vec<ParsedLine>,
}

/// Every supported parser key, in auto-detection order.
pub const PARSER_KEYS: [&str; 14] = [
    "bbl_savings",
    "cardx_card",
    "gsb_savings",
    "kbank_card",
    "kbank_savings",
    "kkp_savings",
    "krungsri_card",
    "krungsri_savings",
    "ktb_savings",
    "ktc_card",
    "scb_savings",
    "ttb_savings",
    "uob_card",
    "uob_savings",
];

pub(crate) fn parse_lines(
    parser_key: &str,
    lines: &[String],
    card_selector: &str,
) -> Result<ParsedStatement, ParseError> {
    // Amount-shaped text written with Thai numerals on either side of the
    // decimal point (including mixed tokens like `200.๐๐`): the ASCII-only
    // row regexes would silently skip it, so flag it instead.
    static THAI_AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:[0-9๐-๙,]*[๐-๙][0-9๐-๙,]*\.[0-9๐-๙]+|[0-9,]+\.[0-9๐-๙]*[๐-๙][0-9๐-๙]*)")
            .expect("thai amount regex")
    });
    INVALID_AMOUNT.set(lines.iter().any(|line| THAI_AMOUNT.is_match(line)));
    let parsed = parse_with(parser_key, lines, card_selector);
    // One amount the parser cannot read exactly fails the whole statement: a
    // substitute value must never be emitted, and a partially read statement
    // cannot be balanced or trusted.
    if INVALID_AMOUNT.replace(false) {
        return Err(ParseError::MalformedAmount);
    }
    parsed
}

fn parse_with(
    parser_key: &str,
    lines: &[String],
    account_name: &str,
) -> Result<ParsedStatement, ParseError> {
    match parser_key {
        "bbl_savings" => Ok(parse_bbl(lines)),
        "cardx_card" => parse_cardx(lines),
        "gsb_savings" => Ok(parse_gsb(lines)),
        "kbank_card" => parse_kbank_card(lines, account_name),
        "kbank_savings" => Ok(parse_kbank_savings(lines)),
        "kkp_savings" => Ok(parse_kkp(lines)),
        "krungsri_card" => Ok(parse_krungsri_card(lines)),
        "krungsri_savings" => Ok(parse_krungsri_savings(lines)),
        "ktb_savings" => Ok(parse_ktb(lines)),
        "ktc_card" => Ok(parse_ktc(lines)),
        "scb_savings" => Ok(parse_scb(lines)),
        "ttb_savings" => Ok(parse_ttb(lines)),
        "uob_card" => Ok(parse_uob_card(lines)),
        "uob_savings" => Ok(parse_uob_savings(lines)),
        other => Err(ParseError::UnknownParser(other.to_owned())),
    }
}

pub(crate) fn normalize_space(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ISO 4217 minor-unit exponent for the foreign currencies the parsers accept.
pub(crate) fn currency_exponent(code: &str) -> u8 {
    match code {
        "JPY" | "KRW" | "VND" => 0,
        "BHD" | "KWD" => 3,
        _ => 2,
    }
}
/// Every parser regex is built here. `\d` / `\D` are rewritten to ASCII
/// classes: Rust's `\d` also matches Thai (and other) digits, which no
/// amount or date parser may read.
fn re(pattern: &str) -> Regex {
    let ascii = pattern.replace(r"\d", "[0-9]").replace(r"\D", "[^0-9]");
    Regex::new(&ascii).expect("static parser regex")
}

fn money(raw: &str) -> i64 {
    money_with_exponent(raw, 2)
}

/// Strict statement-balance reader: rejects leading `+` and fractional parts
/// other than 2 decimal places (e.g. `500.000` or `500.005`) so malformed
/// balances cannot be accepted via prefix matching.
fn balance_money(raw: &str) -> i64 {
    let trimmed = raw.trim();
    if trimmed
        .split_once('.')
        .is_some_and(|(_, frac)| frac.len() != 2)
    {
        INVALID_AMOUNT.set(true);
        return 0;
    }
    money(trimmed)
}

/// Largest amount a statement may carry, in minor units (1 trillion THB).
/// With at most `MAX_TEXT_LINES` rows, no statement sum can leave `i64`.
const MAX_AMOUNT_MINOR: i64 = 100_000_000_000_000;

thread_local! {
    /// Set by `money_with_exponent` when a parser reads an amount it cannot
    /// represent exactly; `parse_lines` turns it into `parse_failed`.
    // ponytail: a flag instead of threading Result through ~55 call sites in
    // every bank parser; parse_lines is the only entry point and is sync.
    static INVALID_AMOUNT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn money_with_exponent(raw: &str, exponent: u8) -> i64 {
    exact_minor_units(raw, exponent).unwrap_or_else(|| {
        INVALID_AMOUNT.set(true);
        0
    })
}

/// Exact minor units, or `None` for a malformed integer or fraction part,
/// precision beyond the currency's exponent, or an out-of-range amount.
fn exact_minor_units(raw: &str, exponent: u8) -> Option<i64> {
    // Validate the printed token before touching it: optional '-', plain
    // digits or 1-3 digits then ',ddd' groups, optional '.digits'. So
    // "1,2.34", ",.50" and Thai numerals are rejected, never re-read.
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^-?(?:[0-9]+|[0-9]{1,3}(?:,[0-9]{3})+)(?:\.[0-9]+)?$")
            .expect("amount token regex")
    });
    let token = raw.trim();
    if !TOKEN.is_match(token) {
        return None;
    }
    let clean = token.replace(',', "");
    let (negative, unsigned) = match clean.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, clean.as_str()),
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let width = usize::from(exponent);
    let (kept, dropped) = fraction.split_at(fraction.len().min(width));
    if dropped.bytes().any(|byte| byte != b'0') {
        return None;
    }
    let minor = format!("{kept:0<width$}");
    let value = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(10_i64.checked_pow(u32::from(exponent))?)?
        .checked_add(if minor.is_empty() {
            0
        } else {
            minor.parse::<i64>().ok()?
        })?;
    (value <= MAX_AMOUNT_MINOR).then_some(if negative { -value } else { value })
}

fn foreign_money(raw: &str, currency: &str) -> i64 {
    money_with_exponent(raw, currency_exponent(&currency.to_ascii_uppercase()))
}
fn iso_yy_slash(raw: &str) -> String {
    let p = raw.split('/').collect::<Vec<_>>();
    format!("20{}-{:0>2}-{:0>2}", p[2], p[1], p[0])
}

fn iso_yy_dash(raw: &str) -> String {
    let p = raw.split('-').collect::<Vec<_>>();
    format!("20{}-{:0>2}-{:0>2}", p[2], p[1], p[0])
}

fn iso_full_slash(raw: &str) -> String {
    let p = raw.split('/').collect::<Vec<_>>();
    let year = p[2].parse::<i32>().unwrap_or_default();
    let year = if year >= 2400 { year - 543 } else { year };
    format!("{year:04}-{:0>2}-{:0>2}", p[1], p[0])
}

fn declared_full_date_range(lines: &[String]) -> Option<(String, String)> {
    let range = re(r"(\d{1,2}/\d{1,2}/\d{4}).*?(\d{1,2}/\d{1,2}/\d{4})");
    lines.iter().find_map(|line| {
        let captures = range.captures(line)?;
        let start = NaiveDate::parse_from_str(&iso_full_slash(&captures[1]), "%Y-%m-%d").ok()?;
        let end = NaiveDate::parse_from_str(&iso_full_slash(&captures[2]), "%Y-%m-%d").ok()?;
        (start <= end && (end - start).num_days() <= 400)
            .then(|| (start.to_string(), end.to_string()))
    })
}

fn basic_line(
    date: String,
    value_date: Option<String>,
    description: String,
    amount: i64,
    balance: Option<i64>,
) -> ParsedLine {
    ParsedLine {
        txn_date: date,
        value_date,
        description,
        amount_satang: amount,
        balance_after_satang: balance,
        foreign_amount_satang: None,
        foreign_currency: None,
    }
}

fn statement(
    lines: Vec<ParsedLine>,
    opening: Option<i64>,
    closing: Option<i64>,
) -> ParsedStatement {
    let verification_ok = verify_balances(opening, closing, &lines);
    ParsedStatement {
        account_number: None,
        holder: None,
        opening_balance_satang: opening,
        closing_balance_satang: closing,
        period_start: None,
        period_end: None,
        // Earliest/latest date, not first/last printed: issuers may print a
        // row out of date order, and the range must stay ascending.
        observed_start: lines.iter().map(|line| line.txn_date.clone()).min(),
        observed_end: lines.iter().map(|line| line.txn_date.clone()).max(),
        declared_empty: false,
        verification_ok,
        lines,
    }
}

fn verify_balances(
    opening: Option<i64>,
    closing: Option<i64>,
    lines: &[ParsedLine],
) -> Option<bool> {
    if lines.is_empty() {
        return None;
    }
    // i128: verification can never overflow, whatever a parser produced.
    if let Some(opening) = opening
        && lines.iter().all(|line| line.balance_after_satang.is_some())
    {
        let mut running = i128::from(opening);
        for line in lines {
            let balance = i128::from(line.balance_after_satang.unwrap_or_default());
            if (running + i128::from(line.amount_satang) - balance).abs() > 1 {
                return Some(false);
            }
            running = balance;
        }
        return Some(closing.is_none_or(|value| (running - i128::from(value)).abs() <= 1));
    }
    opening.zip(closing).map(|(start, end)| {
        let total = lines
            .iter()
            .map(|line| i128::from(line.amount_satang))
            .sum::<i128>();
        (i128::from(start) + total - i128::from(end)).abs() <= 1
    })
}

fn running_sign(previous: i64, amount: i64, next: i64) -> i64 {
    if (i128::from(previous) + i128::from(amount) - i128::from(next)).abs() <= 1 {
        amount
    } else {
        -amount
    }
}

fn parse_bbl(lines: &[String]) -> ParsedStatement {
    let txn = re(r"^(\d{2}/\d{2}/\d{2})\s+(.+?)\s+([\d,]+\.\d{2})\s+([\d,]+\.\d{2})(?:\s+\S+)?$");
    let open = re(r"(?i)^\d{2}/\d{2}/\d{2}\s+B/F\s+([\d,]+\.\d{2})$");
    let acct = re(
        r"(?i)(?:เลขที่บัญชี(?:\s*/\s*Account\s*No\.?)?|Account\s*No\.?)\s*[:：]?\s*(\d[\d\- ]{7,20}\d)",
    );
    let holder =
        re(r"(?i)(?:ชื่อ\s*/\s*)?Name\s+(.+?)(?:\s+(?:เลขที่บัญชี|Account\s*(?:No|Number)\.?)|$)");
    let period = re(r"(?i)Statement\s+Period\s+(\d{2}/\d{2}/\d{4})\s*-\s*(\d{2}/\d{2}/\d{4})");
    let empty = re(r"(?i)Total\s+No\.\s+of\s+Debits\s+0\b.*Credits?\s+0\b");
    let (mut running, mut opening) = (None, None);
    let (mut account_number, mut holder_name, mut period_start, mut period_end) =
        (None, None, None, None);
    let mut out = Vec::new();
    let mut declared_empty = false;
    for line in lines {
        account_number.get_or_insert_with(|| {
            acct.captures(line)
                .map(|m| m[1].trim().to_owned())
                .unwrap_or_default()
        });
        if account_number.as_deref() == Some("") {
            account_number = None;
        }
        holder_name.get_or_insert_with(|| {
            holder
                .captures(line)
                .map(|m| m[1].trim().to_owned())
                .unwrap_or_default()
        });
        if holder_name.as_deref() == Some("") {
            holder_name = None;
        }
        if let Some(m) = period.captures(line) {
            period_start = Some(iso_full_slash(&m[1]));
            period_end = Some(iso_full_slash(&m[2]));
        }
        declared_empty |= empty.is_match(line);
        if running.is_none() {
            if let Some(m) = open.captures(line) {
                running = Some(money(&m[1]));
                opening = running;
            }
            continue;
        }
        if let Some(m) = txn.captures(line) {
            let amount = money(&m[3]);
            let next = money(&m[4]);
            let signed = running_sign(running.unwrap_or_default(), amount, next);
            running = Some(next);
            let date = iso_yy_slash(&m[1]);
            out.push(basic_line(
                date.clone(),
                Some(date),
                m[2].trim().to_owned(),
                signed,
                Some(next),
            ));
        }
    }
    let mut result = statement(out, opening, running);
    result.account_number = account_number;
    result.holder = holder_name;
    result.period_start = period_start.or(result.period_start);
    result.period_end = period_end.or(result.period_end);
    result.declared_empty = declared_empty;
    result
}

fn parse_scb(lines: &[String]) -> ParsedStatement {
    let txn = re(
        r"^(\d{2}/\d{2}/\d{2})\s+\d{2}:\d{2}\s+\w+\s+\w+\s+([\d,]+\.\d{2})\s+([\d,]+\.\d{2})\s*(.*?)$",
    );
    let open = re(r"(?i)(?:BALANCE BROUGHT FORWARD|ยอดเงินคงเหลือยกมา).*?([\d,]+\.\d{2})$");
    let acct = re(r"(?i)(?:เลขที่บัญชี|ACCOUNT\s*(?:NO|NUMBER)\.?)\s*[:：]?\s*(\d[\d\- ]{7,15}\d)");
    let holder = re(r"(?i)(?:ชื่อบัญชี|ชื่อ\s*-\s*สกุล|ACCOUNT\s*NAME)\s*[:：]?\s*(.+)$");
    let (mut running, mut opening, mut account_number, mut holder_name) = (None, None, None, None);
    let mut out = Vec::new();
    let mut declared_empty = false;
    for line in lines {
        if account_number.is_none() {
            account_number = acct.captures(line).map(|m| m[1].trim().to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.captures(line).map(|m| m[1].trim().to_owned());
        }
        declared_empty |= line.contains("TOTAL ITEMS 0 0");
        if running.is_none() {
            if let Some(m) = open.captures(line) {
                running = Some(money(&m[1]));
                opening = running;
            }
            continue;
        }
        if let Some(m) = txn.captures(line) {
            let amount = money(&m[2]);
            let next = money(&m[3]);
            let signed = running_sign(running.unwrap_or_default(), amount, next);
            running = Some(next);
            let date = iso_yy_slash(&m[1]);
            out.push(basic_line(
                date.clone(),
                Some(date),
                m[4].trim().to_owned(),
                signed,
                Some(next),
            ));
        }
    }
    let mut result = statement(out, opening, running);
    if let Some((start, end)) = declared_full_date_range(lines) {
        result.period_start = Some(start);
        result.period_end = Some(end);
    }
    result.account_number = account_number;
    result.holder = holder_name;
    result.declared_empty = declared_empty;
    result
}

fn parse_gsb(lines: &[String]) -> ParsedStatement {
    let txn = re(
        r"^(\d{2}/\d{2}/\d{4})\s+(.+?)\s+([\d,]+\.\d{2})\s+([\d,]+\.\d{2})(?:\s+(\d+)\s+(\d+))?$",
    );
    let open = re(r"ยอด\s*ยก\s*มา\s+([\d,]+\.\d{2})$");
    let close = re(r"ยอด\s*ยก\s*ไป\s+([\d,]+\.\d{2})$");
    let acct = re(r"เลข\s*ที่\s*บัญชี\s*[:：]?\s*(\d[\d\- ]{7,20}\d)");
    let holder = re(r"ชื่อ\s*บัญชี\s*[:：]?\s*(.+)$");
    let period = re(r"(\d{2}/\d{2}/\d{4})\s*[-–]\s*(\d{2}/\d{2}/\d{4})");
    let zero_debit = re(r"ยอดรวมรายการถอน\s*=\s*0\s+รายการ");
    let zero_credit = re(r"ยอดรวมรายการฝาก\s*=\s*0\s+รายการ");
    let (
        mut running,
        mut opening,
        mut closing,
        mut account_number,
        mut holder_name,
        mut ps,
        mut pe,
    ) = (None, None, None, None, None, None, None);
    let (mut zd, mut zc) = (false, false);
    let mut out = Vec::new();
    for line in lines {
        if account_number.is_none() {
            account_number = acct.captures(line).map(|m| m[1].trim().to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.captures(line).map(|m| m[1].trim().to_owned());
        }
        if let Some(m) = period.captures(line) {
            ps = Some(iso_full_slash(&m[1]));
            pe = Some(iso_full_slash(&m[2]));
        }
        zd |= zero_debit.is_match(line);
        zc |= zero_credit.is_match(line);
        if running.is_none() {
            if let Some(m) = open.captures(line) {
                running = Some(money(&m[1]));
                opening = running;
            }
            continue;
        }
        if let Some(m) = close.captures(line) {
            closing = Some(money(&m[1]));
            continue;
        }
        if let Some(m) = txn.captures(line) {
            let amount = money(&m[3]);
            let next = money(&m[4]);
            let signed = running_sign(running.unwrap_or_default(), amount, next);
            running = Some(next);
            let date = iso_full_slash(&m[1]);
            let row = basic_line(
                date.clone(),
                Some(date),
                m[2].trim().to_owned(),
                signed,
                Some(next),
            );
            out.push(row);
        }
    }
    let mut result = statement(out, opening, closing.or(running));
    result.account_number = account_number;
    result.holder = holder_name;
    result.period_start = ps.or(result.period_start);
    result.period_end = pe.or(result.period_end);
    result.declared_empty = zd && zc;
    result
}

fn parse_kbank_savings(lines: &[String]) -> ParsedStatement {
    let txn = re(
        r"^(\d{2}-\d{2}-\d{2})\s+\d{2}:\d{2}\s+(.+?)\s+([\d,]+\.\d{2})\s+([\d,]+\.\d{2})\s*(.*?)$",
    );
    let open = re(r"(?i)^(\d{2}-\d{2}-\d{2})\s+(?:Beginning Balance|ยอดยกมา)\s+([\d,]+\.\d{2})$");
    let acct = re(
        r"(?i)(?:Account\s*(?:No\.?|Number)|A/C\s*No\.?|เลขที่บัญชี|หมายเลขบัญชี)\s*[:：]?\s*([0-9][0-9\s-]{5,}[0-9])",
    );
    let (mut running, mut opening, mut account_number) = (None, None, None);
    let mut out = Vec::new();
    for line in lines {
        if account_number.is_none() {
            account_number = acct
                .captures(line)
                .map(|m| normalize_space(&m[1]).replace(' ', ""));
        }
        if running.is_none() {
            if let Some(m) = open.captures(line) {
                running = Some(money(&m[2]));
                opening = running;
            }
            continue;
        }
        if let Some(m) = txn.captures(line) {
            let amount = money(&m[3]);
            let next = money(&m[4]);
            let signed = running_sign(running.unwrap_or_default(), amount, next);
            running = Some(next);
            let date = iso_yy_dash(&m[1]);
            let row = basic_line(
                date.clone(),
                Some(date),
                format!("{} {}", m[2].trim(), m[5].trim()).trim().to_owned(),
                signed,
                Some(next),
            );
            out.push(row);
        }
    }
    let mut result = statement(out, opening, running);
    if let Some((start, end)) = declared_full_date_range(lines) {
        result.period_start = Some(start);
        result.period_end = Some(end);
    }
    result.account_number = account_number;
    result
}

fn parse_kkp(lines: &[String]) -> ParsedStatement {
    let date = re(r"^\d{2}/\d{2}/20\d{2}$");
    let amount = re(r"^[\d,]+\.\d{2}$");
    let (mut running, mut opening) = (None, None);
    let mut out = Vec::new();
    for line in lines {
        let mut tokens = line
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut i = 0;
        while i + 1 < tokens.len() {
            let split_date = re(r"^\d{2}/\d{2}/20$").is_match(&tokens[i])
                && re(r"^\d{2}$").is_match(&tokens[i + 1]);
            let split_amount = re(r"^[\d,]+$").is_match(&tokens[i])
                && re(r"^\d*\.\d{2}$").is_match(&tokens[i + 1]);
            if split_date || split_amount {
                let suffix = tokens.remove(i + 1);
                tokens[i].push_str(&suffix);
            } else {
                i += 1;
            }
        }
        if tokens.is_empty() || !date.is_match(&tokens[0]) {
            continue;
        }
        let amounts = tokens
            .iter()
            .filter(|v| amount.is_match(v))
            .cloned()
            .collect::<Vec<_>>();
        let skip = tokens.iter().any(|v| {
            matches!(
                v.as_str(),
                "Balan" | "Balance" | "Forward" | "ยอดยกมา" | "ยกมา"
            )
        });
        if amounts.is_empty() {
            continue;
        }
        if amounts.len() == 1 || skip {
            if running.is_none() {
                running = Some(money(&amounts[0]));
                opening = running;
            }
            continue;
        }
        let raw_amount = &amounts[amounts.len() - 2];
        let next = money(&amounts[amounts.len() - 1]);
        let signed = running_sign(running.unwrap_or_default(), money(raw_amount), next);
        if running.is_none() {
            opening = Some(0);
        }
        running = Some(next);
        let desc = tokens
            .iter()
            .skip(1)
            .take_while(|v| !amount.is_match(v))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let d = iso_full_slash(&tokens[0]);
        let row = basic_line(d.clone(), Some(d), desc, signed, Some(next));
        out.push(row);
    }
    let mut result = statement(out, opening, running);
    if let Some((start, end)) = declared_full_date_range(lines) {
        result.period_start = Some(start);
        result.period_end = Some(end);
    }
    result
}

fn parse_cardx(lines: &[String]) -> Result<ParsedStatement, ParseError> {
    let summary = re(
        r"^\s*(\d{1,2})/(\d{1,2})/(\d{2,4})\s+-?[\d,]+\.\d{2}\s+-?[\d,]+\.\d{2}\s+\d{1,2}/\d{1,2}/\d{2,4}\s*$",
    );
    let closing = re(r"(?i)(?:CLOSING DATE|วันสรุปยอด)");
    let date_any = re(r"\b(\d{1,2})/(\d{1,2})/(\d{2,4})\b");
    let mut anchor = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(m) = summary.captures(line) {
            anchor = anchor_parts(&m[1], &m[2], &m[3]);
            break;
        }
        if closing.is_match(line) {
            for next in lines.iter().skip(i).take(3) {
                if let Some(m) = date_any.captures(next) {
                    anchor = anchor_parts(&m[1], &m[2], &m[3]);
                    if anchor.is_some() {
                        break;
                    }
                }
            }
        }
        if anchor.is_some() {
            break;
        }
    }
    let(year,month,day)=anchor.ok_or_else(||ParseError::Format("CardX parser could not determine statement month/year from an in-document closing date".into()))?;
    let txn = re(
        r"^(\d{2}/\d{2})\s+(\d{2}/\d{2})\s+(.+?)\s+(?:(USD|AUD|EUR|GBP|JPY|SGD|HKD|CNY|TWD|MYR|IDR|VND|PHP|KRW|NZD|CHF|CAD|DKK|NOK|SEK|THB|INR|ZAR|QAR|AED|BHD|KWD)\s+(-?[\d,]+\.\d{2})\s+)?(-?[\d,]+\.\d{2})$",
    );
    let prev = re(
        r"(?i)(?:PREVIOUS BALANCE|ยอดเรียกเก็บรอบที่แล้ว|ยอดยกมา|ยอดค้างชำระยกมา)[^0-9+\-]*?([+-]?[\d,]+\.\d+)",
    );
    let total = re(
        r"(?i)(?:TOTAL BALANCE|ยอดที่ต้องชำาระ|ยอดที่ต้องชำระ|ยอดรวมที่ต้องชำระ|ยอดค้างชำระรวม)[^0-9+\-]*?([+-]?[\d,]+\.\d+)",
    );
    let (mut previous, mut closing_balance) = (None, None);
    let mut out = Vec::new();
    for line in lines {
        if previous.is_none() {
            previous = prev.captures(line).map(|m| balance_money(&m[1]));
        }
        if closing_balance.is_none() {
            closing_balance = total.captures(line).map(|m| balance_money(&m[1]));
        }
        if let Some(m) = txn.captures(line) {
            let desc = m[3].trim().to_owned();
            let printed = money(&m[6]);
            let lower = desc.to_lowercase();
            let credit = printed < 0
                || [
                    "payment received from easy app",
                    "payment received,thk you",
                    "payment - thank you",
                    "payment received",
                ]
                .iter()
                .any(|v| lower.contains(v));
            let amount = if credit { printed.abs() } else { -printed };
            let mut row = basic_line(
                card_date(&m[1], year, month),
                Some(card_date(&m[2], year, month)),
                desc,
                amount,
                None,
            );
            if let (Some(currency), Some(fx)) = (m.get(4), m.get(5)) {
                row.foreign_amount_satang = Some(foreign_money(fx.as_str(), currency.as_str()));
                row.foreign_currency = Some(currency.as_str().to_owned());
            }
            out.push(row);
        }
    }
    let mut result = statement(out, previous.map(|v| -v), closing_balance.map(|v| -v));
    let (period_start, period_end) = card_statement_period(year, month, day).ok_or_else(|| {
        ParseError::Format("CardX parser found an invalid in-document closing date".into())
    })?;
    result.period_start = Some(period_start);
    result.period_end = Some(period_end);
    Ok(result)
}

fn anchor_parts(day: &str, month: &str, year: &str) -> Option<(i32, u32, u32)> {
    let day = day.parse::<u32>().ok()?;
    let month = month.parse::<u32>().ok()?;
    let mut year = year.parse::<i32>().ok()?;
    if (2400..=2699).contains(&year) {
        year -= 543;
    } else if (0..=99).contains(&year) {
        year += 2000;
    }
    if NaiveDate::from_ymd_opt(year, month, day).is_none() {
        None
    } else {
        Some((year, month, day))
    }
}

fn card_statement_period(year: i32, month: u32, day: u32) -> Option<(String, String)> {
    let end = NaiveDate::from_ymd_opt(year, month, day)?;
    let (previous_year, previous_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    let next_month = if previous_month == 12 {
        NaiveDate::from_ymd_opt(previous_year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(previous_year, previous_month + 1, 1)?
    };
    let previous_month_end = next_month - ChronoDuration::days(1);
    let previous_close = NaiveDate::from_ymd_opt(
        previous_year,
        previous_month,
        day.min(previous_month_end.day()),
    )?;
    Some((
        (previous_close + ChronoDuration::days(1)).to_string(),
        end.to_string(),
    ))
}
fn card_date(ddmm: &str, year: i32, statement_month: u32) -> String {
    let p = ddmm.split('/').collect::<Vec<_>>();
    let month = p[1].parse::<u32>().unwrap_or_default();
    let year = if month <= statement_month + 1 {
        year
    } else {
        year - 1
    };
    format!("{year:04}-{month:02}-{:0>2}", p[0])
}

fn kbank_card_section_key(
    label: &str,
    first: &str,
    masked_middle: &str,
    last: Option<&str>,
) -> String {
    let mut slug = label
        .to_ascii_lowercase()
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    while slug.contains("__") {
        slug = slug.replace("__", "_");
    }
    let slug = slug.trim_matches('_');
    let slug = if slug.is_empty() { "card" } else { slug };
    let base = format!(
        "kbank_card:{slug}:{}:{}",
        first.to_ascii_lowercase(),
        masked_middle.to_ascii_lowercase()
    );
    last.map_or(base.clone(), |value| {
        format!("{base}:{}", value.to_ascii_lowercase())
    })
}

fn kbank_card_matches(label: Option<&str>, key: Option<&str>, selector: &str) -> bool {
    if selector.starts_with("kbank_card:") {
        return key == Some(selector);
    }
    selector.is_empty()
        || label.is_none_or(|value| {
            let lower = value.to_lowercase();
            selector.to_lowercase().split_whitespace().all(|token| {
                lower
                    .split(|character: char| character.is_whitespace() || "-_/".contains(character))
                    .any(|part| part == token)
            })
        })
}

fn parse_kbank_card(lines: &[String], account_name: &str) -> Result<ParsedStatement, ParseError> {
    // Row dates only passed a shape regex; 31/02 must fail, not panic.
    let date = |value: &str| {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| ParseError::Format("kbank_card: invalid calendar date".to_owned()))
    };
    let txn = re(
        r"^(\d{2}/\d{2}/\d{2})\s+(\d{2}/\d{2}/\d{2})\s+(.+?)\s+(?:(USD|AUD|EUR|GBP|JPY|SGD|HKD|CNY|TWD|MYR|IDR|VND|PHP|KRW|NZD|CHF|CAD|DKK|NOK|SEK|THB|INR|ZAR|QAR|AED|BHD|KWD)\s+(-?[\d,]+\.\d{2})\s+)?(-?[\d,]+\.\d{2})$",
    );
    let txn_prefix = re(r"^(\d{2}/\d{2}/\d{2})\s+(\d{2}/\d{2}/\d{2})\s+\S");
    let card = re(
        r"(?:/ (?:ACCOUNT DETAILS|รายละเอียด(?:บัญชี|บัตร))\s+)?(.+?)\s+(\d{4})\s+(\d{2}XX)(?:\s+XXXX\s+(\d{4}))?",
    );
    let prev = re(
        r"(?i)(?:PREVIOUS BALANCE|ยอดเรียกเก็บรอบที่แล้ว|ยอดยกมา|ยอดค้างชำระยกมา)[^0-9+\-]*?([+-]?[\d,]+\.\d+)",
    );
    let total = re(
        r"(?i)(?:TOTAL BALANCE|ยอดที่ต้องชำาระ|ยอดที่ต้องชำระ|ยอดรวมที่ต้องชำระ|ยอดค้างชำระรวม)[^0-9+\-]*?([+-]?[\d,]+\.\d+)",
    );
    let statement_date = re(r"(?i)(?:STATEMENT\s+DATE|วันสรุปยอด).*?(\d{1,2})/(\d{1,2})/(\d{4})");
    // Explicit zero-activity wording for the selected card: a whole line,
    // inside that card's section (before its TOTAL BALANCE). Equal balances,
    // or the phrase inside other text such as footer terms, never certify an
    // empty cycle.
    // ponytail: marker wording is unverified against a real zero-activity
    // KBank PDF; confirm against one before relying on it.
    let no_activity = re(
        r"(?i)^\s*(?:NO\s+TRANSACTIONS?(?:\s+(?:THIS|FOR\s+THIS)\s+(?:CYCLE|PERIOD|STATEMENT))?|ไม่มีรายการ(?:เคลื่อนไหว)?(?:ในรอบนี้)?)\s*\.?\s*$",
    );
    let mut saw_no_activity = false;
    let mut closing_date = None;
    let (mut current, mut current_key, mut previous, mut closing) =
        (None::<String>, None::<String>, None, None);
    let mut saw_selected_header = false;
    let mut out = Vec::new();
    for line in lines {
        if closing_date.is_none()
            && let Some(found) = statement_date.captures(line)
        {
            closing_date = anchor_parts(&found[1], &found[2], &found[3]);
        }
        if let Some(m) = card.captures(line) {
            let label = m[1].trim().to_owned();
            current_key = Some(kbank_card_section_key(
                &label,
                &m[2],
                &m[3],
                m.get(4).map(|value| value.as_str()),
            ));
            current = Some(label);
            saw_selected_header |=
                kbank_card_matches(current.as_deref(), current_key.as_deref(), account_name);
            continue;
        }
        let matches = kbank_card_matches(current.as_deref(), current_key.as_deref(), account_name);
        if matches && previous.is_none() {
            previous = prev.captures(line).map(|m| balance_money(&m[1]));
        }
        saw_no_activity |= matches && closing.is_none() && no_activity.is_match(line);
        if matches && closing.is_none() {
            closing = total.captures(line).map(|m| balance_money(&m[1]));
        }
        if !matches {
            continue;
        }
        if let Some(m) = txn.captures(line) {
            let transaction_date = iso_yy_slash(&m[1]);
            let posting_date = iso_yy_slash(&m[2]);
            let (transaction_on, posting_on) = (date(&transaction_date)?, date(&posting_date)?);
            let mut economic_date = transaction_date.clone();
            if let Some((year, month, day)) = closing_date
                && let Some((period_start, period_end)) = card_statement_period(year, month, day)
            {
                let (start, end) = (date(&period_start)?, date(&period_end)?);
                let in_cycle =
                    |date: NaiveDate| date >= start - ChronoDuration::days(1) && date <= end;
                // KBank may print a posting on the prior cycle's closing day.
                // Installments can retain the original purchase date in the
                // first column while their current-cycle posting date appears
                // in the second. Footer examples have neither date in-cycle.
                if in_cycle(transaction_on) {
                    economic_date.clone_from(&transaction_date);
                } else if in_cycle(posting_on) {
                    economic_date.clone_from(&posting_date);
                } else {
                    continue;
                }
            }
            let mut row = basic_line(
                economic_date,
                Some(posting_date),
                m[3].trim().to_owned(),
                -money(&m[6]),
                None,
            );
            if let (Some(currency), Some(fx)) = (m.get(4), m.get(5)) {
                row.foreign_amount_satang = Some(foreign_money(fx.as_str(), currency.as_str()));
                row.foreign_currency = Some(currency.as_str().to_owned());
            }
            out.push(row);
        } else if let Some(prefix) = txn_prefix.captures(line) {
            // An in-cycle date-prefixed row whose amount could not be read must
            // fail the statement rather than silently disappearing (offsetting
            // unreadable rows could otherwise still reconcile to `posted`).
            let transaction_date = iso_yy_slash(&prefix[1]);
            let posting_date = iso_yy_slash(&prefix[2]);
            let row_in_cycle = if let Some((year, month, day)) = closing_date
                && let Some((period_start, period_end)) = card_statement_period(year, month, day)
                && let (Ok(start), Ok(end)) = (date(&period_start), date(&period_end))
            {
                let in_cycle = |d: NaiveDate| d >= start - ChronoDuration::days(1) && d <= end;
                date(&transaction_date).map_or(true, in_cycle)
                    || date(&posting_date).map_or(true, in_cycle)
            } else {
                true
            };
            if row_in_cycle {
                INVALID_AMOUNT.set(true);
            }
        }
    }
    let mut result = statement(out, previous.map(|v| -v), closing.map(|v| -v));
    if let Some((year, month, day)) = closing_date
        && let Some((start, end)) = card_statement_period(year, month, day)
    {
        result.period_start = Some(start);
        result.period_end = Some(end);
    }
    result.declared_empty = !account_name.is_empty()
        && result.lines.is_empty()
        && saw_selected_header
        && saw_no_activity
        && previous
            .zip(closing)
            .is_none_or(|(opening, closing)| (opening - closing).abs() <= 1);
    Ok(result)
}

fn parse_ktc(lines: &[String]) -> ParsedStatement {
    let txn = re(r"^(\d{2}/\d{2}/\d{2})\s+(\d{2}/\d{2}/\d{2})\s+(.+?)\s+(-?[\d,]+\.\d{2})$");
    let prev = re(r"ยอดเรียกเก็บรอบที่แล้ว\s+(-?[\d,]+\.\d{2})");
    let total = re(r"(?:ยอดที่ต้องชำาระ/ชำาระเกิน\s*\(-\)|สรุปยอดรอบนี้\s+.+?)\s+(-?[\d,]+\.\d{2})$");
    let minimum = re(r"ยอดขั้นตำ่าที่ต้องชำาระ\s+(-?[\d,]+\.\d{2})");
    let card = re(r"\b\d{4}-\d{2}XX-XXXX-\d{4}\b");
    let holder = re(r"\b(?:MR|MRS|MS|MISS|DR)\.\s+[A-Z][A-Z .'-]+");
    let (mut previous, mut closing, mut min, mut account_number, mut holder_name) =
        (None, None, None, None, None);
    let mut out = Vec::new();
    for line in lines {
        if account_number.is_none() {
            account_number = card.find(line).map(|m| m.as_str().to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.find(line).map(|m| m.as_str().trim().to_owned());
        }
        if previous.is_none() {
            previous = prev.captures(line).map(|m| money(&m[1]));
        }
        if closing.is_none() {
            closing = total.captures(line).map(|m| money(&m[1]));
        }
        if min.is_none() {
            min = minimum.captures(line).map(|m| money(&m[1]));
        }
        if let Some(m) = txn.captures(line) {
            let row = basic_line(
                iso_yy_slash(&m[1]),
                Some(iso_yy_slash(&m[2])),
                m[3].trim().to_owned(),
                -money(&m[4]),
                None,
            );
            out.push(row);
        }
    }
    let mut result = statement(out, previous.map(|v| -v), closing.map(|v| -v));
    result.account_number = account_number;
    result.holder = holder_name;
    result
}

fn parse_krungsri_card(lines: &[String]) -> ParsedStatement {
    let installment = re(
        r"^(\d{2}/\d{2}/\d{2})\s+(\d{2}/\d{2}/\d{2})\s+(.+?)\s+([\d,]+\.\d{2})\s+(\d{1,3}/\d{1,3})\s+([\d,]+\.\d{2})$",
    );
    let txn = re(r"^(\d{2}/\d{2}/\d{2})\s+(\d{2}/\d{2}/\d{2})\s+(.+?)\s+(-?[\d,]+\.\d{2})$");
    let prev = re(r"ยอดเรียกเก็บของรอบบัญชี[^\d]{0,14}([\d,]+\.\d{2})");
    let total = re(r"Total Payment Due For Credit Card\s+([\d,]+\.\d{2})");
    let card = re(r"\b(\d{4} \d{2}XX XXXX \d{4})\b");
    let holder = re(r"\(([A-Z]\.?\s*[A-Z][A-Z .'-]+)\)");
    let (mut previous, mut closing, mut account_number, mut holder_name) = (None, None, None, None);
    let mut out = Vec::new();
    for line in lines {
        if account_number.is_none() {
            account_number = card.captures(line).map(|m| m[1].to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.captures(line).map(|m| m[1].trim().to_owned());
        }
        if previous.is_none() {
            previous = prev.captures(line).map(|m| money(&m[1]));
        }
        if closing.is_none() {
            closing = total.captures(line).map(|m| money(&m[1]));
        }
        if let Some(m) = installment.captures(line) {
            let row = basic_line(
                iso_yy_slash(&m[2]),
                Some(iso_yy_slash(&m[1])),
                format!("{} {}", m[3].trim(), &m[5]),
                -money(&m[6]),
                None,
            );
            out.push(row);
            continue;
        }
        if let Some(m) = txn.captures(line) {
            let row = basic_line(
                iso_yy_slash(&m[1]),
                Some(iso_yy_slash(&m[2])),
                m[3].trim().to_owned(),
                -money(&m[4]),
                None,
            );
            out.push(row);
        }
    }
    let mut result = statement(out, previous.map(|v| -v), closing.map(|v| -v));
    result.account_number = account_number;
    result.holder = holder_name;
    result
}

fn parse_ktb(lines: &[String]) -> ParsedStatement {
    let txn = re(
        r"^(\d{2}/\d{2}/\d{2})\s+(.+?)\s+(-?[\d,]+\.\d{2}(?:\s+-?[\d,]+\.\d{2}){1,2})(?:\s+\d+)?$",
    );
    let amounts = re(r"-?[\d,]+\.\d{2}");
    let acct = re(r"(?i)(?:Account Number|เลขที่บัญชี)\s+(\d[\d\- ]*\d)");
    let holder = re(r"(?i)(?:Account Name|ชื่อบัญชี)\s+(.+?)(?:\s+(?:Account Type\b|ประเภทบัญชี)|$)");
    let opening_re = re(
        r"(?i)(?:ยอดยกมา|ยอดเงินยกมา|ยอดคงเหลือยกมา|balance brought forward|brought forward balance|beginning balance|opening balance|previous balance)[^0-9+\-]*?([+-]?[\d,]+\.\d+)",
    );
    let debit = re(r"(?i)transfer out|withdraw|payment|debit|โอนออก|ถอน|ชำาระ|ชำระ|หักบัญชี|เดบิต");
    let credit =
        re(r"(?i)transfer in|deposit|credit|interest|salary|โอนเข้า|ฝาก|รับโอน|ดอกเบี้ย|เครดิต|เงินเดือน");
    let (mut previous, mut opening_anchor, mut account_number, mut holder_name) =
        (None, None, None, None);
    let mut out = Vec::new();
    for line in lines {
        if account_number.is_none() {
            account_number = acct.captures(line).map(|m| m[1].trim().to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.captures(line).map(|m| m[1].trim().to_owned());
        }
        if opening_anchor.is_none() {
            opening_anchor = opening_re.captures(line).map(|m| balance_money(&m[1]));
        }
        let Some(m) = txn.captures(line) else {
            continue;
        };
        let cols = amounts
            .find_iter(&m[3])
            .map(|v| v.as_str())
            .collect::<Vec<_>>();
        let (desc, date) = (m[2].trim(), iso_yy_slash(&m[1]));
        // The string labels document how each row's direction was decided.
        let (amount, balance, verified, _direction_source) = if cols.len() >= 3 {
            let withdrawal = money(cols[cols.len() - 3]);
            let deposit = money(cols[cols.len() - 2]);
            (
                if deposit != 0 { deposit } else { -withdrawal },
                money(cols[cols.len() - 1]),
                true,
                "explicit-columns",
            )
        } else {
            let raw = money(cols[0]);
            let balance = money(cols[1]);
            if let Some(prev) = previous {
                (
                    running_sign(prev, raw, balance),
                    balance,
                    true,
                    "previous-balance",
                )
            } else if let Some(anchor) = opening_anchor {
                (
                    running_sign(anchor, raw, balance),
                    balance,
                    true,
                    "opening-anchor",
                )
            } else if debit.is_match(desc) {
                (-raw, balance, true, "description-keyword")
            } else if credit.is_match(desc) {
                (raw, balance, true, "description-keyword")
            } else {
                (raw, balance, false, "unknown-first-row")
            }
        };
        if opening_anchor.is_none() && out.is_empty() && verified {
            opening_anchor = Some(balance - amount);
        }
        previous = Some(balance);
        let row = basic_line(
            date.clone(),
            Some(date),
            desc.to_owned(),
            amount,
            Some(balance),
        );
        out.push(row);
    }
    let closing = previous;
    let mut result = statement(out, opening_anchor, closing);
    result.account_number = account_number;
    result.holder = holder_name;
    result
}

fn thai_month(raw: &str) -> Option<u32> {
    Some(match raw {
        "ม.ค." => 1,
        "ก.พ." => 2,
        "มี.ค." => 3,
        "เม.ย." => 4,
        "พ.ค." => 5,
        "มิ.ย." => 6,
        "ก.ค." => 7,
        "ส.ค." => 8,
        "ก.ย." => 9,
        "ต.ค." => 10,
        "พ.ย." => 11,
        "ธ.ค." => 12,
        _ => return None,
    })
}
fn parse_ttb(lines: &[String]) -> ParsedStatement {
    let txn = re(
        r"^(\d{1,2})\s+([\u0E00-\u0E7F.]+)\s+(\d{2})\s+(\d{2}:\d{2})\s+(.+?)\s+([-+]?[\d,]+\.\d{2})\s+([\d,]+\.\d{2})(?:\s+(.*))?$",
    );
    let acct = re(r"\b(\d{3}-\d-\d{5}-\d)\b");
    let holder = re(r"((?:น\.ส\.|นาย|นาง)\s+[ก-๙]+\s+[ก-๙]+)");
    let (mut account_number, mut holder_name) = (None, None);
    let mut out = Vec::new();
    for line in lines {
        if account_number.is_none() {
            account_number = acct.captures(line).map(|m| m[1].to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.captures(line).map(|m| m[1].to_owned());
        }
        if let Some(m) = txn.captures(line) {
            let Some(month) = thai_month(&m[2]) else {
                continue;
            };
            let year = 2500 + m[3].parse::<i32>().unwrap_or_default() - 543;
            let date = format!("{year:04}-{month:02}-{:0>2}", &m[1]);
            let desc = format!(
                "{} {}",
                m[5].trim(),
                m.get(8).map_or("", |v| v.as_str().trim())
            )
            .trim()
            .to_owned();
            let row = basic_line(
                date.clone(),
                Some(date),
                desc,
                // TTB prints deposits with a leading '+' (e.g. `+1,000.00`).
                money(m[6].strip_prefix('+').unwrap_or(&m[6])),
                Some(money(&m[7])),
            );
            out.push(row);
        }
    }
    out.reverse();
    let opening = out
        .first()
        .and_then(|r| r.balance_after_satang.map(|b| b - r.amount_satang));
    let closing = out.last().and_then(|r| r.balance_after_satang);
    let mut result = statement(out, opening, closing);
    result.account_number = account_number;
    result.holder = holder_name;
    result
}

fn parse_uob_card(lines: &[String]) -> ParsedStatement {
    let txn =
        re(r"^(\d{1,2})\s+([A-Z]{3})\s+(\d{1,2})\s+([A-Z]{3})\s+(.+?)\s+([\d,]+\.\d{2})(\s+CR)?$");
    let fx = re(
        r"(USD|AUD|EUR|GBP|JPY|SGD|HKD|CNY|TWD|MYR|IDR|VND|PHP|KRW|NZD|CHF|CAD|DKK|NOK|SEK|THB|INR|ZAR|QAR|AED|BHD|KWD)\s+([\d,]+\.\d{2})$",
    );
    let stmt_date = re(r"STATEMENT DATE\s+(\d{1,2})\s+([A-Z]{3})\s+(\d{4})");
    let acct = re(r"ACCOUNT NUMBER\s+([\d-]+)");
    let prev = re(r"PREVIOUS BALANCE\s+([\d,]+\.\d{2})");
    let total = re(r"TOTAL BALANCE\s*-\s*UOB\s+\w+\s+([\d,]+\.\d{2})");
    let holder = re(r"\b(?:MR|MRS|MS|MISS|DR)\.?\s+[A-Z][A-Z .'-]{2,}");
    let (mut anchor, mut previous, mut closing, mut account_number, mut holder_name) =
        (None, None, None, None, None);
    let mut out = Vec::new();
    for line in lines {
        if anchor.is_none() {
            anchor = stmt_date.captures(line).and_then(|m| {
                month_en(&m[2]).map(|mon| (m[3].parse::<i32>().unwrap_or_default(), mon))
            });
        }
        if account_number.is_none() {
            account_number = acct.captures(line).map(|m| m[1].to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.find(line).map(|m| m.as_str().trim().to_owned());
        }
        if previous.is_none() {
            previous = prev.captures(line).map(|m| money(&m[1]));
        }
        if closing.is_none() {
            closing = total.captures(line).map(|m| money(&m[1]));
        }
        let (Some(m), Some((year, statement_month))) = (txn.captures(line), anchor) else {
            continue;
        };
        let Some(post_month) = month_en(&m[2]) else {
            continue;
        };
        let Some(trans_month) = month_en(&m[4]) else {
            continue;
        };
        let date_for = |day: &str, month: u32| {
            let y = if month > statement_month {
                year - 1
            } else {
                year
            };
            format!("{y:04}-{month:02}-{day:0>2}")
        };
        let desc = m[5].trim().to_owned();
        let mut row = basic_line(
            date_for(&m[3], trans_month),
            Some(date_for(&m[1], post_month)),
            desc.clone(),
            if m.get(7).is_some() {
                money(&m[6])
            } else {
                -money(&m[6])
            },
            None,
        );
        if let Some(f) = fx.captures(&desc) {
            row.foreign_currency = Some(f[1].to_owned());
            row.foreign_amount_satang = Some(foreign_money(&f[2], &f[1]));
        }
        out.push(row);
    }
    let mut result = statement(out, previous.map(|v| -v), closing.map(|v| -v));
    result.account_number = account_number;
    result.holder = holder_name;
    result
}
fn month_en(raw: &str) -> Option<u32> {
    Some(match raw.to_ascii_uppercase().as_str() {
        "JAN" => 1,
        "FEB" => 2,
        "MAR" => 3,
        "APR" => 4,
        "MAY" => 5,
        "JUN" => 6,
        "JUL" => 7,
        "AUG" => 8,
        "SEP" => 9,
        "OCT" => 10,
        "NOV" => 11,
        "DEC" => 12,
        _ => return None,
    })
}

fn parse_uob_savings(lines: &[String]) -> ParsedStatement {
    let period_re = re(
        r"(?i)Period:\s*(\d{1,2})\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+(\d{4})\s+to\s+(\d{1,2})\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+(\d{4})",
    );
    let acct = re(r"\b(\d{3}-\d{3}-\d{3}-\d)\b");
    let txn = re(
        r"(?i)^(\d{1,2})\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+(\d{1,2})\s+(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+(.+?)\s+([\d,]+\.\d{2}(?:\s+[\d,]+\.\d{2})?)$",
    );
    let amounts = re(r"[\d,]+\.\d{2}");
    let total = re(r"(?i)^Total\s+[\d,]+\.\d{2}\s+[\d,]+\.\d{2}\s+([\d,]+\.\d{2})$");
    let (mut period, mut account_number, mut total_closing) = (None, None, None);
    for line in lines {
        if period.is_none() {
            period = period_re.captures(line).and_then(|m| {
                let sy = m[3].parse::<i32>().ok()?;
                let ey = m[6].parse::<i32>().ok()?;
                let sm = month_en(&m[2])?;
                let em = month_en(&m[5])?;
                Some((
                    format!("{sy:04}-{sm:02}-{:0>2}", &m[1]),
                    format!("{ey:04}-{em:02}-{:0>2}", &m[4]),
                    sy,
                    ey,
                ))
            });
        }
        if account_number.is_none() {
            account_number = acct.captures(line).map(|m| m[1].to_owned());
        }
        if total_closing.is_none() {
            total_closing = total.captures(line).map(|m| money(&m[1]));
        }
    }
    let (mut opening, mut running) = (None, None);
    let mut out = Vec::new();
    for line in lines {
        let (Some(m), Some((start, end, sy, ey))) = (txn.captures(line), period.as_ref()) else {
            continue;
        };
        let vals = amounts
            .find_iter(&m[6])
            .map(|v| v.as_str())
            .collect::<Vec<_>>();
        let first = money(vals[0]);
        let row_balance = vals.get(1).map(|v| money(v));
        let desc = m[5].trim();
        if re(r"(?i)^BALANCE\s+B/F$").is_match(desc) {
            opening = Some(first);
            running = Some(first);
            continue;
        }
        let upper = desc.to_ascii_uppercase();
        let keyword = if re(r"\b(?:TRF\s+DEP|DEPOSIT|DEP|INT|INTEREST)\b").is_match(&upper) {
            Some(1)
        } else if re(r"\b(?:TRF\s+WD|WITHDRAW|WITHDRAWAL|WD|FEE)\b").is_match(&upper) {
            Some(-1)
        } else {
            None
        };
        let sign = keyword.or_else(|| {
            running.zip(row_balance).and_then(|(old, new)| {
                if (old + first - new).abs() <= 1 {
                    Some(1)
                } else if (old - first - new).abs() <= 1 {
                    Some(-1)
                } else {
                    None
                }
            })
        });
        let Some(sign) = sign else { continue };
        let amount = sign * first;
        let after = row_balance.or_else(|| running.map(|v| v + amount));
        if let Some(v) = after {
            running = Some(v);
        }
        let within = |day: &str, mon: &str| {
            let month = month_en(mon).unwrap_or_default();
            [sy, ey]
                .into_iter()
                .map(|year| format!("{year:04}-{month:02}-{day:0>2}"))
                .find(|d| d >= start && d <= end)
                .unwrap_or_else(|| format!("{sy:04}-{month:02}-{day:0>2}"))
        };
        let row = basic_line(
            within(&m[1], &m[2]),
            Some(within(&m[3], &m[4])),
            desc.to_owned(),
            amount,
            after,
        );
        out.push(row);
    }
    let mut result = statement(out, opening, running.or(total_closing));
    result.account_number = account_number;
    if let Some((start, end, _, _)) = period {
        result.period_start = Some(start);
        result.period_end = Some(end);
    }
    result
}

fn parse_krungsri_savings(lines: &[String]) -> ParsedStatement {
    let txn = re(
        r"^(\d{2}/\d{2}/\d{4})\s+(\d{2}:\d{2}:\d{2})\s+(.+?)\s+([\d,]+\.\d{2})\s+([\d,]+\.\d{2})\s+([A-Z][A-Z0-9_-]*)(?:\s+(.*))?$",
    );
    let acct = re(r"\b(\d{3}-\d-\d{5}-\d)\b");
    let holder = re(r"(?i)ชื่อ\s*บัญชี\s*(.+)$");
    let period = re(r"(\d{2}/\d{2}/\d{4})\s*-\s*(\d{2}/\d{2}/\d{4})");
    let credit = re(r"(?i)รับ\s*โอน|ฝาก|ดอก|เงิน\s*เข้า");
    let debit = re(r"(?i)โอนเงินพร้อมเพย์|โอน\s*เงิน|ถอน|ชำ\s*ระ|ชา\s*ระ|จ่าย");
    let continuation = re(r"[:：]\s*(\S.*)$");
    let (mut account_number, mut holder_name, mut ps, mut pe, mut opening) =
        (None, None, None, None, None);
    let mut running: Option<i64> = None;
    let mut out = Vec::new();
    for line in lines {
        if account_number.is_none() {
            account_number = acct.captures(line).map(|m| m[1].to_owned());
        }
        if holder_name.is_none() {
            holder_name = holder.captures(line).map(|m| m[1].trim().to_owned());
        }
        if ps.is_none()
            && let Some(m) = period.captures(line)
        {
            ps = Some(iso_full_slash(&m[1]));
            pe = Some(iso_full_slash(&m[2]));
        }
        if let Some(m) = txn.captures(line) {
            let raw = money(&m[4]);
            let after = money(&m[5]);
            let (signed, _direction_source) = if let Some(old) = running {
                if (old + raw - after).abs() <= 1 {
                    (raw, "balance-delta")
                } else if (old - raw - after).abs() <= 1 || debit.is_match(&m[3]) {
                    (
                        -raw,
                        if (old - raw - after).abs() <= 1 {
                            "balance-delta"
                        } else {
                            "keyword-fallback"
                        },
                    )
                } else {
                    (raw, "balance-delta-mismatch")
                }
            } else if debit.is_match(&m[3]) {
                (-raw, "keyword")
            } else if credit.is_match(&m[3]) {
                (raw, "keyword")
            } else {
                (raw, "unknown-first-row")
            };
            if running.is_none() && (credit.is_match(&m[3]) || debit.is_match(&m[3])) {
                opening = Some(after - signed);
            }
            running = Some(after);
            let date = iso_full_slash(&m[1]);
            let desc = format!(
                "{} {}",
                m[3].trim(),
                m.get(7).map_or("", |v| v.as_str().trim())
            )
            .trim()
            .to_owned();
            let row = basic_line(date.clone(), Some(date), desc, signed, Some(after));
            out.push(row);
        } else if let (Some(last), Some(m)) = (out.last_mut(), continuation.captures(line)) {
            let clean = m[1].trim();
            last.description.push_str(" · ");
            last.description.push_str(clean);
        }
    }
    let mut result = statement(out, opening, running);
    result.account_number = account_number;
    result.holder = holder_name;
    result.period_start = ps.or(result.period_start);
    result.period_end = pe.or(result.period_end);
    result
}

#[cfg(test)]
mod tests {
    use super::{exact_minor_units, foreign_money};

    #[test]
    fn amounts_are_exact_or_rejected() {
        assert_eq!(exact_minor_units("2,500.75", 2), Some(250_075));
        assert_eq!(exact_minor_units("-0.25", 2), Some(-25));
        assert_eq!(exact_minor_units("9", 0), Some(9));
        assert_eq!(exact_minor_units("4.500", 2), Some(450), "trailing zero");
        assert_eq!(exact_minor_units("10,000,000.00", 2), Some(1_000_000_000));
        for bad in [
            "",
            ",.10",
            "2b.00",
            "-.75",
            "4.501",
            "--2.00",
            "+3.00",
            "9,87.65",
            "98,76.00",
            "9876,543.00",
            "๒๐๐.๐๐",
            "2 000.00",
            "88888888888888888888.88",
            "1,000,000,000,000.05",
        ] {
            assert_eq!(exact_minor_units(bad, 2), None, "{bad:?}");
        }
        assert_eq!(foreign_money("5,000", "JPY"), 5_000);
        assert_eq!(foreign_money("1.250", "KWD"), 1_250);
    }
}
