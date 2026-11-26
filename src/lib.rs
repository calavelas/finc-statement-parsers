//! Exact-decimal parsers for Thai bank and credit card PDF statements.
//!
//! [`parse_pdf`] extracts text natively (no external binary) and runs a
//! per-bank line parser; [`parse_lines`] skips extraction for callers that
//! already have text (for example `pdftotext -layout` output). Both return a
//! [`Statement`] in the documented universal format, or an error.
//!
//! A parse is **not** a success just because text extraction worked: when the
//! statement prints balance anchors, the rows must reconcile to them, and a
//! statement with no rows must say so explicitly.

mod banks;

use std::panic::AssertUnwindSafe;

use rust_decimal::Decimal;
use serde::Serialize;

pub use banks::PARSER_KEYS;

const MAX_PDF_PAGES: usize = 200;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_LINES: usize = 50_000;
/// Largest PDF the library accepts.
pub const MAX_PDF_BYTES: usize = 12 * 1024 * 1024;

/// Identifier written to [`Statement::format`].
pub const FORMAT: &str = "finc-statement/v1";

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("PDF password is missing or incorrect")]
    Password,
    #[error("PDF is larger than {MAX_PDF_BYTES} bytes")]
    TooLarge,
    #[error("PDF has too many pages")]
    TooManyPages,
    #[error("PDF text is too large")]
    TextTooLarge,
    #[error("PDF parser failed: {0}")]
    Pdf(String),
    #[error("unknown parser '{0}'")]
    UnknownParser(String),
    #[error("statement amount is malformed or out of range")]
    MalformedAmount,
    #[error("{0}")]
    Format(String),
    #[error("rows do not reconcile to the statement's printed balances")]
    Unbalanced,
    #[error("no transactions found and the statement does not declare an empty period")]
    NoTransactions,
    #[error("no parser recognised this statement; pass an explicit parser key")]
    Unrecognised,
}

/// Outcome of the balance reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BalanceCheck {
    /// Rows reconcile exactly (±0.01) to the printed opening/closing or
    /// running balances.
    Passed,
    /// The statement prints no usable anchors; rows could not be verified.
    Unverified,
    /// The statement explicitly declares no activity for the period.
    DeclaredEmpty,
}

/// One statement in the universal format. See README for the contract.
#[derive(Clone, Debug, Serialize)]
pub struct Statement {
    pub format: &'static str,
    pub parser: String,
    /// Statement currency; every supported bank issues THB statements.
    pub currency: &'static str,
    /// As printed on the document (often masked). Read locally, never sent anywhere.
    pub account_number: Option<String>,
    pub holder: Option<String>,
    /// Coverage declared by the statement (ISO dates), when it prints one.
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    /// Earliest and latest row dates. Never a substitute for `period_*`.
    pub observed_start: Option<String>,
    pub observed_end: Option<String>,
    pub opening_balance: Option<Decimal>,
    pub closing_balance: Option<Decimal>,
    pub balance_check: BalanceCheck,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    /// Date the transaction happened (economic date).
    pub transaction_date: String,
    /// Date the bank posted it; equals `transaction_date` when only one is printed.
    pub posted_date: String,
    pub description: String,
    /// Signed: positive = money into the account (deposit, card payment or
    /// refund), negative = money out (withdrawal, card spend).
    pub amount: Decimal,
    pub currency: &'static str,
    /// Running balance printed on the row, if any (same sign convention).
    pub balance_after: Option<Decimal>,
    /// Original-currency amount for foreign card spend, unsigned as printed.
    pub foreign_amount: Option<Decimal>,
    pub foreign_currency: Option<String>,
}

/// Extract normalised, non-empty text lines from a PDF. The password is used
/// only in memory and never appears in errors.
pub fn extract_pdf_lines(bytes: &[u8], password: &str) -> Result<Vec<String>, ParseError> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err(ParseError::TooLarge);
    }
    // ponytail: no wall-clock timeout; v1 ran this in a killable worker
    // process. Wrap the call in one if you parse untrusted PDFs on a server.
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        let pages = if password.is_empty() {
            pdf_extract::extract_text_from_mem_by_pages(bytes)
        } else {
            pdf_extract::extract_text_from_mem_by_pages_encrypted(bytes, password)
        }
        .map_err(|error| classify_pdf_error(&error.to_string()))?;
        if pages.len() > MAX_PDF_PAGES {
            return Err(ParseError::TooManyPages);
        }
        if pages.iter().map(String::len).sum::<usize>() > MAX_TEXT_BYTES {
            return Err(ParseError::TextTooLarge);
        }
        text_lines(&pages.concat())
    }))
    .unwrap_or_else(|_| Err(ParseError::Pdf("malformed PDF object".to_owned())))
}

/// Split already-extracted text into the normalised lines the parsers expect.
pub fn text_lines(text: &str) -> Result<Vec<String>, ParseError> {
    let lines = text
        .lines()
        .map(banks::normalize_space)
        .filter(|line| !line.is_empty())
        .take(MAX_TEXT_LINES + 1)
        .collect::<Vec<_>>();
    if lines.len() > MAX_TEXT_LINES {
        Err(ParseError::TextTooLarge)
    } else {
        Ok(lines)
    }
}

fn classify_pdf_error(message: &str) -> ParseError {
    let lower = message.to_lowercase();
    if lower.contains("password") || lower.contains("decrypt") || lower.contains("encrypted") {
        ParseError::Password
    } else {
        ParseError::Pdf(message.chars().take(300).collect())
    }
}

/// Extract and parse a PDF. See [`parse_lines`] for `parser` and `card`.
pub fn parse_pdf(
    bytes: &[u8],
    password: &str,
    parser: Option<&str>,
    card: &str,
) -> Result<Statement, ParseError> {
    parse_lines(parser, &extract_pdf_lines(bytes, password)?, card)
}

/// Parse extracted lines with one parser key, or auto-detect when `parser` is
/// `None` (the best balanced result wins; ties go to [`PARSER_KEYS`] order).
/// `card` selects one card on multi-card KBank statements (label words such
/// as `"platinum"`); empty means every card.
pub fn parse_lines(
    parser: Option<&str>,
    lines: &[String],
    card: &str,
) -> Result<Statement, ParseError> {
    let Some(key) = parser else {
        return PARSER_KEYS
            .iter()
            .filter_map(|key| parse_lines(Some(key), lines, card).ok())
            .filter(|statement| !statement.rows.is_empty())
            .rev()
            .max_by_key(|s| (s.balance_check == BalanceCheck::Passed, s.rows.len()))
            .ok_or(ParseError::Unrecognised);
    };
    let parsed = banks::parse_lines(key, lines, card)?;
    let balance_check = match parsed.verification_ok {
        Some(false) => return Err(ParseError::Unbalanced),
        _ if parsed.lines.is_empty() && !parsed.declared_empty => {
            return Err(ParseError::NoTransactions);
        }
        _ if parsed.lines.is_empty() => BalanceCheck::DeclaredEmpty,
        Some(true) => BalanceCheck::Passed,
        None => BalanceCheck::Unverified,
    };
    let thb = |minor: i64| Decimal::new(minor, 2);
    Ok(Statement {
        format: FORMAT,
        parser: key.to_owned(),
        currency: "THB",
        account_number: parsed.account_number,
        holder: parsed.holder,
        period_start: parsed.period_start,
        period_end: parsed.period_end,
        observed_start: parsed.observed_start,
        observed_end: parsed.observed_end,
        opening_balance: parsed.opening_balance_satang.map(thb),
        closing_balance: parsed.closing_balance_satang.map(thb),
        balance_check,
        rows: parsed
            .lines
            .into_iter()
            .map(|line| Row {
                posted_date: line.value_date.unwrap_or_else(|| line.txn_date.clone()),
                transaction_date: line.txn_date,
                description: line.description,
                amount: thb(line.amount_satang),
                currency: "THB",
                balance_after: line.balance_after_satang.map(thb),
                foreign_amount: line
                    .foreign_currency
                    .as_deref()
                    .zip(line.foreign_amount_satang)
                    .map(|(code, minor)| {
                        Decimal::new(minor, u32::from(banks::currency_exponent(code)))
                    }),
                foreign_currency: line.foreign_currency,
            })
            .collect(),
    })
}
