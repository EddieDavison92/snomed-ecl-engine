use super::*;
use crate::decimal::Decimal;
use crate::ecl::{AttributeValue, Cardinality, Comparison, Refinement};
use crate::store::ConcreteValue;

pub(super) enum Range {
    Concepts(Vec<u32>),
    /// `*`: every concept, which every concept-valued row holds.
    AnyConcept,
    Concrete(Vec<u32>),
}
pub(super) enum Prepared {
    Attribute {
        cardinality: Cardinality,
        names: Vec<u32>,
        range: Range,
        comparison: Comparison,
        /// Reverse attributes only: each concept something points at, with the
        /// number of distinct sources pointing at it, sorted by concept. Sized
        /// by the answer rather than the edition.
        reverse_counts: Option<Vec<(u32, u32)>>,
    },
    Group(Cardinality, Box<Prepared>),
    And(Vec<Prepared>),
    Or(Vec<Prepared>),
}
impl Context<'_> {
    pub(super) fn prepare(
        &mut self,
        refinement: &Refinement,
        depth: usize,
        grouped: bool,
    ) -> Result<Prepared> {
        self.tick(1)?;
        if depth > MAX_DEPTH * 3 {
            return Err(EvalError::InvalidAst);
        }
        match refinement {
            Refinement::Attribute(attribute) => {
                if grouped && attribute.reverse {
                    // The parser rejects this form; a constructed AST gets the same ruling.
                    return Err(EvalError::Semantic(
                        "Reverse flag inside an attribute group has no defined ECL semantics"
                            .into(),
                    ));
                }
                let names = self.eval(&attribute.name, depth + 1)?;
                let range = match &attribute.value {
                    AttributeValue::Concepts(expr)
                        if !attribute.reverse && matches!(**expr, Expr::All) =>
                    {
                        Range::AnyConcept
                    }
                    AttributeValue::Concepts(expr) => Range::Concepts(self.eval(expr, depth + 1)?),
                    value => {
                        let mut matching = self.reserve(self.store.concrete_values.len())?;
                        for (index, stored) in self.store.concrete_values.iter().enumerate() {
                            self.tick(1)?;
                            let matches = match (value, stored) {
                                (AttributeValue::Number(target), ConcreteValue::Number(wire)) => {
                                    let actual = Decimal::parse(wire.trim_start_matches('#'))
                                        .ok_or(EvalError::InvalidAst)?;
                                    attribute.comparison.matches(actual.cmp(target))
                                }
                                (AttributeValue::Strings(targets), ConcreteValue::Text(wire)) => {
                                    let actual = decode_rf2_string(wire)?;
                                    let equal = targets.contains(&actual);
                                    equal == (attribute.comparison == Comparison::Eq)
                                }
                                (
                                    AttributeValue::Boolean(target),
                                    ConcreteValue::Boolean(actual),
                                ) => {
                                    (*target == *actual) == (attribute.comparison == Comparison::Eq)
                                }
                                _ => false,
                            };
                            if matches {
                                matching.push(index as u32);
                            }
                        }
                        Range::Concrete(matching)
                    }
                };
                let mut reverse_counts = None;
                if attribute.reverse {
                    let Range::Concepts(sources) = &range else {
                        return Err(EvalError::InvalidAst);
                    };
                    let isa = self
                        .store
                        .ordinal(116680003)
                        .is_some_and(|kind| names.binary_search(&kind).is_ok());
                    // `=` walks only the value set; `!=` still walks every other concept.
                    let n = self.store.ids.len();
                    let walk = if attribute.comparison == Comparison::Eq {
                        self.tick(sources.len())?;
                        Box::new(sources.iter().copied()) as Box<dyn Iterator<Item = u32>>
                    } else {
                        self.tick(n)?;
                        Box::new((0..n as u32).filter(|s| sources.binary_search(s).is_err()))
                    };
                    let mut pointed = Vec::new();
                    let mut targets = Vec::new();
                    for source in walk {
                        if !self.store.is_active(source) {
                            continue;
                        }
                        let rows = self.store.attributes.get(source);
                        let parents = self.store.parents.get(source);
                        self.tick(1 + rows.len() + if isa { parents.len() } else { 0 })?;
                        targets.clear();
                        targets.extend(
                            rows.iter()
                                .filter(|row| names.binary_search(&row.kind).is_ok())
                                .map(|row| row.value),
                        );
                        if isa {
                            targets.extend(parents);
                        }
                        // Reverse cardinality counts source concepts, not duplicate incoming rows.
                        targets.sort_unstable();
                        targets.dedup();
                        self.claim(targets.len())?;
                        pointed.extend_from_slice(&targets);
                    }
                    self.tick(pointed.len())?;
                    pointed.sort_unstable();
                    let mut counts: Vec<(u32, u32)> = Vec::new();
                    for &target in &pointed {
                        match counts.last_mut() {
                            Some((last, count)) if *last == target => *count += 1,
                            _ => counts.push((target, 1)),
                        }
                    }
                    self.live -= pointed.len();
                    self.claim(counts.len() * 2)?;
                    reverse_counts = Some(counts);
                }
                Ok(Prepared::Attribute {
                    cardinality: attribute.cardinality,
                    names,
                    range,
                    comparison: attribute.comparison,
                    reverse_counts,
                })
            }
            Refinement::Group(cardinality, inner) => Ok(Prepared::Group(
                *cardinality,
                Box::new(self.prepare(inner, depth + 1, true)?),
            )),
            Refinement::And(parts) | Refinement::Or(parts) => {
                if parts.len() < 2 {
                    return Err(EvalError::InvalidAst);
                }
                let prepared = parts
                    .iter()
                    .map(|part| self.prepare(part, depth + 1, grouped))
                    .collect::<Result<_>>()?;
                Ok(if matches!(refinement, Refinement::And(_)) {
                    Prepared::And(prepared)
                } else {
                    Prepared::Or(prepared)
                })
            }
        }
    }
    pub(super) fn matches_refinement(
        &mut self,
        prepared: &Prepared,
        source: u32,
        group: Option<u32>,
    ) -> Result<bool> {
        self.tick(1)?;
        match prepared {
            Prepared::Attribute {
                cardinality,
                names,
                range,
                comparison,
                reverse_counts,
            } => {
                if let Some(counts) = reverse_counts {
                    let count = counts
                        .binary_search_by_key(&source, |&(target, _)| target)
                        .map_or(0, |i| counts[i].1);
                    return Ok(cardinality.contains(count as usize));
                }
                let rows = match range {
                    Range::Concepts(_) | Range::AnyConcept => self.store.attributes.get(source),
                    Range::Concrete(_) => self.store.concrete.get(source),
                };
                self.tick(rows.len())?;
                let mut count = 0;
                for row in rows {
                    if group.is_some_and(|g| g != row.group)
                        || names.binary_search(&row.kind).is_err()
                    {
                        continue;
                    }
                    let matches = match range {
                        Range::Concepts(values) => {
                            values.binary_search(&row.value).is_ok()
                                == (*comparison == Comparison::Eq)
                        }
                        Range::AnyConcept => *comparison == Comparison::Eq,
                        Range::Concrete(values) => values.binary_search(&row.value).is_ok(),
                    };
                    if matches {
                        count += 1;
                    }
                }
                if group.is_none()
                    && self
                        .store
                        .ordinal(116680003)
                        .is_some_and(|kind| names.binary_search(&kind).is_ok())
                {
                    let parents = self.store.parents.get(source);
                    match range {
                        Range::Concepts(values) => {
                            self.tick(parents.len())?;
                            count += parents
                                .iter()
                                .filter(|p| {
                                    values.binary_search(p).is_ok()
                                        == (*comparison == Comparison::Eq)
                                })
                                .count();
                        }
                        Range::AnyConcept if *comparison == Comparison::Eq => {
                            count += parents.len();
                        }
                        _ => {}
                    }
                }
                Ok(cardinality.contains(count))
            }
            Prepared::Group(cardinality, inner) => {
                let rows = self.store.attributes.get(source);
                let concrete = self.store.concrete.get(source);
                self.tick(rows.len() + concrete.len())?;
                let mut groups = self.reserve(rows.len() + concrete.len())?;
                groups.extend(
                    rows.iter()
                        .chain(concrete)
                        .map(|r| r.group)
                        .filter(|g| *g != 0),
                );
                groups.sort_unstable();
                groups.dedup();
                let mut count = 0;
                for &group in &groups {
                    if self.matches_refinement(inner, source, Some(group))? {
                        count += 1;
                    }
                }
                self.release(groups);
                Ok(cardinality.contains(count))
            }
            Prepared::And(parts) => {
                for part in parts {
                    if !self.matches_refinement(part, source, group)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Prepared::Or(parts) => {
                for part in parts {
                    if self.matches_refinement(part, source, group)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
    /// A sorted superset of the concepts that can satisfy `prepared`, when one
    /// is cheaper to name than testing `limit` focus concepts one by one;
    /// `None` means test every focus concept.
    ///
    /// An attribute needing at least one match holds only for concepts with a
    /// matching row. For a reverse attribute those are the keys of its counts;
    /// for a forward one, the sources pointing at a value in its range, plus
    /// the children of those values when the name includes is-a. So
    /// `* : 363698007 = << 39607008` tests the concepts with a lung site
    /// rather than the edition. The per-concept test still decides every
    /// answer.
    pub(super) fn candidates(
        &mut self,
        prepared: &Prepared,
        limit: usize,
    ) -> Result<Option<Vec<u32>>> {
        Ok(match prepared {
            Prepared::Attribute { cardinality, .. } if cardinality.min == 0 => None,
            Prepared::Attribute {
                reverse_counts: Some(counts),
                ..
            } => Some(counts.iter().map(|&(target, _)| target).collect()),
            Prepared::Attribute {
                names,
                range: Range::Concepts(values),
                comparison: Comparison::Eq,
                ..
            } if values.len() <= limit => {
                let store = self.store;
                let isa = store
                    .ordinal(116680003)
                    .is_some_and(|kind| names.binary_search(&kind).is_ok());
                self.tick(values.len())?;
                let mut total = 0usize;
                for &value in values {
                    total += store.attributes.sources(value).len();
                    if isa {
                        total += store.children.get(value).len();
                    }
                }
                if total > limit {
                    return Ok(None);
                }
                self.tick(total)?;
                let mut bound = Vec::with_capacity(total);
                for &value in values {
                    bound.extend_from_slice(store.attributes.sources(value));
                    if isa {
                        bound.extend_from_slice(store.children.get(value));
                    }
                }
                bound.sort_unstable();
                bound.dedup();
                Some(bound)
            }
            Prepared::Attribute { .. } => None,
            Prepared::Group(cardinality, inner) if cardinality.min >= 1 => {
                self.candidates(inner, limit)?
            }
            Prepared::Group(..) => None,
            Prepared::And(parts) => {
                let mut bound: Option<Vec<u32>> = None;
                for part in parts {
                    if let Some(set) = self.candidates(part, limit)? {
                        bound = Some(match bound {
                            None => set,
                            Some(current) => intersect(&current, &set),
                        });
                    }
                }
                bound
            }
            Prepared::Or(parts) => {
                let mut union = Vec::new();
                for part in parts {
                    let Some(set) = self.candidates(part, limit)? else {
                        return Ok(None);
                    };
                    union.extend(set);
                }
                union.sort_unstable();
                union.dedup();
                Some(union)
            }
        })
    }
    pub(super) fn release_prepared(&mut self, prepared: Prepared) {
        match prepared {
            Prepared::Attribute {
                names,
                range,
                reverse_counts,
                ..
            } => {
                self.release(names);
                match range {
                    Range::Concepts(values) | Range::Concrete(values) => self.release(values),
                    Range::AnyConcept => {}
                }
                if let Some(counts) = reverse_counts {
                    self.live -= counts.len() * 2;
                }
            }
            Prepared::Group(_, inner) => self.release_prepared(*inner),
            Prepared::And(parts) | Prepared::Or(parts) => {
                for part in parts {
                    self.release_prepared(part);
                }
            }
        }
    }
    pub(super) fn project(&mut self, seeds: &[u32], names: &[u32]) -> Result<QueryResult> {
        let mut selected = Vec::new();
        let mut values = std::collections::BTreeSet::new();
        let isa = self
            .store
            .ordinal(116680003)
            .is_some_and(|kind| names.binary_search(&kind).is_ok());
        for &seed in seeds {
            let rows = self.store.attributes.get(seed);
            self.tick(1 + rows.len())?;
            for row in rows {
                if names.binary_search(&row.kind).is_ok() && self.store.is_active(row.value) {
                    self.claim(1)?;
                    selected.push(row.value);
                }
            }
            if isa {
                let parents = self.store.parents.get(seed);
                self.tick(parents.len())?;
                self.claim(parents.len())?;
                selected.extend_from_slice(parents);
            }
            self.tick(self.store.concrete.get(seed).len())?;
            for row in self.store.concrete.get(seed) {
                if names.binary_search(&row.kind).is_err() {
                    continue;
                }
                let wire = &self.store.concrete_values[row.value as usize];
                let bytes = match wire {
                    ConcreteValue::Number(v) | ConcreteValue::Text(v) => v.len(),
                    ConcreteValue::Boolean(_) => 0,
                };
                self.tick(bytes + 1)?;
                self.claim(16 + bytes.div_ceil(4))?;
                let value = match wire {
                    ConcreteValue::Number(v) => crate::store::MemberValue::Number(
                        Decimal::parse(v.trim_start_matches('#'))
                            .ok_or(EvalError::InvalidAst)?
                            .to_string(),
                    ),
                    ConcreteValue::Text(v) => {
                        crate::store::MemberValue::String(decode_rf2_string(v)?)
                    }
                    ConcreteValue::Boolean(v) => crate::store::MemberValue::Boolean(*v),
                };
                self.live -= 16 + bytes.div_ceil(4);
                if !values.contains(&value) {
                    self.claim(super::values::value_cost(&value))?;
                    values.insert(value);
                }
            }
        }
        self.tick(selected.len())?;
        let claimed = selected.len();
        selected.sort_unstable();
        selected.dedup();
        if !values.is_empty() {
            for &i in &selected {
                self.tick(1)?;
                let value =
                    crate::store::MemberValue::Concept(self.store.ids[i as usize].to_string());
                self.claim(super::values::value_cost(&value))?;
                values.insert(value);
            }
            self.live -= claimed;
            return Ok(QueryResult::Values(values.into_iter().collect()));
        }
        selected.shrink_to_fit();
        self.live -= claimed;
        self.claim(selected.capacity())?;
        Ok(QueryResult::Concepts(selected))
    }
}

/// Both inputs sorted and unique.
pub(super) fn intersect(left: &[u32], right: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(left.len().min(right.len()));
    let (mut a, mut b) = (0, 0);
    while a < left.len() && b < right.len() {
        match left[a].cmp(&right[b]) {
            std::cmp::Ordering::Less => a += 1,
            std::cmp::Ordering::Greater => b += 1,
            std::cmp::Ordering::Equal => {
                out.push(left[a]);
                a += 1;
                b += 1;
            }
        }
    }
    out
}

fn decode_rf2_string(wire: &str) -> Result<String> {
    let mut chars = wire
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or(EvalError::InvalidAst)?
        .chars();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        if c == '\\' {
            result.push(chars.next().ok_or(EvalError::InvalidAst)?);
        } else {
            result.push(c);
        }
    }
    Ok(result)
}
