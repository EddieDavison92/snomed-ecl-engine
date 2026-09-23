//! Parses one expression per stdin line, given as a JSON byte array, and
//! prints how the parser ruled on it: `ok`, or the error kind, offset and
//! message separated by tabs. Used by `scripts/grammar_differential.py`.
use snomed_ecl_engine::ecl::parse;
use std::io::{self, BufRead, Write};

fn main() -> io::Result<()> {
    std::panic::set_hook(Box::new(|_| {}));
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let bytes: Vec<u8> = serde_json::from_str::<Vec<u8>>(&line?).expect("a JSON byte array");
        let verdict = match std::str::from_utf8(&bytes) {
            // The grammar is over UTF-8; bytes that are not UTF-8 cannot be an expression.
            Err(_) => "Encoding".to_string(),
            // A panic is a parser bug; report it as a verdict of its own.
            Ok(text) => match std::panic::catch_unwind(|| parse(text)) {
                Ok(Ok(_)) => "ok".to_string(),
                Ok(Err(error)) => format!("{:?}	{}	{}", error.kind, error.offset, error.message),
                Err(_) => "Panic	0	parser panicked".to_string(),
            },
        };
        writeln!(out, "{verdict}")?;
    }
    Ok(())
}
