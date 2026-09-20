use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum History {
    Minimum,
    Moderate,
    Maximum,
    Subset(Box<Expr>),
}

impl Parser<'_> {
    pub(super) fn history(&mut self, depth: usize) -> Result<History> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "History nesting exceeds 64"));
        }
        self.take("{{");
        self.ws()?;
        if !self.take("+") {
            return Err(self.unexpected());
        }
        self.ws()?;
        if !self.keyword("history") {
            return Err(self.unexpected());
        }
        let result = if self.take("-") || self.take("_") {
            if self.keyword("min") {
                History::Minimum
            } else if self.keyword("mod") {
                History::Moderate
            } else if self.keyword("max") {
                History::Maximum
            } else {
                return Err(self.unexpected());
            }
        } else {
            self.ws()?;
            if self.take("(") {
                let subset = self.expression(depth + 1)?;
                self.ws()?;
                if !self.take(")") {
                    return Err(self.unexpected());
                }
                History::Subset(Box::new(subset))
            } else {
                History::Maximum
            }
        };
        self.ws()?;
        if !self.take("}}") {
            return Err(self.unexpected());
        }
        Ok(result)
    }
}
