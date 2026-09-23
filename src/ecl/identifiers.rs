use super::*;

impl Parser<'_> {
    pub(super) fn alias(&mut self) -> Result<String> {
        let start = self.pos;
        while self
            .rest()
            .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '-')
        {
            self.pos += 1;
        }
        let name = &self.text[start..self.pos];
        if !crate::config::valid_alias(name) {
            return Err(self.unexpected());
        }
        Ok(name.to_ascii_lowercase())
    }
    pub(super) fn starts_alternate(&self) -> bool {
        let rest = self.rest().strip_prefix('"').unwrap_or(self.rest());
        // A scheme alias starts with a letter, so `"#5"` is a string, not an identifier.
        rest.starts_with(|c: char| c.is_ascii_alphabetic())
            && rest
                .bytes()
                .find(|b| !(b.is_ascii_alphanumeric() || *b == b'-'))
                == Some(b'#')
    }
    /// Gives back an operator that an unquoted code swallowed, as in `x#a-or *`,
    /// where the grammar reads code `a-` and then `or`. Only when what follows
    /// the whole code could not continue an expression, so `x#color` stays.
    fn release_operator(&mut self, start: usize) {
        let mark = self.mark();
        let continues = self.ws().is_ok() && {
            let rest = self.rest();
            let word = self.word().to_ascii_lowercase();
            rest.is_empty()
                || rest.starts_with([')', ':', '.', ',', '|', '{', '}'])
                || ["and", "or", "minus"].contains(&word.as_str())
        };
        let end = mark.pos;
        self.reset(mark);
        if continues {
            return;
        }
        let code = self.text[start..end].to_ascii_lowercase();
        for operator in ["minus", "and", "or"] {
            if code.len() > operator.len() && code.ends_with(operator) {
                let cut = end - operator.len();
                // The operator needs whitespace or a comment after it.
                if self.text[end..].starts_with([' ', '\t', '\r', '\n']) || self.text[end..].starts_with("/*") {
                    self.pos = cut;
                }
                return;
            }
        }
    }
    /// Whether an attribute name parses after the dot at the current position.
    fn dot_starts_attribute(&mut self) -> bool {
        let mark = self.mark();
        self.pos += 1;
        let parsed = self.ws().is_ok() && self.subexpression(0).is_ok();
        self.reset(mark);
        parsed
    }
    pub(super) fn alternate_identifier(&mut self) -> Result<Expr> {
        let whole = self.take("\"");
        let scheme = self.alias()?;
        if !self.take("#") {
            return Err(self.unexpected());
        }
        // Accept both the normative whole-identifier quotes and the code-only form in the examples.
        let quoted = whole || self.take("\"");
        let start = self.pos;
        while let Some(c) = self.rest().chars().next() {
            if quoted {
                if c == '"' {
                    break;
                }
                if c == '\\' || c.is_ascii_control() && !matches!(c, '\t' | '\r' | '\n') {
                    return Err(self.unexpected());
                }
            } else if !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')) {
                break;
            } else if c == '.' && self.pos > start && self.dot_starts_attribute() {
                // A dot is part of the code unless an attribute name follows it,
                // in which case it is the dot operator: `x#a.b.<< 1234567`.
                break;
            }
            self.pos += c.len_utf8();
        }
        if !quoted {
            self.release_operator(start);
        }
        let code = self.text[start..self.pos].to_owned();
        if code.is_empty() || quoted && !self.take("\"") {
            return Err(self.unexpected());
        }
        self.ws()?;
        if self.take("|") {
            self.term()?;
        }
        self.node(Expr::AlternateIdentifier { scheme, code })
    }
}
