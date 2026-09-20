//! ECL 2.3 parsing. Unsupported constructs fail before evaluation.
use std::fmt;
mod refinement;
pub use refinement::{AttributeConstraint, AttributeValue, Cardinality, Comparison, Refinement};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hierarchy {
    Descendant,
    DescendantOrSelf,
    Child,
    ChildOrSelf,
    Ancestor,
    AncestorOrSelf,
    Parent,
    ParentOrSelf,
}

impl Hierarchy {
    pub fn ancestors(self) -> bool {
        matches!(
            self,
            Self::Ancestor | Self::AncestorOrSelf | Self::Parent | Self::ParentOrSelf
        )
    }
    pub fn direct(self) -> bool {
        matches!(
            self,
            Self::Child | Self::ChildOrSelf | Self::Parent | Self::ParentOrSelf
        )
    }
    pub fn include_self(self) -> bool {
        matches!(
            self,
            Self::DescendantOrSelf | Self::ChildOrSelf | Self::AncestorOrSelf | Self::ParentOrSelf
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Concept(u64),
    All,
    Hierarchy(Hierarchy, Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Minus(Box<Expr>, Box<Expr>),
    Refined(Box<Expr>, Box<Refinement>),
    Dotted(Box<Expr>, Vec<Expr>),
    Extremum { top: bool, inner: Box<Expr> },
    MemberOf(Box<Expr>),
    RefsetContainingAny(Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    Syntax,
    Unsupported,
    Limit,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub offset: usize,
    pub message: &'static str,
}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} at byte {}: {}",
            self.kind, self.offset, self.message
        )
    }
}
impl std::error::Error for ParseError {}
type Result<T> = std::result::Result<T, ParseError>;

pub const MAX_QUERY_BYTES: usize = 65536;
pub const MAX_DEPTH: usize = 64;
pub const MAX_NODES: usize = 4096;

pub fn parse(text: &str) -> Result<Expr> {
    let mut parser = Parser {
        text,
        pos: 0,
        nodes: 0,
    };
    if text.len() > MAX_QUERY_BYTES {
        return Err(parser.error(ParseErrorKind::Limit, "Query exceeds 65536 bytes"));
    }
    let expression = parser.expression(0)?;
    parser.ws()?;
    if parser.pos != text.len() {
        return Err(parser.unexpected());
    }
    Ok(expression)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Boolean {
    And,
    Or,
    Minus,
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn error(&self, kind: ParseErrorKind, message: &'static str) -> ParseError {
        ParseError {
            kind,
            offset: self.pos,
            message,
        }
    }
    fn rest(&self) -> &str {
        &self.text[self.pos..]
    }
    fn take(&mut self, value: &str) -> bool {
        if self.rest().starts_with(value) {
            self.pos += value.len();
            true
        } else {
            false
        }
    }
    fn ws(&mut self) -> Result<bool> {
        let start = self.pos;
        loop {
            while self.rest().starts_with([' ', '\t', '\r', '\n']) {
                self.pos += 1;
            }
            if !self.take("/*") {
                break;
            }
            let end = self
                .rest()
                .find("*/")
                .ok_or_else(|| self.error(ParseErrorKind::Syntax, "Unclosed comment"))?;
            if self.rest()[..end]
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
            {
                return Err(self.error(ParseErrorKind::Syntax, "Invalid comment character"));
            }
            self.pos += end + 2;
        }
        Ok(self.pos != start)
    }
    fn word(&self) -> &str {
        let end = self
            .rest()
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count();
        &self.rest()[..end]
    }
    fn required_ws(&mut self) -> Result<()> {
        if self.ws()? {
            Ok(())
        } else {
            Err(self.error(
                ParseErrorKind::Syntax,
                "Keyword requires following whitespace or comment",
            ))
        }
    }
    fn node(&mut self, expression: Expr) -> Result<Expr> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(self.error(ParseErrorKind::Limit, "Too many expression nodes"));
        }
        Ok(expression)
    }
    fn boolean(&mut self) -> Result<Option<Boolean>> {
        self.ws()?;
        if self.take(",") {
            return Ok(Some(Boolean::And));
        }
        let word = self.word();
        let op = if word.eq_ignore_ascii_case("and") {
            Boolean::And
        } else if word.eq_ignore_ascii_case("or") {
            Boolean::Or
        } else if word.eq_ignore_ascii_case("minus") {
            Boolean::Minus
        } else {
            return Ok(None);
        };
        self.pos += word.len();
        self.required_ws()?;
        Ok(Some(op))
    }
    fn term(&mut self) -> Result<()> {
        let original = self.pos;
        let after_ws = match self.ws() {
            Ok(_) => self.pos,
            Err(_) => original,
        };
        // A comment can also be literal annotation text. Try both interpretations.
        for start in [after_ws, original] {
            self.pos = start;
            let mut last_non_space = false;
            while let Some(character) = self.rest().chars().next() {
                if last_non_space {
                    let saved = self.pos;
                    if self.ws().is_ok() && self.take("|") {
                        return Ok(());
                    }
                    self.pos = saved;
                }
                if character == '|' || character.is_ascii_control() {
                    break;
                }
                last_non_space = character != ' ';
                self.pos += character.len_utf8();
            }
        }
        self.pos = original;
        Err(self.error(ParseErrorKind::Syntax, "Invalid or unclosed concept term"))
    }
    fn expression(&mut self, depth: usize) -> Result<Expr> {
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Expression nesting exceeds 64"));
        }
        let left = self.subexpression(depth)?;
        if self.take(":") {
            let refinement = self.refinement(depth + 1, true)?;
            return self.node(Expr::Refined(Box::new(left), Box::new(refinement)));
        }
        if self.take(".") {
            let mut attributes = vec![self.subexpression(depth + 1)?];
            while self.take(".") {
                attributes.push(self.subexpression(depth + 1)?);
            }
            return self.node(Expr::Dotted(Box::new(left), attributes));
        }
        let Some(op) = self.boolean()? else {
            return Ok(left);
        };
        let right = self.subexpression(depth)?;
        if op == Boolean::Minus {
            if self.boolean()?.is_some() {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "Parentheses required around mixed or repeated exclusion",
                ));
            }
            return self.node(Expr::Minus(Box::new(left), Box::new(right)));
        }
        let mut operands = vec![left, right];
        while let Some(next) = self.boolean()? {
            if next != op {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "Mixed Boolean operators require parentheses",
                ));
            }
            operands.push(self.subexpression(depth)?);
        }
        self.node(if op == Boolean::And {
            Expr::And(operands)
        } else {
            Expr::Or(operands)
        })
    }
    fn subexpression(&mut self, depth: usize) -> Result<Expr> {
        self.ws()?;
        if depth > MAX_DEPTH {
            return Err(self.error(ParseErrorKind::Limit, "Expression nesting exceeds 64"));
        }
        let extremum = if self.take("!!>") {
            Some(true)
        } else if self.take("!!<") {
            Some(false)
        } else if self.keyword("top") {
            self.required_ws()?;
            Some(true)
        } else if self.keyword("bottom") {
            self.required_ws()?;
            Some(false)
        } else {
            None
        };
        if let Some(top) = extremum {
            self.ws()?;
            let parenthesised = self.rest().starts_with('(');
            let inner = self.subexpression(depth + 1)?;
            if !parenthesised && matches!(inner, Expr::Hierarchy(_, _) | Expr::Extremum { .. }) {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "Unary operators require a parenthesised operand",
                ));
            }
            return self.node(Expr::Extremum {
                top,
                inner: Box::new(inner),
            });
        }
        let mut hierarchy = None;
        for (symbol, op) in [
            ("<<!", Hierarchy::ChildOrSelf),
            (">>!", Hierarchy::ParentOrSelf),
            ("<<", Hierarchy::DescendantOrSelf),
            (">>", Hierarchy::AncestorOrSelf),
            ("<!", Hierarchy::Child),
            (">!", Hierarchy::Parent),
            ("<", Hierarchy::Descendant),
            (">", Hierarchy::Ancestor),
        ] {
            if self.take(symbol) {
                hierarchy = Some(op);
                break;
            }
        }
        if hierarchy.is_none() {
            let word = self.word();
            for (name, op) in [
                ("descendantof", Hierarchy::Descendant),
                ("descendantorselfof", Hierarchy::DescendantOrSelf),
                ("childof", Hierarchy::Child),
                ("childorselfof", Hierarchy::ChildOrSelf),
                ("ancestorof", Hierarchy::Ancestor),
                ("ancestororselfof", Hierarchy::AncestorOrSelf),
                ("parentof", Hierarchy::Parent),
                ("parentorselfof", Hierarchy::ParentOrSelf),
            ] {
                if word.eq_ignore_ascii_case(name) {
                    hierarchy = Some(op);
                    self.pos += word.len();
                    self.required_ws()?;
                    break;
                }
            }
        }
        self.ws()?;
        let refset_operator = if self.take("^R") || self.take("^r") {
            Some(true)
        } else if self.take("^") {
            Some(false)
        } else if self.keyword("memberOf") {
            self.required_ws()?;
            Some(false)
        } else if self.keyword("refsetContainingAny") {
            self.required_ws()?;
            Some(true)
        } else {
            None
        };
        self.ws()?;
        if refset_operator.is_some() && self.rest().starts_with('[') {
            return Err(self.error(
                ParseErrorKind::Unsupported,
                "Member field projections are not implemented",
            ));
        }
        let mut expression = if self.take("(") {
            let inner = self.expression(depth + 1)?;
            self.ws()?;
            if !self.take(")") {
                return Err(self.unexpected());
            }
            inner
        } else if self.take("*") {
            self.node(Expr::All)?
        } else if self.word().eq_ignore_ascii_case("any") {
            self.pos += 3;
            self.node(Expr::All)?
        } else if self.rest().starts_with(|c: char| c.is_ascii_digit()) {
            let start = self.pos;
            while self.rest().starts_with(|c: char| c.is_ascii_digit()) {
                self.pos += 1;
            }
            let code = &self.text[start..self.pos];
            if !(6..=18).contains(&code.len()) || code.starts_with('0') {
                return Err(self.error(
                    ParseErrorKind::Syntax,
                    "SCTID must contain 6 to 18 digits without a leading zero",
                ));
            }
            let code = code
                .parse()
                .map_err(|_| self.error(ParseErrorKind::Syntax, "Invalid SCTID"))?;
            self.ws()?;
            if self.take("|") {
                self.term()?;
            }
            self.node(Expr::Concept(code))?
        } else {
            return Err(self.unexpected());
        };
        self.ws()?;
        if self.rest().starts_with(['^', '{', '[']) {
            return Err(self.unexpected());
        }
        if let Some(reverse) = refset_operator {
            expression = self.node(if reverse {
                Expr::RefsetContainingAny(Box::new(expression))
            } else {
                Expr::MemberOf(Box::new(expression))
            })?;
        }
        if let Some(op) = hierarchy {
            self.node(Expr::Hierarchy(op, Box::new(expression)))
        } else {
            Ok(expression)
        }
    }
    fn unexpected(&self) -> ParseError {
        let word = self.word().to_ascii_lowercase();
        let feature = if self.rest().starts_with(':') {
            Some("Attribute refinements are not implemented")
        } else if self.rest().starts_with('.') {
            Some("Dotted attributes are not implemented")
        } else if self.rest().starts_with('^')
            || matches!(word.as_str(), "memberof" | "refsetcontainingany")
        {
            Some("Refset operations are not implemented")
        } else if self.rest().starts_with('{') {
            Some("Filters and history supplements are not implemented")
        } else if self.rest().starts_with("!!") || matches!(word.as_str(), "top" | "bottom") {
            Some("Top and bottom are not implemented")
        } else if self.rest().starts_with('"')
            || self
                .rest()
                .split_whitespace()
                .next()
                .is_some_and(|s| s.contains('#'))
        {
            Some("Alternate identifiers are not implemented")
        } else {
            None
        };
        self.error(
            if feature.is_some() {
                ParseErrorKind::Unsupported
            } else {
                ParseErrorKind::Syntax
            },
            feature.unwrap_or("Unexpected token or missing operand"),
        )
    }
}
