use super::*;
use crate::decimal::Decimal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberPredicate {
    Concepts(Box<Expr>),
    Number(Decimal),
    Text(Vec<SearchTerm>),
    Boolean(Option<bool>),
    Dates(Vec<Option<u32>>),
    /// Quoted dates also satisfy the untyped string grammar; the column decides their meaning.
    DatesOrText {
        dates: Vec<Option<u32>>,
        terms: Vec<SearchTerm>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberFilter {
    pub field: String,
    pub comparison: Comparison,
    pub value: MemberPredicate,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberQuery {
    pub source: Box<Expr>,
    pub reverse: bool,
    /// None selects referencedComponentId; an empty list selects all non-metadata fields.
    pub fields: Option<Vec<String>>,
    pub filters: Vec<MemberFilter>,
}
impl Parser<'_> {
    pub(super) fn member_fields(&mut self) -> Result<Vec<String>> {
        self.take("[");
        self.ws()?;
        let mut fields = Vec::new();
        if self.take("*") || self.keyword("any") {
            self.ws()?;
        } else {
            loop {
                let field = self.word().to_ascii_lowercase();
                if field.is_empty() || !field.bytes().all(|b| b.is_ascii_alphabetic()) {
                    return Err(self.unexpected());
                }
                if fields.contains(&field) {
                    self.refuse(self.pos, "A field is projected twice");
                }
                self.pos += field.len();
                fields.push(field);
                self.ws()?;
                if !self.take(",") {
                    break;
                }
                self.ws()?;
            }
        }
        if !self.take("]") {
            return Err(self.unexpected());
        }
        self.ws()?;
        Ok(fields)
    }
    pub(super) fn starts_member_filter(&mut self) -> Result<bool> {
        let saved = self.pos;
        let result = if self.take("{{") {
            self.ws()?;
            self.rest().starts_with(['m', 'M']) && self.filter_name() != "moduleid"
        } else {
            false
        };
        self.pos = saved;
        Ok(result)
    }
    pub(super) fn member_filters(&mut self, depth: usize) -> Result<Vec<MemberFilter>> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Filter nesting exceeds 64"));
        }
        self.take("{{");
        self.ws()?;
        if !(self.take("M") || self.take("m")) {
            return Err(self.unexpected());
        }
        self.ws()?;
        let mut result = Vec::new();
        loop {
            let field = self.filter_name();
            if field.is_empty() || !field.bytes().all(|b| b.is_ascii_alphabetic()) {
                return Err(self.unexpected());
            }
            self.pos += field.len();
            let comparison = self.comparison()?;
            let saved = self.pos;
            if self.take("(") {
                self.ws()?;
            }
            let quoted = !self.starts_alternate()
                && (self.rest().starts_with('"')
                    || self.word().eq_ignore_ascii_case("match")
                    || self.word().eq_ignore_ascii_case("wild"));
            self.pos = saved;
            // The activeFilter and effectiveTimeFilter forms come first; the generic
            // memberFieldFilter still parses other value lexemes, which evaluation types.
            let dated = field == "effectivetime" && self.rest().starts_with('"');
            let active_literal = self.rest().starts_with(['*', '"'])
                || self.word().eq_ignore_ascii_case("any")
                || matches!(self.rest().as_bytes(), [b'0' | b'1', rest @ ..]
                    if !rest.first().is_some_and(u8::is_ascii_digit));
            let value = if field == "active" && active_literal {
                MemberPredicate::Boolean(self.active_value()?)
            } else if dated
                || quoted && !matches!(comparison, Comparison::Eq | Comparison::Ne)
                || self.rest().starts_with("\"\"")
            {
                let mark = self.mark();
                let list = self.take("(");
                self.ws()?;
                let first = self.filter_date();
                if first.is_err()
                    && dated
                    && matches!(comparison, Comparison::Eq | Comparison::Ne)
                {
                    // Not a date, so the grammar reads it as a string compared with a
                    // field that happens to be named effectiveTime; evaluation types it.
                    self.reset(mark);
                    let terms = self.search_terms()?;
                    result.push(MemberFilter {
                        field,
                        comparison,
                        value: MemberPredicate::Text(terms),
                    });
                    self.ws()?;
                    if self.take("}}") {
                        return Ok(result);
                    }
                    if !self.take(",") {
                        return Err(self.unexpected());
                    }
                    self.ws()?;
                    continue;
                }
                let mut dates = vec![first?];
                loop {
                    let spaced = self.ws()?;
                    if !list || self.take(")") {
                        break;
                    }
                    if !spaced {
                        return Err(self.unexpected());
                    }
                    dates.push(self.filter_date()?);
                }
                MemberPredicate::Dates(dates)
            } else if self.take("#") {
                let start = self.pos;
                if !self.take("-") {
                    self.take("+");
                }
                let integer = self.pos;
                while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                    self.pos += 1;
                }
                if self.pos == integer
                    || self.pos - integer > 1 && self.text[integer..].starts_with('0')
                {
                    return Err(self.unexpected());
                }
                if self.take(".") {
                    while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                        self.pos += 1;
                    }
                }
                MemberPredicate::Number(
                    Decimal::parse(&self.text[start..self.pos]).ok_or_else(|| self.unexpected())?,
                )
            } else if quoted && !self.starts_alternate() {
                let start = self.pos;
                if let Ok(dates) = self.member_dates() {
                    let terms = dates
                        .iter()
                        .flatten()
                        .map(|d| SearchTerm::Match(vec![format!("{d:08}")]))
                        .collect();
                    MemberPredicate::DatesOrText { dates, terms }
                } else {
                    self.pos = start;
                    MemberPredicate::Text(self.search_terms()?)
                }
            } else if !self.starts_alternate() && self.value_keyword("true") {
                MemberPredicate::Boolean(Some(true))
            } else if !self.starts_alternate() && self.value_keyword("false") {
                MemberPredicate::Boolean(Some(false))
            } else if field == "moduleid" {
                // Only moduleFilter admits a bare set of concepts, `(a b)`.
                MemberPredicate::Concepts(Box::new(self.filter_concepts(depth + 1)?))
            } else {
                MemberPredicate::Concepts(Box::new(self.subexpression(depth + 1)?))
            };
            if !matches!(
                value,
                MemberPredicate::Number(_) | MemberPredicate::Dates(_)
            ) && !matches!(comparison, Comparison::Eq | Comparison::Ne)
            {
                return Err(self.unexpected());
            }
            result.push(MemberFilter {
                field,
                comparison,
                value,
            });
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(self.error(ParseErrorKind::Limit, "Too many member predicates"));
            }
            self.ws()?;
            if self.take("}}") {
                return Ok(result);
            }
            if !self.take(",") {
                return Err(self.unexpected());
            }
            self.ws()?;
        }
    }

    fn member_dates(&mut self) -> Result<Vec<Option<u32>>> {
        let list = self.take("(");
        self.ws()?;
        let mut result = vec![self.filter_date()?];
        loop {
            let separated = self.ws()?;
            if !list || self.take(")") {
                return Ok(result);
            }
            if !separated {
                return Err(self.unexpected());
            }
            result.push(self.filter_date()?);
        }
    }
}
