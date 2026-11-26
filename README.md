# finc-statement-parsers

Exact-decimal parsers for Thai bank and credit card PDF statements, plus a
catalog of how those statements arrive by email. Extracted from
[FINC](https://finc.money) so anyone can add a bank.

- Native Rust PDF text extraction (`pdf-extract`, no external binaries).
- One line parser per statement layout, producing one universal JSON format.
- Money is exact: amounts are read as integer minor units and emitted as
  `rust_decimal` strings. No floating point anywhere.
- **A parse is not a success just because text came out of the PDF.** When the
  statement prints balance anchors, the rows must reconcile to them (±0.01);
  any amount that cannot be read exactly (Thai numerals, `9,87.65`, `1,000.005`)
  fails the whole statement instead of being skipped or guessed.

## Supported statements

| Parser key          | Bank / product                 | Reconciles against                      |
|---------------------|--------------------------------|-----------------------------------------|
| `bbl_savings`       | Bangkok Bank savings           | B/F + running balance                   |
| `scb_savings`       | SCB savings / current          | brought-forward + running balance       |
| `gsb_savings`       | GSB savings                    | ยอดยกมา / ยอดยกไป + running balance      |
| `kbank_savings`     | KBank savings                  | beginning balance + running balance     |
| `kbank_card`        | KBank credit cards (multi-card)| previous / total balance per card       |
| `kkp_savings`       | KKP (incl. Dime Save)          | balance forward + running balance       |
| `krungsri_savings`  | Krungsri savings               | running balance (opening inferred)      |
| `krungsri_card`     | Krungsri credit card           | previous / total payment due            |
| `ktb_savings`       | Krungthai savings              | opening anchor + running balance        |
| `ktc_card`          | KTC credit card                | previous / amount due                   |
| `ttb_savings`       | ttb savings                    | running balance (opening inferred)      |
| `uob_savings`       | UOB savings (incl. TMRW)       | B/F + running balance                   |
| `uob_card`          | UOB credit card                | previous / total balance                |
| `cardx_card`        | CardX credit card              | previous / total balance                |

Layouts were written against real statements in FINC v1 but are tested here
only with synthetic text, so treat every parser as "known to work for the
layout it was written for". Banks change layouts; PRs welcome.

## CLI

```sh
cargo install --path .
STMT_PW='…' finc-parse statement.pdf --password-env STMT_PW
finc-parse statement.pdf --parser kbank_card --card platinum
pdftotext -layout statement.pdf - > statement.txt && finc-parse statement.txt
```

- The password is only ever read from the environment variable you name; there
  is deliberately no `--password` flag (argv shows up in `ps` and shell history).
- Without `--parser`, every parser is tried and the best balanced result wins.
  Pass `--parser` when you know the bank.
- A file that does not start with `%PDF` is treated as already-extracted text.
  Some layouts (notably KBank cards) parse best from `pdftotext -layout` output.
- Exit status is non-zero, with no JSON, when the statement fails to parse or
  reconcile.

## Universal format (`finc-statement/v1`)

```json
{
  "format": "finc-statement/v1",
  "parser": "scb_savings",
  "currency": "THB",
  "account_number": null,
  "holder": null,
  "period_start": "2026-06-01",
  "period_end": "2026-06-30",
  "observed_start": "2026-06-01",
  "observed_end": "2026-06-02",
  "opening_balance": "2000.00",
  "closing_balance": "2100.00",
  "balance_check": "passed",
  "rows": [
    {
      "transaction_date": "2026-06-01",
      "posted_date": "2026-06-01",
      "description": "Test Shop",
      "amount": "-200.00",
      "currency": "THB",
      "balance_after": "1800.00",
      "foreign_amount": null,
      "foreign_currency": null
    }
  ]
}
```

- **Amounts** are decimal strings, never JSON numbers.
- **Sign:** positive = money into the account (deposit, card payment, refund);
  negative = money out (withdrawal, card spend). Card balances are therefore
  negative when you owe money.
- `period_*` is the coverage the statement itself declares; `observed_*` is
  just the first/last row date and must never be treated as coverage.
- `balance_check`: `passed` (rows reconcile to printed anchors), `unverified`
  (the layout prints no usable anchors), or `declared_empty` (the statement
  explicitly says there was no activity).
- `foreign_amount` uses the currency's own minor unit (JPY has none, KWD has 3).
- `account_number` / `holder` are copied from the document as printed (usually
  masked) so callers can route multi-account statements. Parsing is local; this
  crate makes no network calls.

## Email catalog

[`catalog/statement-recipes.json`](catalog/statement-recipes.json) records how
each statement arrives: institution, statement kind, parser key, sender,
subject hint, attachment filename pattern and password format (none are
confirmed yet, so all are `null`). Subjects that contain account digits use
placeholders like `<last four>`.

## Adding a bank

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Privacy

See [PRIVACY.md](PRIVACY.md). Never commit real statements, even masked.

## License

Apache-2.0. See [LICENSE](LICENSE).
