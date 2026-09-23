//! ECL 2.3 parsing. Unsupported constructs fail before evaluation.
use std::fmt;
mod descriptions;
mod filters;
pub use descriptions::{DescriptionFilter, Dialect};
mod history;
mod identifiers;
mod members;
mod refinement;
mod search;
pub use filters::ConceptFilter;
pub use history::History;
pub use members::{MemberFilter, MemberPredicate, MemberQuery};
pub use refinement::{AttributeConstraint, AttributeValue, Cardinality, Comparison, Refinement};
pub use search::SearchTerm;

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
    AlternateIdentifier { scheme: String, code: String },
    DialectAlias(String),
    All,
    Hierarchy(Hierarchy, Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Minus(Box<Expr>, Box<Expr>),
    Refined(Box<Expr>, Box<Refinement>),
    Dotted(Box<Expr>, Vec<Expr>),
    Extremum { top: bool, inner: Box<Expr> },
    MemberOf(Box<Expr>),
    Members(MemberQuery),
    History(Box<Expr>, History),
    RefsetContainingAny(Box<Expr>),
    ConceptFiltered(Box<Expr>, Vec<ConceptFilter>),
    DescriptionFiltered(Box<Expr>, Vec<DescriptionFilter>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    Syntax,
    /// The engine does not implement this valid ECL form yet.
    Unsupported,
    /// A recognised combination is refused because of its semantic rules or unresolved meaning.
    Semantic,
    Limit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
        refused: None,
    };
    if text.len() > MAX_QUERY_BYTES {
        return Err(parser.error(ParseErrorKind::Limit, "Query exceeds 65536 bytes"));
    }
    let expression = parser.expression(0)?;
    parser.ws()?;
    if parser.pos != text.len() {
        return Err(parser.unexpected());
    }
    // A grammatical expression the engine refuses for its meaning. Reported
    // only once the whole text parses, so malformed text is a syntax error.
    if let Some(refusal) = parser.refused {
        return Err(refusal);
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
    /// The first semantic refusal, held until the text is known to parse.
    refused: Option<ParseError>,
}

/// Where the parser was, to return to after an alternative fails.
#[derive(Clone)]
struct Mark {
    pos: usize,
    nodes: usize,
    refused: Option<ParseError>,
}

impl Parser<'_> {
    fn mark(&self) -> Mark {
        Mark {
            pos: self.pos,
            nodes: self.nodes,
            refused: self.refused.clone(),
        }
    }
    fn reset(&mut self, mark: Mark) {
        self.pos = mark.pos;
        self.nodes = mark.nodes;
        self.refused = mark.refused;
    }
    /// Records a grammatical form refused for its meaning, and parses on.
    fn refuse(&mut self, at: usize, message: &'static str) {
        self.refused.get_or_insert(ParseError {
            kind: ParseErrorKind::Semantic,
            offset: at,
            message,
        });
    }
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
                .any(|c| c.is_ascii_control() && !matches!(c, '\t' | '\r' | '\n'))
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
            let refinement = self.refinement(depth + 1)?;
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
        } else if !self.starts_alternate() && self.keyword("top") {
            self.required_ws()?;
            Some(true)
        } else if !self.starts_alternate() && self.keyword("bottom") {
            self.required_ws()?;
            Some(false)
        } else {
            None
        };
        self.ws()?;
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
        if hierarchy.is_none() && !self.starts_alternate() {
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
        if extremum.is_some() && hierarchy.is_some() {
            return Err(self.error(
                ParseErrorKind::Syntax,
                "Unary operators require a parenthesised operand",
            ));
        }
        // ABNF quoted strings are case-insensitive (RFC 5234 2.3) and the parsing guidance
        // says keywords are case-insensitive, so "^R" admits ^r; only ECL.g4 restricts it to CAP_R.
        // `^R#x` and `^R-#x` are memberOf over the alternate identifiers `R#x` and
        // `R-#x`: a scheme alias starts with a letter, so `^R` then `#x` or `-#x`
        // has no reading.
        let alternate_scheme_r = {
            let bytes = self.rest().as_bytes();
            bytes.len() > 2
                && bytes[..2].eq_ignore_ascii_case(b"^r")
                && !bytes[2].is_ascii_alphabetic()
                && bytes[2..]
                    .iter()
                    .find(|b| !(b.is_ascii_alphanumeric() || **b == b'-'))
                    == Some(&b'#')
        };
        let refset_operator = if !alternate_scheme_r && (self.take("^R") || self.take("^r")) {
            Some(true)
        } else if self.take("^") {
            Some(false)
        } else if !self.starts_alternate() && self.operator_keyword("memberOf") {
            self.ws()?;
            Some(false)
        } else if !self.starts_alternate() && self.operator_keyword("refsetContainingAny") {
            self.ws()?;
            Some(true)
        } else {
            None
        };
        self.ws()?;
        let fields = if refset_operator == Some(false) && self.rest().starts_with('[') {
            Some(self.member_fields()?)
        } else {
            None
        };
        let mut expression = if self.take("(") {
            let inner = self.expression(depth + 1)?;
            self.ws()?;
            if !self.take(")") {
                return Err(self.unexpected());
            }
            inner
        } else if self.starts_alternate() {
            self.alternate_identifier()?
        } else if self.take("*") {
            self.node(Expr::All)?
        } else if self.value_keyword("any") {
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
        let mut member_filters = Vec::new();
        while self.starts_member_filter()? {
            if refset_operator.is_none() {
                // Logical model 4: member filters apply to results of the memberOf function.
                self.refuse(
                    self.pos,
                    "Member filters require a refset operator (^ or ^R); ECL defines them only over memberOf rows",
                );
            }
            member_filters.extend(self.member_filters(depth + 1)?);
            self.ws()?;
        }
        if let Some(reverse) = refset_operator {
            expression = self.node(if fields.is_some() || !member_filters.is_empty() {
                Expr::Members(MemberQuery {
                    source: Box::new(expression),
                    reverse,
                    fields,
                    filters: member_filters,
                })
            } else if reverse {
                Expr::RefsetContainingAny(Box::new(expression))
            } else {
                Expr::MemberOf(Box::new(expression))
            })?;
        }
        if let Some(op) = hierarchy {
            expression = self.node(Expr::Hierarchy(op, Box::new(expression)))?;
        }
        if let Some(top) = extremum {
            expression = self.node(Expr::Extremum {
                top,
                inner: Box::new(expression),
            })?;
        }
        // The fallback to a member filter below only reads a refused form, so
        // it applies before any other filter and without a refset operator.
        let mut filtered = refset_operator.is_some();
        while self.rest().starts_with("{{") {
            let saved = self.pos;
            self.take("{{");
            self.ws()?;
            let concept = self.rest().starts_with(['C', 'c']);
            let history = self.rest().starts_with('+');
            self.pos = saved;
            if history {
                let supplement = self.history(depth + 1)?;
                expression = self.node(Expr::History(Box::new(expression), supplement))?;
                self.ws()?;
                break;
            } else if concept {
                let filters = self.concept_filters(depth + 1)?;
                expression = self.node(Expr::ConceptFiltered(Box::new(expression), filters))?;
                filtered = true;
            } else {
                let mark = self.mark();
                match self.description_filters(depth + 1) {
                    Ok(filters) => {
                        expression =
                            self.node(Expr::DescriptionFiltered(Box::new(expression), filters))?;
                        // `{{moduleid = *}}` also reads as the member filter `m oduleid`,
                        // and then a member filter may still follow it.
                        let end = self.mark();
                        self.reset(mark.clone());
                        let member = !filtered
                            && self.starts_member_filter_letter()
                            && self.member_filters(depth + 1).is_ok()
                            && self.pos == end.pos;
                        self.reset(end);
                        if member {
                            self.ws()?;
                            continue;
                        }
                    }
                    Err(error) => {
                        // `{{moduleid = *, x = *}}` is also `{{m oduleid = *, x = *}}`, a
                        // member filter, which the grammar admits without a refset operator.
                        self.reset(mark.clone());
                        if filtered
                            || !self.starts_member_filter_letter()
                            || self.member_filters(depth + 1).is_err()
                        {
                            self.reset(mark);
                            return Err(error);
                        }
                        self.refuse(
                            mark.pos,
                            "Member filters require a refset operator (^ or ^R); ECL defines them only over memberOf rows",
                        );
                        // More member filters may follow this one.
                        self.ws()?;
                        continue;
                    }
                }
                filtered = true;
            }
            self.ws()?;
        }
        Ok(expression)
    }
    /// A long-syntax operator that the grammar lets run into `ANY`, as in
    /// `memberOfANY`.
    fn operator_keyword(&mut self, word: &str) -> bool {
        let found = self.word();
        let joined = found.len() == word.len() + 3
            && found[..word.len()].eq_ignore_ascii_case(word)
            && found[word.len()..].eq_ignore_ascii_case("any");
        if found.eq_ignore_ascii_case(word) || joined {
            self.pos += word.len();
            true
        } else {
            false
        }
    }
    fn unexpected(&self) -> ParseError {
        self.error(
            ParseErrorKind::Syntax,
            "Unexpected token or missing operand",
        )
    }
}
