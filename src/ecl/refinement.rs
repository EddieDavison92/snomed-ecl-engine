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
    /// A value keyword, which the grammar lets run straight into a following
    /// operator: `true or` may be written `trueor`, `ANY MINUS` `ANYMINUS`, and
    /// an attribute named `ANY NOT = x` `ANYNOT = x`.
    pub(super) fn value_keyword(&mut self, word: &str) -> bool {
        let found = self.word();
        let rest = found.get(word.len()..).unwrap_or("");
        let joined = found.len() > word.len()
            && found[..word.len()].eq_ignore_ascii_case(word)
            && (["and", "or", "minus"].iter().any(|op| rest.eq_ignore_ascii_case(op))
                || rest.eq_ignore_ascii_case("not")
                    && skip_space(&self.rest()[found.len()..]).starts_with('='));
        if found.eq_ignore_ascii_case(word) || joined {
            self.pos += word.len();
            true
        } else {
            false
        }
    }
    /// A filter keyword or member field name, lowercased. The long syntax's
    /// `not =` may follow a keyword without a space, as in `idNOT =`, so a
    /// known keyword ending in `not` before `=` is read without it.
    pub(super) fn filter_name(&self) -> String {
        const KNOWN: &[&str] = &[
            "term", "language", "type", "typeid", "dialect", "dialectid", "id", "moduleid",
            "effectivetime", "active", "definitionstatus", "definitionstatusid",
        ];
        let name = self.word().to_ascii_lowercase();
        if let Some(keyword) = name.strip_suffix("not") {
            let after = skip_space(&self.rest()[name.len()..]);
            if KNOWN.contains(&keyword) && after.starts_with('=') {
                return keyword.to_owned();
            }
        }
        name
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
            let min = &self.text[min_text.clone()];
            let max = &self.text[max_text.clone()];
            min.len().cmp(&max.len()).then_with(|| min.cmp(max)).is_gt()
        });
        if !self.take("]") {
            return Err(self.error(ParseErrorKind::Syntax, "Invalid cardinality range"));
        }
        if reversed {
            // Grammatical, but no count lies between a minimum and a smaller maximum.
            self.refuse(min_text.start, "Cardinality minimum exceeds its maximum");
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
    fn attribute(
        &mut self,
        depth: usize,
        cardinality: Cardinality,
        grouped: bool,
    ) -> Result<Refinement> {
        let flag = self.pos;
        // Whitespace after a flag is optional, including before a long-form name operator.
        let reverse = if self.starts_alternate() {
            false
        } else if self.text[self.pos..]
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("reverseof"))
        {
            self.pos += 9;
            true
        } else if self.word().eq_ignore_ascii_case("refsetcontainingany") {
            false
        } else {
            self.take("R") || self.take("r")
        };
        if reverse && grouped {
            // 6.2 and 6.3 define the reverse flag for whole refinements only; a reversed
            // relationship belongs to the source concept's group, never to the tested concept's.
            self.refuse(
                flag,
                "Reverse flag inside an attribute group has no defined ECL semantics",
            );
        }
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
        } else if !self.starts_alternate() && self.value_keyword("true") {
            AttributeValue::Boolean(true)
        } else if !self.starts_alternate() && self.value_keyword("false") {
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
            // The grammar admits R with a concrete value; 6.2 defines reversal only over
            // destination concepts, so no concrete value can be a reversed source.
            self.refuse(
                flag,
                "Reverse flag with a concrete value has no defined ECL semantics",
            );
        }
        Ok(Refinement::Attribute(AttributeConstraint {
            cardinality,
            reverse,
            name,
            comparison,
            value,
        }))
    }
    /// One operand of a refinement: an attribute or bracketed attribute set,
    /// which may also join an attribute set, or else a group or a bracketed
    /// refinement. The flag says which.
    fn operand(&mut self, depth: usize) -> Result<(Refinement, bool)> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Refinement nesting exceeds 64"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(self.error(ParseErrorKind::Limit, "Too many nodes"));
        }
        self.ws()?;
        let start = self.mark();
        let cardinality = self.cardinality()?;
        self.ws()?;
        if self.take("{") {
            let inner = self.attribute_set(depth + 1, true)?;
            self.ws()?;
            if !self.take("}") {
                return Err(self.unexpected());
            }
            return Ok((Refinement::Group(cardinality, Box::new(inner)), false));
        }
        self.reset(start.clone());
        let attribute = self.subattribute(depth, false);
        if attribute.is_ok() || !self.text[start.pos..].starts_with('(') {
            return attribute.map(|a| (a, true));
        }
        self.reset(start);
        self.take("(");
        let inner = self.refinement(depth + 1)?;
        self.ws()?;
        if !self.take(")") {
            return Err(self.unexpected());
        }
        Ok((inner, false))
    }

    /// Attributes joined by one operator, as the grammar's `eclAttributeSet`,
    /// which is all a group or a bracketed attribute set may hold.
    fn attribute_set(&mut self, depth: usize, grouped: bool) -> Result<Refinement> {
        let mut parts = vec![self.subattribute(depth, grouped)?];
        let mut operator = None;
        loop {
            let before = self.mark();
            let Some(next) = self.boolean()? else { break };
            if next == Boolean::Minus || operator.is_some_and(|op| op != next) {
                self.reset(before);
                break;
            }
            operator = Some(next);
            parts.push(self.subattribute(depth, grouped)?);
        }
        Ok(match operator {
            None => parts.pop().expect("one part"),
            Some(Boolean::And) => Refinement::And(parts),
            Some(_) => Refinement::Or(parts),
        })
    }

    /// An attribute, or a bracketed attribute set.
    fn subattribute(&mut self, depth: usize, grouped: bool) -> Result<Refinement> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Refinement nesting exceeds 64"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(self.error(ParseErrorKind::Limit, "Too many nodes"));
        }
        self.ws()?;
        let start = self.mark();
        let has_cardinality = self.rest().starts_with('[');
        let cardinality = self.cardinality()?;
        let attribute = self.attribute(depth, cardinality, grouped);
        if attribute.is_ok() || has_cardinality || !self.text[start.pos..].starts_with('(') {
            return attribute;
        }
        self.reset(start);
        self.take("(");
        let inner = self.attribute_set(depth + 1, grouped)?;
        self.ws()?;
        if !self.take(")") {
            return Err(self.unexpected());
        }
        Ok(inner)
    }

    /// Operands joined by operators.
    ///
    /// The grammar nests attribute sets, each with one operator, inside a
    /// refinement with one operator, and only attributes may join a set. So an
    /// unbracketed mix of conjunction and disjunction is grammatical exactly
    /// when every operator beside a group or bracketed refinement agrees; it
    /// then derives more than one way, as `a, b OR c` reads both `(a, b) OR c`
    /// and `a, (b OR c)`. 6.4 makes brackets mandatory for such a mix, so it is
    /// refused as ambiguous rather than read one way. Otherwise it is a syntax
    /// error.
    pub(super) fn refinement(&mut self, depth: usize) -> Result<Refinement> {
        let start = self.pos;
        let (first, attribute) = self.operand(depth)?;
        let mut parts = vec![first];
        let mut attributes = vec![attribute];
        let mut operators = Vec::new();
        while let Some(operator) = self.boolean()? {
            if operator == Boolean::Minus {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "Exclusion is not a refinement operator",
                ));
            }
            let (part, attribute) = self.operand(depth)?;
            parts.push(part);
            attributes.push(attribute);
            operators.push(operator);
        }
        let Some(&operator) = operators.first() else {
            return Ok(parts.pop().expect("one part"));
        };
        if operators.iter().any(|&op| op != operator) {
            let mut beside_groups = operators
                .iter()
                .enumerate()
                .filter(|&(i, _)| !attributes[i] || !attributes[i + 1])
                .map(|(_, &op)| op);
            let first = beside_groups.next();
            if beside_groups.any(|op| Some(op) != first) {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "Mixed refinement operators require parentheses",
                ));
            }
            self.refuse(
                start,
                "Conjunction and disjunction together require brackets (6.4)",
            );
        }
        Ok(if operator == Boolean::And {
            Refinement::And(parts)
        } else {
            Refinement::Or(parts)
        })
    }
}

/// Text after any whitespace and comments.
fn skip_space(mut text: &str) -> &str {
    loop {
        text = text.trim_start_matches([' ', '\t', '\r', '\n']);
        match text.strip_prefix("/*").and_then(|c| c.find("*/").map(|end| &c[end + 2..])) {
            Some(rest) => text = rest,
            None => return text,
        }
    }
}
