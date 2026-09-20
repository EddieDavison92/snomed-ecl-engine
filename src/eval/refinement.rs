use super::*;
use crate::decimal::Decimal;
use crate::ecl::{AttributeValue, Cardinality, Comparison, Refinement};
use crate::store::ConcreteValue;

pub(super) enum Range {
    Concepts(Vec<u32>),
    Concrete(Vec<u32>),
}
pub(super) enum Prepared {
    Attribute {
        cardinality: Cardinality,
        names: Vec<u32>,
        range: Range,
        comparison: Comparison,
        reverse_counts: Option<Vec<u32>>,
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
                    return Err(EvalError::Unsupported("Reverse attributes inside groups"));
                }
                let names = self.eval(&attribute.name, depth + 1)?;
                let range = match &attribute.value {
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
                    let n = self.store.ids.len();
                    self.tick(n)?;
                    let mut counts = self.reserve(n)?;
                    counts.resize(n, 0);
                    let isa = self
                        .store
                        .ordinal(116680003)
                        .is_some_and(|kind| names.binary_search(&kind).is_ok());
                    for source in 0..n as u32 {
                        if !self.store.is_active(source) {
                            continue;
                        }
                        let selected = sources.binary_search(&source).is_ok();
                        if selected != (attribute.comparison == Comparison::Eq) {
                            continue;
                        }
                        let rows = self.store.attributes.get(source);
                        let parents = self.store.parents.get(source);
                        self.tick(1 + rows.len() + if isa { parents.len() } else { 0 })?;
                        let mut targets =
                            self.reserve(rows.len() + if isa { parents.len() } else { 0 })?;
                        targets.extend(
                            rows.iter()
                                .filter(|row| names.binary_search(&row.kind).is_ok())
                                .map(|row| row.value),
                        );
                        if isa {
                            targets.extend(parents);
                        }
                        targets.sort_unstable();
                        targets.dedup();
                        // Reverse cardinality counts source concepts, not duplicate incoming rows.
                        for &target in &targets {
                            counts[target as usize] += 1;
                        }
                        self.release(targets);
                    }
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
                    return Ok(cardinality.contains(counts[source as usize] as usize));
                }
                let rows = match range {
                    Range::Concepts(_) => self.store.attributes.get(source),
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
                    if let Range::Concepts(values) = range {
                        let parents = self.store.parents.get(source);
                        self.tick(parents.len())?;
                        count += parents
                            .iter()
                            .filter(|p| {
                                values.binary_search(p).is_ok() == (*comparison == Comparison::Eq)
                            })
                            .count();
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
                }
                if let Some(counts) = reverse_counts {
                    self.release(counts);
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
    pub(super) fn dotted(&mut self, seeds: &[u32], names: &[u32]) -> Result<Vec<u32>> {
        let n = self.store.ids.len();
        self.tick(n)?;
        let mut selected = vec![false; n];
        let isa = self
            .store
            .ordinal(116680003)
            .is_some_and(|kind| names.binary_search(&kind).is_ok());
        for &seed in seeds {
            let rows = self.store.attributes.get(seed);
            self.tick(1 + rows.len())?;
            for row in rows {
                if names.binary_search(&row.kind).is_ok() && self.store.is_active(row.value) {
                    selected[row.value as usize] = true;
                }
            }
            if isa {
                self.tick(self.store.parents.get(seed).len())?;
                for &parent in self.store.parents.get(seed) {
                    selected[parent as usize] = true;
                }
            }
            self.tick(self.store.concrete.get(seed).len())?;
            if self
                .store
                .concrete
                .get(seed)
                .iter()
                .any(|row| names.binary_search(&row.kind).is_ok())
            {
                return Err(EvalError::Unsupported(
                    "Concrete dotted projection needs typed result output",
                ));
            }
        }
        let mut result = self.reserve(selected.iter().filter(|&&s| s).count())?;
        result.extend(
            selected
                .iter()
                .enumerate()
                .filter(|(_, s)| **s)
                .map(|(i, _)| i as u32),
        );
        Ok(result)
    }
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
