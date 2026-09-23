//! The parser answers every string with an expression or an error, never a
//! panic: a server parses whatever a caller sends.
use snomed_ecl_engine::ecl::parse;

const PIECES: &[&str] = &[
    "<<", "<", ">", "!", "^", "^R", "R", "#", ".", ":", ",", "(", ")", "[", "]", "{", "}", "{{",
    "}}", "|", "\"", "\\", "*", "=", "!=", "..", "/*", "*/", " ", "\n", "and", "or", "minus",
    "not", "any", "memberOf", "reverseOf", "x", "é", "𝔸", "\u{f684}", "404684003", "0", "7", "D",
    "C", "M", "+", "history", "-", "_", "term", "wild:", "id", "active", "effectiveTime",
];

#[test]
fn random_text_never_panics_the_parser() {
    let mut state = 0x9e3779b97f4a7c15u64;
    let mut next = |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    for _ in 0..200_000 {
        let length = 1 + next(24);
        let text: String = (0..length).map(|_| PIECES[next(PIECES.len())]).collect();
        let outcome = std::panic::catch_unwind(|| parse(&text));
        assert!(outcome.is_ok(), "parser panicked on {text:?}");
    }
}
