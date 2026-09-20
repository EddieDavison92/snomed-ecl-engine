use super::*;
use crate::decimal::Decimal;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cardinality {
    pub min: u64,
    pub max: Option<u64>,
}
impl Default for Cardinality {
    fn default() -> Self {
        Self { min: 1, max: None }
    }
}
impl Cardinality {
    pub fn contains(self, count: usize) -> bool {
        count as u64 >= self.min && self.max.is_none_or(|max| count as u64 <= max)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
impl Comparison {
    pub fn matches(self, order: Ordering) -> bool {
        match self {
            Self::Eq => order == Ordering::Equal,
            Self::Ne => order != Ordering::Equal,
            Self::Lt => order == Ordering::Less,
            Self::Le => order != Ordering::Greater,
            Self::Gt => order == Ordering::Greater,
            Self::Ge => order != Ordering::Less,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeValue {
    Concepts(Box<Expr>),
    Number(Decimal),
    Strings(Vec<String>),
    Boolean(bool),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeConstraint {
    pub cardinality: Cardinality,
    pub reverse: bool,
    pub name: Box<Expr>,
    pub comparison: Comparison,
    pub value: AttributeValue,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refinement {
    Attribute(AttributeConstraint),
    Group(Cardinality, Box<Refinement>),
    And(Vec<Refinement>),
    Or(Vec<Refinement>),
}

impl Parser<'_> {
    pub(super) fn keyword(&mut self, word: &str) -> bool {
        if self.word().eq_ignore_ascii_case(word) {
            self.pos += word.len();
            true
        } else {
            false
        }
    }
    fn natural(&mut self) -> Result<(u64, std::ops::Range<usize>)> {
        let start = self.pos;
        while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
            self.pos += 1;
        }
        let text = &self.text[start..self.pos];
        if text.is_empty() || text.len() > 1 && text.starts_with('0') {
            return Err(self.error(ParseErrorKind::Syntax, "Leading zero in cardinality"));
        }
        // Every stored row/group count fits u32. Larger bounds remain above that domain.
        Ok((text.parse().unwrap_or(u64::MAX), start..self.pos))
    }
    fn cardinality(&mut self) -> Result<Cardinality> {
        if !self.take("[") {
            return Ok(Cardinality::default());
        }
        let (min, min_text) = self.natural()?;
        if !self.take("..") {
            self.required_ws()?;
            if !self.keyword("to") {
                return Err(self.unexpected());
            }
            self.required_ws()?;
        }
        let max = if self.take("*") || self.keyword("many") {
            None
        } else {
            Some(self.natural()?)
        };
        let reversed = max.as_ref().is_some_and(|(_, max_text)| {
            let min = &self.text[min_text];
            let max = &self.text[max_text.clone()];
            min.len().cmp(&max.len()).then_with(|| min.cmp(max)).is_gt()
        });
        if !self.take("]") || reversed {
            return Err(self.error(ParseErrorKind::Syntax, "Invalid cardinality range"));
        }
        self.ws()?;
        Ok(Cardinality {
            min,
            max: max.map(|(value, _)| value),
        })
    }
    pub(super) fn comparison(&mut self) -> Result<Comparison> {
        self.ws()?;
        for (text, op) in [
            ("!=", Comparison::Ne),
            ("<>", Comparison::Ne),
            ("<=", Comparison::Le),
            (">=", Comparison::Ge),
            ("=", Comparison::Eq),
            ("<", Comparison::Lt),
            (">", Comparison::Gt),
        ] {
            if self.take(text) {
                self.ws()?;
                return Ok(op);
            }
        }
        if self.keyword("not") {
            self.ws()?;
            if self.take("=") {
                self.ws()?;
                return Ok(Comparison::Ne);
            }
        }
        Err(self.error(ParseErrorKind::Syntax, "Expected comparison operator"))
    }
    pub(super) fn quoted(&mut self) -> Result<String> {
        if !self.take("\"") {
            return Err(self.unexpected());
        }
        let mut value = String::new();
        loop {
            let Some(c) = self.rest().chars().next() else {
                return Err(self.error(ParseErrorKind::Syntax, "Unclosed string"));
            };
            self.pos += c.len_utf8();
            if c == '"' {
                return Ok(value);
            }
            if c == '\\' {
                let Some(escaped) = self.rest().chars().next() else {
                    return Err(self.unexpected());
                };
                // The brief 2.3 grammar also permits an escaped literal asterisk.
                if !matches!(escaped, '\\' | '"' | '*') {
                    return Err(self.error(ParseErrorKind::Syntax, "Invalid string escape"));
                }
                self.pos += escaped.len_utf8();
                value.push(escaped);
            } else if c.is_ascii_control() && !matches!(c, '\r' | '\n' | '\t') {
                return Err(self.unexpected());
            } else {
                value.push(c);
            }
        }
    }
    fn attribute(&mut self, depth: usize, cardinality: Cardinality) -> Result<Refinement> {
        let reverse = !self.starts_alternate() && (self.keyword("reverseof") || self.take("R"));
        self.ws()?;
        let name = Box::new(self.subexpression(depth + 1)?);
        let comparison = self.comparison()?;
        let value = if self.take("#") {
            let start = self.pos;
            self.take("-");
            self.take("+");
            let integer_start = self.pos;
            while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos - integer_start > 1 && self.text[integer_start..].starts_with('0') {
                return Err(self.unexpected());
            }
            if self.take(".") {
                while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            let number = Decimal::parse(&self.text[start..self.pos])
                .ok_or_else(|| self.error(ParseErrorKind::Syntax, "Invalid decimal"))?;
            AttributeValue::Number(number)
        } else if self.rest().starts_with('"') && !self.starts_alternate() {
            let value = self.quoted()?;
            if value.is_empty() {
                return Err(self.error(ParseErrorKind::Syntax, "Empty concrete string"));
            }
            AttributeValue::Strings(vec![value])
        } else if !self.starts_alternate() && self.keyword("true") {
            AttributeValue::Boolean(true)
        } else if !self.starts_alternate() && self.keyword("false") {
            AttributeValue::Boolean(false)
        } else {
            let saved = self.pos;
            if self.take("(") {
                self.ws()?;
            }
            if self.pos != saved && self.rest().starts_with('"') && !self.starts_alternate() {
                let mut values = vec![self.quoted()?];
                loop {
                    let spaced = self.ws()?;
                    if self.take(")") {
                        break;
                    }
                    if !spaced {
                        return Err(self.unexpected());
                    }
                    values.push(self.quoted()?);
                }
                if values.iter().any(String::is_empty) {
                    return Err(self.unexpected());
                }
                AttributeValue::Strings(values)
            } else {
                self.pos = saved;
                AttributeValue::Concepts(Box::new(self.subexpression(depth + 1)?))
            }
        };
        if !matches!(value, AttributeValue::Number(_))
            && !matches!(comparison, Comparison::Eq | Comparison::Ne)
        {
            return Err(self.error(
                ParseErrorKind::Syntax,
                "Ordered comparison requires a numeric attribute value",
            ));
        }
        if reverse && !matches!(value, AttributeValue::Concepts(_)) {
            return Err(self.error(ParseErrorKind::Syntax, "Concrete values cannot be reversed"));
        }
        Ok(Refinement::Attribute(AttributeConstraint {
            cardinality,
            reverse,
            name,
            comparison,
            value,
        }))
    }
    fn subrefinement(&mut self, depth: usize, groups: bool) -> Result<Refinement> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Refinement nesting exceeds 64"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(self.error(ParseErrorKind::Limit, "Too many nodes"));
        }
        self.ws()?;
        let has_cardinality = self.rest().starts_with('[');
        let cardinality = self.cardinality()?;
        if self.take("{") {
            if !groups {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "Nested relationship groups are not allowed",
                ));
            }
            let inner = self.refinement(depth + 1, false)?;
            self.ws()?;
            if !self.take("}") {
                return Err(self.unexpected());
            }
            return Ok(Refinement::Group(cardinality, Box::new(inner)));
        }
        let (saved, nodes) = (self.pos, self.nodes);
        let attribute = self.attribute(depth, cardinality);
        if attribute.is_ok() || !self.text[saved..].starts_with('(') {
            return attribute;
        }
        self.pos = saved;
        self.nodes = nodes;
        if has_cardinality {
            return Err(self.unexpected());
        }
        self.take("(");
        let inner = self.refinement(depth + 1, groups)?;
        self.ws()?;
        if !self.take(")") {
            return Err(self.unexpected());
        }
        Ok(inner)
    }
    pub(super) fn refinement(&mut self, depth: usize, groups: bool) -> Result<Refinement> {
        let first = self.subrefinement(depth, groups)?;
        let Some(operator) = self.boolean()? else {
            return Ok(first);
        };
        if operator == Boolean::Minus {
            return Err(self.error(
                ParseErrorKind::Syntax,
                "Exclusion is not a refinement operator",
            ));
        }
        let mut parts = vec![first, self.subrefinement(depth, groups)?];
        while let Some(next) = self.boolean()? {
            if next != operator {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "Mixed refinement operators require parentheses",
                ));
            }
            parts.push(self.subrefinement(depth, groups)?);
        }
        Ok(if operator == Boolean::And {
            Refinement::And(parts)
        } else {
            Refinement::Or(parts)
        })
    }
}
