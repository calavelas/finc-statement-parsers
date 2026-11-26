# Contributing

## Never commit real statement data

This is the one hard rule. Do not commit PDFs, extracted text, screenshots,
names, account or card numbers, amounts or descriptions from a real statement,
not even "just in a test" and not even masked. Git history is forever.

Write fixtures by hand instead: copy the *shape* of each line (column order,
date format, keywords), then fill it with invented values: names like
`SOMCHAI TESTER`, account numbers made of zeros (`000-0-00000-0`,
`0000 00XX XXXX 0000`), round amounts (`1,000.00`). Check that the synthetic
rows still reconcile to the synthetic balances.

`.gitignore` blocks `*.pdf`, `*.csv` and `.env*`; keep it that way.

## Adding a bank

1. Extract text locally: `finc-parse your.pdf --parser scb_savings` will fail,
   but `pdftotext -layout your.pdf -` or a quick `extract_pdf_lines` call shows
   the lines the parsers see (whitespace is collapsed to single spaces).
2. Add `fn parse_<bank>_<kind>(lines: &[String]) -> ParsedStatement` in
   `src/banks.rs`, register its key in `PARSER_KEYS` and `parse_with`.
3. Rules every parser follows:
   - Build regexes with `re(...)` (it makes `\d` ASCII-only, so Thai digits
     never parse as amounts).
   - Read money with `money` / `balance_money` / `foreign_money`. Never
     `parse::<f64>()`.
   - Capture the printed opening/closing balances whenever the layout has
     them, and pass them to `statement(...)` so the rows are reconciled.
   - If a row looks like a transaction but its amount cannot be read, the
     statement must fail, not drop the row.
4. Add a synthetic fixture and test in `tests/synthetic.rs` (rows, signs,
   balanced result), and add it to the auto-detection test.
5. Add an entry to `catalog/statement-recipes.json` if you know how the
   statement is delivered (subject, sender, filename). Use placeholders for any
   personal digits.
6. `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`.

## Licensing

By contributing you agree your contribution is licensed under Apache-2.0.
