//! `finc-parse <file> [--password-env VAR] [--parser KEY] [--card SELECTOR]`
//!
//! Prints the universal statement JSON to stdout. `<file>` is a PDF, or text
//! already extracted from one (anything not starting with `%PDF`). The PDF
//! password is only ever read from the named environment variable.

use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("finc-parse: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    const USAGE: &str =
        "usage: finc-parse <file> [--password-env VAR] [--parser KEY] [--card SELECTOR]";
    let mut args = std::env::args().skip(1);
    let (mut file, mut password, mut parser, mut card) = (None, String::new(), None, String::new());
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(USAGE);
        match arg.as_str() {
            "--password-env" => {
                let var = value()?;
                password = std::env::var(&var).map_err(|_| format!("env var {var} is not set"))?;
            }
            "--parser" => parser = Some(value()?),
            "--card" => card = value()?,
            "-h" | "--help" => {
                return Err(format!(
                    "{USAGE}\nparsers: {}",
                    finc_statement_parsers::PARSER_KEYS.join(", ")
                ));
            }
            _ if file.is_none() && !arg.starts_with("--") => file = Some(arg),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let path = file.ok_or(USAGE)?;
    let bytes = std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    let lines = if bytes.starts_with(b"%PDF") {
        finc_statement_parsers::extract_pdf_lines(&bytes, &password)
    } else {
        finc_statement_parsers::text_lines(&String::from_utf8_lossy(&bytes))
    }
    .map_err(|error| error.to_string())?;
    let statement = finc_statement_parsers::parse_lines(parser.as_deref(), &lines, &card)
        .map_err(|error| error.to_string())?;
    serde_json::to_string_pretty(&statement).map_err(|error| error.to_string())
}
