# Privacy

This crate parses personal financial documents, so the repository holds itself
to two rules:

1. **No real statement data, ever.** Every fixture is hand-written and
   synthetic: invented names (`SOMCHAI TESTER`), account and card numbers made
   of zeros, round amounts. Nothing was copied from a real statement or from
   the fixtures of the private codebase these parsers came from.
2. **Parsing is local.** The library and CLI make no network calls. PDF
   passwords are read only from an environment variable named on the command
   line, used in memory, and never printed.

## Pre-publication scan (initial import)

Run over every tracked file and every commit before the repository was made
public:

- Digit runs of 4+ characters: only synthetic round amounts, dates, crate
  versions and pinned action SHAs.
- Account and card shapes (`ddd-d-ddddd-d`, `dddd ddXX XXXX dddd`, masked
  `xx`-prefixed digits): only all-zero synthetic values.
- Email addresses: only the public bank sender in `catalog/`.
- Personal-name patterns (English titles, Thai นาย/นาง/น.ส.): only the
  synthetic `SOMCHAI TESTER` and the parser's own regex.
- The original project's environment files and owner identifiers, its email
  catalog's account suffixes, card product names and storage IDs, and every
  string literal from its parser tests: no matches.

If you find anything that looks real, open a private security advisory rather
than a public issue.
