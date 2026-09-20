use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchTerm {
    Match(Vec<String>),
    /// None is an unescaped wildcard; Some is literal text.
    Wild(Vec<Option<String>>),
}

impl Parser<'_> {
    pub(super) fn search_terms(&mut self) -> Result<Vec<SearchTerm>> {
        let list = self.take("(");
        self.ws()?;
        let mut terms = Vec::new();
        loop {
            let wild = self.keyword("wild");
            let typed = wild || self.keyword("match");
            if typed {
                self.ws()?;
                if !self.take(":") {
                    return Err(self.unexpected());
                }
                self.ws()?;
            }
            if !self.take("\"") {
                return Err(self.unexpected());
            }
            let mut text = String::new();
            let mut parts = Vec::new();
            loop {
                let Some(c) = self.rest().chars().next() else {
                    return Err(self.error(ParseErrorKind::Syntax, "Unclosed search term"));
                };
                self.pos += c.len_utf8();
                if c == '"' {
                    break;
                }
                if c == '\\' {
                    let Some(escaped) = self.rest().chars().next() else {
                        return Err(self.unexpected());
                    };
                    if !(matches!(escaped, '\\' | '"') || wild && escaped == '*') {
                        return Err(
                            self.error(ParseErrorKind::Syntax, "Invalid search term escape")
                        );
                    }
                    self.pos += escaped.len_utf8();
                    text.push(escaped);
                } else if wild && c == '*' {
                    if !text.is_empty() {
                        parts.push(Some(std::mem::take(&mut text)));
                    }
                    if parts.last() != Some(&None) {
                        parts.push(None);
                    }
                } else if c.is_ascii_control() && !matches!(c, '\t' | '\r' | '\n') {
                    return Err(self.unexpected());
                } else {
                    text.push(c);
                }
            }
            let term = if wild {
                if !text.is_empty() {
                    parts.push(Some(text));
                }
                if parts.is_empty() {
                    return Err(self.error(ParseErrorKind::Syntax, "Empty search term"));
                }
                SearchTerm::Wild(parts)
            } else {
                let words: Vec<_> = text.split_ascii_whitespace().map(str::to_owned).collect();
                if words.is_empty() {
                    return Err(self.error(ParseErrorKind::Syntax, "Empty search term"));
                }
                SearchTerm::Match(words)
            };
            self.nodes += 1 + match &term {
                SearchTerm::Match(words) => words.len(),
                SearchTerm::Wild(parts) => parts.len(),
            };
            if self.nodes > MAX_NODES {
                return Err(self.error(ParseErrorKind::Limit, "Too many search terms"));
            }
            terms.push(term);
            let spaced = self.ws()?;
            if !list || self.take(")") {
                return Ok(terms);
            }
            if !spaced {
                return Err(self.unexpected());
            }
        }
    }
}
