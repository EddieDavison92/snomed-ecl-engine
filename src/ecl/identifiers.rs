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
        rest.bytes()
            .find(|b| !(b.is_ascii_alphanumeric() || *b == b'-'))
            == Some(b'#')
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
            }
            self.pos += c.len_utf8();
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
