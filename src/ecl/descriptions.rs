use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DescriptionFilter {
    Term(Comparison, Vec<SearchTerm>),
    Metadata(ConceptFilter),
    Language(Comparison, Vec<[u8; 2]>),
    Type(Comparison, Box<Expr>),
    Id(Comparison, Vec<u64>),
    Dialect(Comparison, Vec<Dialect>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialect {
    pub refsets: Expr,
    pub acceptability: Vec<u64>,
}

impl Parser<'_> {
    pub(super) fn description_filters(&mut self, depth: usize) -> Result<Vec<DescriptionFilter>> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Filter nesting exceeds 64"));
        }
        self.take("{{");
        self.ws()?;
        if self.rest().starts_with(['d', 'D'])
            && self.filter_name() != "dialect"
            && self.filter_name() != "dialectid"
        {
            self.pos += 1;
            self.ws()?;
        }
        let mut filters = Vec::new();
        loop {
            let name = self.filter_name();
            self.pos += name.len();
            let comparison = self.comparison()?;
            if name != "effectivetime" && !matches!(comparison, Comparison::Eq | Comparison::Ne) {
                return Err(self.unexpected());
            }
            let filter = match name.as_str() {
                "term" => DescriptionFilter::Term(comparison, self.search_terms()?),
                "active" => DescriptionFilter::Metadata(ConceptFilter::Active(
                    comparison,
                    self.active_value()?,
                )),
                "moduleid" => DescriptionFilter::Metadata(ConceptFilter::Module(
                    comparison,
                    Box::new(self.filter_concepts(depth)?),
                )),
                "effectivetime" => DescriptionFilter::Metadata(ConceptFilter::EffectiveTime(
                    comparison,
                    self.description_list(|p| p.filter_date())?,
                )),
                "language" => {
                    let values = self.description_list(|p| {
                        let word = p.word();
                        if word.len() != 2 || !word.bytes().all(|b| b.is_ascii_alphabetic()) {
                            return Err(p.unexpected());
                        }
                        let value = word.to_ascii_lowercase().as_bytes().try_into().unwrap();
                        p.pos += 2;
                        Ok(value)
                    })?;
                    DescriptionFilter::Language(comparison, values)
                }
                "id" => DescriptionFilter::Id(
                    comparison,
                    self.description_list(|p| p.description_id())?,
                ),
                "typeid" => {
                    DescriptionFilter::Type(comparison, Box::new(self.filter_concepts(depth)?))
                }
                "type" => {
                    let values = self.description_list(|p| {
                        let code = if p.keyword("syn") || p.keyword("synonym") {
                            900000000000013009
                        } else if p.keyword("fsn") || p.keyword("fullySpecifiedName") {
                            900000000000003001
                        } else if p.keyword("def") || p.keyword("definition") {
                            900000000000550004
                        } else {
                            return Err(p.unexpected());
                        };
                        p.node(Expr::Concept(code))
                    })?;
                    let expr = if values.len() == 1 {
                        values.into_iter().next().unwrap()
                    } else {
                        self.node(Expr::Or(values))?
                    };
                    DescriptionFilter::Type(comparison, Box::new(expr))
                }
                "dialectid" | "dialect" => DescriptionFilter::Dialect(
                    comparison,
                    self.description_dialects(name == "dialect", depth)?,
                ),
                _ => return Err(self.error(ParseErrorKind::Syntax, "Unknown description filter")),
            };
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(self.error(ParseErrorKind::Limit, "Too many filters"));
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

    fn description_dialects(&mut self, aliases: bool, depth: usize) -> Result<Vec<Dialect>> {
        let saved = (self.pos, self.nodes);
        let mut dialects = if aliases || self.rest().starts_with('(') {
            let list = self.description_list(|p| {
                let refsets = if aliases {
                    let alias = p.alias()?;
                    p.node(Expr::DialectAlias(alias))?
                } else {
                    let id = p.description_id()?;
                    let end = p.pos;
                    p.ws()?;
                    if p.take("|") {
                        p.term()?;
                    } else {
                        p.pos = end;
                    }
                    p.node(Expr::Concept(id))?
                };
                let end = p.pos;
                p.ws()?;
                let acceptability = if p.rest().starts_with('(') {
                    p.acceptabilities()?
                } else {
                    p.pos = end;
                    Vec::new()
                };
                Ok(Dialect {
                    refsets,
                    acceptability,
                })
            });
            match list {
                Ok(list) => list,
                Err(e) if aliases => return Err(e),
                Err(_) => {
                    (self.pos, self.nodes) = saved;
                    vec![Dialect {
                        refsets: self.subexpression(depth)?,
                        acceptability: Vec::new(),
                    }]
                }
            }
        } else {
            vec![Dialect {
                refsets: self.subexpression(depth)?,
                acceptability: Vec::new(),
            }]
        };
        self.ws()?;
        if self.rest().starts_with('(') {
            let shared = self.acceptabilities()?;
            for dialect in &mut dialects {
                if dialect.acceptability.is_empty() {
                    dialect.acceptability = shared.clone();
                }
            }
        }
        Ok(dialects)
    }
    fn description_list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        let list = self.take("(");
        self.ws()?;
        let mut values = Vec::new();
        loop {
            values.push(item(self)?);
            let separated = self.ws()?;
            if !list || self.take(")") {
                return Ok(values);
            }
            if !separated {
                return Err(self.unexpected());
            }
        }
    }
    fn description_id(&mut self) -> Result<u64> {
        let start = self.pos;
        while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
            self.pos += 1;
        }
        let digits = &self.text[start..self.pos];
        if !(6..=18).contains(&digits.len()) || digits.starts_with('0') {
            return Err(self.unexpected());
        }
        digits.parse().map_err(|_| self.unexpected())
    }
    fn acceptabilities(&mut self) -> Result<Vec<u64>> {
        self.description_list(|p| {
            if p.keyword("prefer") || p.keyword("preferred") {
                Ok(900000000000548007)
            } else if p.keyword("accept") || p.keyword("acceptable") {
                Ok(900000000000549004)
            } else {
                let code = p.description_id()?;
                let end = p.pos;
                p.ws()?;
                if p.take("|") {
                    p.term()?;
                } else {
                    p.pos = end;
                }
                Ok(code)
            }
        })
    }
}
