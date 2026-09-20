use super::*;
use crate::decimal::Decimal;
use crate::ecl::{Comparison, MemberPredicate, MemberQuery};
use crate::store::{MemberColumn, MemberValue};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq)]
pub enum QueryResult {
    Concepts(Vec<u32>),
    Values(Vec<MemberValue>),
    Rows(Vec<BTreeMap<String, MemberValue>>),
}
impl QueryResult {
    pub fn len(&self) -> usize {
        match self {
            Self::Concepts(v) => v.len(),
            Self::Values(v) => v.len(),
            Self::Rows(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
enum Prepared<'a> {
    Concepts(Vec<u32>),
    Number(&'a Decimal),
    Boolean(Option<bool>),
    Dates(&'a [Option<u32>]),
    DatesOrText {
        dates: &'a [Option<u32>],
        #[cfg(feature = "unicode")]
        terms: Option<crate::text::Terms<'a>>,
    },
    #[cfg(feature = "unicode")]
    Text(crate::text::Terms<'a>),
}

impl Context<'_> {
    pub(super) fn member_query(
        &mut self,
        query: &MemberQuery,
        depth: usize,
        terminal: bool,
    ) -> Result<QueryResult> {
        self.tick(1)?;
        if depth > MAX_DEPTH * 3
            || query.filters.len() > MAX_NODES
            || query.reverse && query.fields.is_some()
        {
            return Err(EvalError::InvalidAst);
        }
        let index = &self.store.member_tables;
        if !index.is_available() {
            return Err(EvalError::Unsupported(
                "Typed member index is absent; rebuild this store from RF2",
            ));
        }
        let candidates = self.eval(&query.source, depth + 1)?;
        let selected: Vec<_> = index
            .refsets()
            .filter(|id| {
                query.reverse
                    || self
                        .store
                        .ordinal(*id)
                        .is_some_and(|o| candidates.binary_search(&o).is_ok())
            })
            .collect();
        self.tick(
            index
                .refsets()
                .count()
                .saturating_mul(candidates.len().checked_ilog2().unwrap_or(0) as usize + 1),
        )?;
        let requested = query
            .fields
            .clone()
            .unwrap_or_else(|| vec!["referencedcomponentid".into()]);
        if requested.len() > 64 {
            return Err(EvalError::InvalidAst);
        }
        for field in requested
            .iter()
            .chain(query.filters.iter().map(|f| &f.field))
        {
            if !selected.iter().any(|r| {
                index
                    .fields(*r)
                    .unwrap()
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(field))
            }) {
                // An ordinary empty member expansion is valid; named projection/filter fields must exist.
                if query.fields.is_some() || !query.filters.is_empty() {
                    return Err(EvalError::InvalidField(field.clone()));
                }
            }
        }
        let mut prepared = Vec::new();
        for filter in &query.filters {
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(EvalError::InvalidAst);
            }
            let op = filter.comparison;
            let value = match &filter.value {
                MemberPredicate::Concepts(expr)
                    if matches!(op, Comparison::Eq | Comparison::Ne) =>
                {
                    Prepared::Concepts(self.eval(expr, depth + 1)?)
                }
                MemberPredicate::Number(value) => Prepared::Number(value),
                MemberPredicate::Boolean(value)
                    if matches!(op, Comparison::Eq | Comparison::Ne) =>
                {
                    Prepared::Boolean(*value)
                }
                MemberPredicate::Dates(values) if !values.is_empty() => Prepared::Dates(values),
                MemberPredicate::DatesOrText { dates, terms } if !dates.is_empty() => {
                    #[cfg(not(feature = "unicode"))]
                    let _ = terms;
                    Prepared::DatesOrText {
                        dates,
                        #[cfg(feature = "unicode")]
                        terms: if dates.iter().all(Option::is_some) {
                            Some(
                                crate::text::Terms::new(terms)
                                    .map_err(|_| EvalError::InvalidAst)?,
                            )
                        } else {
                            None
                        },
                    }
                }
                MemberPredicate::Text(values) if matches!(op, Comparison::Eq | Comparison::Ne) => {
                    #[cfg(feature = "unicode")]
                    {
                        Prepared::Text(
                            crate::text::Terms::new(values).map_err(|_| EvalError::InvalidAst)?,
                        )
                    }
                    #[cfg(not(feature = "unicode"))]
                    {
                        let _ = values;
                        return Err(EvalError::Unsupported(
                            "Member string matching requires the unicode Cargo feature",
                        ));
                    }
                }
                _ => return Err(EvalError::InvalidAst),
            };
            prepared.push(value);
        }
        // Reject tuple-valued subqueries before scanning, even when no row could match.
        if !terminal && requested.len() != 1 {
            return Err(EvalError::TypeMismatch);
        }
        let mut rows = Vec::new();
        let mut values = std::collections::BTreeSet::new();
        let words = self.store.ids.len().div_ceil(32);
        let mut marked = self.reserve(words)?;
        marked.resize(words, 0);
        let row_result = terminal && query.fields.as_ref().is_some_and(|f| f.len() != 1);
        let mut scalar_concepts = None;
        let active_explicit = query
            .filters
            .iter()
            .any(|f| f.field.eq_ignore_ascii_case("active"));
        for refset in selected {
            self.tick(1)?;
            let table = index
                .get(refset)
                .map_err(|e| EvalError::Index(e.to_string()))?
                .unwrap();
            let names = if requested.is_empty() {
                table.names[5..].to_vec()
            } else {
                requested.clone()
            };
            let columns: Option<Vec<_>> = names.iter().map(|name| table.column(name)).collect();
            let Some(columns) = columns else {
                continue;
            };
            let filter_columns: Option<Vec<_>> = query
                .filters
                .iter()
                .map(|f| table.column(&f.field))
                .collect();
            let Some(filter_columns) = filter_columns else {
                continue;
            };
            let concepts =
                query.reverse || columns.len() == 1 && matches!(columns[0], MemberColumn::Id(_));
            if requested.len() == 1 {
                if scalar_concepts.is_some_and(|prior| prior != concepts) {
                    return Err(EvalError::TypeMismatch);
                }
                scalar_concepts = Some(concepts);
            }
            for (column, predicate) in filter_columns.iter().zip(&prepared) {
                validate_type(column, predicate)?;
            }
            let MemberColumn::Boolean(active) = &table.columns[2] else {
                return Err(EvalError::Index("Invalid member status".into()));
            };
            let MemberColumn::Id(references) = &table.columns[5] else {
                return Err(EvalError::TypeMismatch);
            };
            for row in 0..table.len() {
                self.tick(1)?;
                if !active_explicit && active[row] == 0 {
                    continue;
                }
                if query.reverse {
                    self.tick(candidates.len().checked_ilog2().unwrap_or(0) as usize + 1)?;
                    if self
                        .store
                        .ordinal(references[row])
                        .is_none_or(|o| candidates.binary_search(&o).is_err())
                    {
                        continue;
                    }
                }
                let mut matches = true;
                for ((column, predicate), filter) in
                    filter_columns.iter().zip(&mut prepared).zip(&query.filters)
                {
                    if !self.member_matches(column, row, predicate, filter.comparison)? {
                        matches = false;
                        break;
                    }
                }
                if !matches {
                    continue;
                }
                if query.reverse {
                    let ordinal = self.store.ordinal(refset).ok_or(EvalError::TypeMismatch)?;
                    marked[ordinal as usize / 32] |= 1 << (ordinal % 32);
                    break;
                } else if concepts && !row_result {
                    let MemberColumn::Id(values) = columns[0] else {
                        unreachable!()
                    };
                    let ordinal = self
                        .store
                        .ordinal(values[row])
                        .ok_or(EvalError::TypeMismatch)?;
                    marked[ordinal as usize / 32] |= 1 << (ordinal % 32);
                } else if !row_result && requested.len() == 1 {
                    let column = columns[0];
                    let bytes = match column {
                        MemberColumn::Text(text) | MemberColumn::Number(text) => {
                            text.get(row).len()
                        }
                        _ => 40,
                    };
                    self.tick(bytes)?;
                    // Check before cloning a potentially large string.
                    self.claim(16 + bytes.div_ceil(4))?;
                    let mut value = column.value(row);
                    if let MemberValue::Number(number) = &mut value {
                        *number = Decimal::parse(number)
                            .ok_or(EvalError::InvalidAst)?
                            .to_string();
                    }
                    self.live -= 16 + bytes.div_ceil(4);
                    if !values.contains(&value) {
                        self.claim(super::values::value_cost(&value))?;
                        values.insert(value);
                    }
                } else {
                    self.claim(names.len().saturating_mul(32))?;
                    let mut values = BTreeMap::new();
                    for (name, col) in names.iter().zip(&columns) {
                        if let MemberColumn::Text(text) | MemberColumn::Number(text) = col {
                            self.tick(text.get(row).len())?;
                            self.claim(text.get(row).len().div_ceil(4))?;
                        }
                        let canonical = table
                            .names
                            .iter()
                            .find(|n| n.eq_ignore_ascii_case(name))
                            .unwrap()
                            .clone();
                        values.insert(canonical, col.value(row));
                    }
                    rows.push(values);
                }
            }
        }
        for predicate in prepared {
            if let Prepared::Concepts(values) = predicate {
                self.release(values);
            }
        }
        self.release(candidates);
        if scalar_concepts == Some(false) && !row_result {
            self.release(marked);
            return Ok(QueryResult::Values(values.into_iter().collect()));
        }
        if row_result {
            // A field with mixed types across refsets must not silently drop earlier concept values.
            if marked.iter().any(|&bits| bits != 0) {
                return Err(EvalError::TypeMismatch);
            }
            self.release(marked);
            return Ok(QueryResult::Rows(rows));
        }
        let count = marked.iter().map(|v| v.count_ones() as usize).sum();
        let mut result = self.reserve(count)?;
        for (word, &bits) in marked.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                self.tick(1)?;
                result.push(word as u32 * 32 + bits.trailing_zeros());
                bits &= bits - 1;
            }
        }
        self.release(marked);
        Ok(QueryResult::Concepts(result))
    }

    fn member_matches(
        &mut self,
        column: &MemberColumn,
        row: usize,
        predicate: &mut Prepared<'_>,
        op: Comparison,
    ) -> Result<bool> {
        self.tick(1)?;
        let member = match (column, predicate) {
            (MemberColumn::Id(values), Prepared::Concepts(allowed)) => {
                self.tick(allowed.len().checked_ilog2().unwrap_or(0) as usize + 1)?;
                self.store
                    .ordinal(values[row])
                    .is_some_and(|v| allowed.binary_search(&v).is_ok())
            }
            (MemberColumn::Integer(values), Prepared::Number(value)) => {
                return Ok(op.matches(Decimal::parse(&values[row].to_string()).unwrap().cmp(value)))
            }
            (MemberColumn::Number(values), Prepared::Number(value)) => {
                self.tick(values.get(row).len())?;
                return Ok(op.matches(
                    Decimal::parse(values.get(row))
                        .ok_or(EvalError::InvalidAst)?
                        .cmp(value),
                ));
            }
            (MemberColumn::Boolean(values), Prepared::Boolean(value)) => {
                value.is_none_or(|v| v == (values[row] != 0))
            }
            (
                MemberColumn::Time(values),
                Prepared::Dates(dates) | Prepared::DatesOrText { dates, .. },
            ) => {
                self.tick(dates.len())?;
                let actual = (values[row] != 0).then_some(values[row]);
                dates.iter().any(|date| match op {
                    Comparison::Eq | Comparison::Ne => actual == *date,
                    Comparison::Le | Comparison::Ge if actual.is_none() && date.is_none() => true,
                    _ => actual
                        .zip(*date)
                        .is_some_and(|(a, b)| op.matches(a.cmp(&b))),
                })
            }
            #[cfg(feature = "unicode")]
            (
                MemberColumn::Text(values),
                Prepared::Text(terms)
                | Prepared::DatesOrText {
                    terms: Some(terms), ..
                },
            ) => {
                self.tick(terms.work(values.get(row)))?;
                terms
                    .matches(
                        values.get(row),
                        self.store
                            .config
                            .member_language_code()
                            .ok_or(EvalError::InvalidAst)?,
                    )
                    .map_err(|e| EvalError::Text(format!("{e:?}")))?
            }
            #[cfg(feature = "unicode")]
            (
                MemberColumn::Uuid(values),
                Prepared::Text(terms)
                | Prepared::DatesOrText {
                    terms: Some(terms), ..
                },
            ) => {
                let text = crate::store::format_uuid(&values[row]);
                self.tick(terms.work(&text))?;
                terms
                    .matches(
                        &text,
                        self.store
                            .config
                            .member_language_code()
                            .ok_or(EvalError::InvalidAst)?,
                    )
                    .map_err(|e| EvalError::Text(format!("{e:?}")))?
            }
            _ => return Err(EvalError::TypeMismatch),
        };
        Ok(member != (op == Comparison::Ne))
    }
}
fn validate_type(column: &MemberColumn, predicate: &Prepared<'_>) -> Result<()> {
    let valid = match (column, predicate) {
        (MemberColumn::Id(_), Prepared::Concepts(_))
        | (MemberColumn::Integer(_) | MemberColumn::Number(_), Prepared::Number(_))
        | (MemberColumn::Boolean(_), Prepared::Boolean(_))
        | (MemberColumn::Time(_), Prepared::Dates(_) | Prepared::DatesOrText { .. }) => true,
        (MemberColumn::Text(_) | MemberColumn::Uuid(_), Prepared::DatesOrText { dates, .. })
            if dates.iter().all(Option::is_some) =>
        {
            if !cfg!(feature = "unicode") {
                return Err(EvalError::Unsupported(
                    "Member string matching requires the unicode Cargo feature",
                ));
            }
            true
        }
        #[cfg(feature = "unicode")]
        (MemberColumn::Text(_) | MemberColumn::Uuid(_), Prepared::Text(_)) => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(EvalError::TypeMismatch)
    }
}
