use super::{Comparison, Expr, ParseErrorKind, Parser, Result, MAX_DEPTH, MAX_NODES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConceptFilter {
    Active(Comparison, Option<bool>),
    Defined(Comparison, Vec<bool>),
    DefinitionStatus(Comparison, Box<Expr>),
    Module(Comparison, Box<Expr>),
    EffectiveTime(Comparison, Vec<Option<u32>>),
}

impl Parser<'_> {
    pub(super) fn concept_filters(&mut self, depth: usize) -> Result<Vec<ConceptFilter>> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Filter nesting exceeds 64"));
        }
        self.take("{{");
        self.ws()?;
        if !(self.take("C") || self.take("c")) {
            return Err(self.unexpected());
        }
        self.ws()?;
        let mut filters = Vec::new();
        loop {
            let name = self.word().to_ascii_lowercase();
            self.pos += name.len();
            let comparison = self.comparison()?;
            if name != "effectivetime" && !matches!(comparison, Comparison::Eq | Comparison::Ne) {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "This filter requires equality or inequality",
                ));
            }
            let filter = match name.as_str() {
                "active" => {
                    let value = if self.take("*") || self.keyword("any") {
                        None
                    } else if self.take("1") || self.keyword("true") {
                        Some(true)
                    } else if self.take("0") || self.keyword("false") {
                        Some(false)
                    } else {
                        return Err(self.unexpected());
                    };
                    ConceptFilter::Active(comparison, value)
                }
                "definitionstatus" => {
                    let set = self.take("(");
                    self.ws()?;
                    let mut values = Vec::new();
                    loop {
                        values.push(if self.keyword("primitive") {
                            false
                        } else if self.keyword("defined") {
                            true
                        } else {
                            return Err(self.unexpected());
                        });
                        let separated = self.ws()?;
                        if !set || self.take(")") {
                            break;
                        }
                        if !separated {
                            return Err(self.unexpected());
                        }
                    }
                    ConceptFilter::Defined(comparison, values)
                }
                "definitionstatusid" => ConceptFilter::DefinitionStatus(
                    comparison,
                    Box::new(self.filter_concepts(depth)?),
                ),
                "moduleid" => {
                    ConceptFilter::Module(comparison, Box::new(self.filter_concepts(depth)?))
                }
                "effectivetime" => {
                    let set = self.take("(");
                    self.ws()?;
                    let mut values = Vec::new();
                    loop {
                        values.push(self.filter_date()?);
                        let separated = self.ws()?;
                        if !set || self.take(")") {
                            break;
                        }
                        if !separated {
                            return Err(self.unexpected());
                        }
                    }
                    ConceptFilter::EffectiveTime(comparison, values)
                }
                _ => return Err(self.error(ParseErrorKind::Syntax, "Unknown concept filter")),
            };
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(self.error(ParseErrorKind::Limit, "Too many filter nodes"));
            }
            filters.push(filter);
            self.ws()?;
            if self.take("}}") {
                return Ok(filters);
            }
            if !self.take(",") {
                return Err(self.unexpected());
            }
            self.ws()?;
        }
    }

    pub(super) fn filter_concepts(&mut self, depth: usize) -> Result<Expr> {
        let saved = (self.pos, self.nodes);
        if self.take("(") {
            self.ws()?;
            let mut values = Vec::new();
            loop {
                let start = self.pos;
                while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                    self.pos += 1;
                }
                let digits = &self.text[start..self.pos];
                if !(6..=18).contains(&digits.len()) || digits.starts_with('0') {
                    break;
                }
                let code = digits.parse().map_err(|_| self.unexpected())?;
                values.push(self.node(Expr::Concept(code))?);
                let mut separated = self.ws()?;
                if self.take("|") {
                    self.term()?;
                    separated = self.ws()?;
                }
                if self.take(")") {
                    return if values.len() == 1 {
                        Ok(values.remove(0))
                    } else {
                        self.node(Expr::Or(values))
                    };
                }
                if !separated {
                    break;
                }
            }
        }
        (self.pos, self.nodes) = saved;
        self.subexpression(depth)
    }

    pub(super) fn filter_date(&mut self) -> Result<Option<u32>> {
        if !self.take("\"") {
            return Err(self.unexpected());
        }
        if self.take("\"") {
            return Ok(None);
        }
        let start = self.pos;
        while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
            self.pos += 1;
        }
        let text = &self.text[start..self.pos];
        if text.len() != 8 || text.starts_with('0') || !self.take("\"") {
            return Err(self.error(
                ParseErrorKind::Syntax,
                "Expected YYYYMMDD or an empty effective time",
            ));
        }
        let value: u32 = text.parse().map_err(|_| self.unexpected())?;
        let year = value / 10000;
        let month = value / 100 % 100;
        let day = value % 100;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if year.is_multiple_of(400)
                || year.is_multiple_of(4) && !year.is_multiple_of(100) =>
            {
                29
            }
            2 => 28,
            _ => 0,
        };
        if day == 0 || day > days {
            return Err(self.error(ParseErrorKind::Syntax, "Invalid effective time date"));
        }
        Ok(Some(value))
    }
}
