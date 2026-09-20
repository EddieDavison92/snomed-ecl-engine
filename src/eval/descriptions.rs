use super::*;
use crate::ecl::{Comparison, ConceptFilter, DescriptionFilter};
use crate::store::DescriptionIndex;

enum Prepared<'a> {
    #[cfg(feature = "unicode")]
    Term(Comparison, crate::text::Terms<'a>),
    Active(Comparison, Option<bool>),
    Language(Comparison, &'a [[u8; 2]]),
    Id(Comparison, &'a [u64]),
    Metadata(Comparison, bool, Vec<u32>),
    Date(Comparison, &'a [Option<u32>]),
    Dialect(Comparison, Vec<(Vec<u32>, &'a [u64])>),
}

impl Context<'_> {
    pub(super) fn description_filters(
        &mut self,
        mut candidates: Vec<u32>,
        filters: &[DescriptionFilter],
        depth: usize,
    ) -> Result<Vec<u32>> {
        if filters.is_empty() || filters.len() > MAX_NODES {
            return Err(EvalError::InvalidAst);
        }
        self.tick(1)?;
        #[cfg(not(feature = "unicode"))]
        if filters
            .iter()
            .any(|f| matches!(f, DescriptionFilter::Term(..)))
        {
            return Err(EvalError::Unsupported(
                "Term matching requires the unicode Cargo feature",
            ));
        }
        let index = self
            .store
            .descriptions
            .get()
            .map_err(|e| EvalError::Index(e.to_string()))?
            .ok_or(EvalError::Unsupported(
                "Store has no description index; rebuild from RF2",
            ))?;
        let mut prepared = Vec::new();
        let mut active_explicit = false;
        for filter in filters {
            self.tick(1)?;
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(EvalError::InvalidAst);
            }
            let item = match filter {
                #[cfg(feature = "unicode")]
                DescriptionFilter::Term(op, terms) => Prepared::Term(
                    *op,
                    crate::text::Terms::new(terms).map_err(|_| EvalError::InvalidAst)?,
                ),
                DescriptionFilter::Metadata(ConceptFilter::Active(op, active)) => {
                    active_explicit = true;
                    Prepared::Active(*op, *active)
                }
                DescriptionFilter::Metadata(ConceptFilter::Module(op, expr)) => {
                    Prepared::Metadata(*op, false, self.eval(expr, depth + 1)?)
                }
                DescriptionFilter::Metadata(ConceptFilter::EffectiveTime(op, dates))
                    if !dates.is_empty() =>
                {
                    Prepared::Date(*op, dates)
                }
                DescriptionFilter::Type(op, expr) => {
                    Prepared::Metadata(*op, true, self.eval(expr, depth + 1)?)
                }
                DescriptionFilter::Language(op, languages)
                    if !languages.is_empty()
                        && languages
                            .iter()
                            .all(|l| l.iter().all(u8::is_ascii_lowercase)) =>
                {
                    Prepared::Language(*op, languages)
                }
                DescriptionFilter::Id(op, ids) if !ids.is_empty() => Prepared::Id(*op, ids),
                DescriptionFilter::Dialect(op, dialects) if !dialects.is_empty() => {
                    let mut values = Vec::new();
                    for dialect in dialects {
                        self.tick(dialect.acceptability.len() + 1)?;
                        values.push((
                            self.eval(&dialect.refsets, depth + 1)?,
                            dialect.acceptability.as_slice(),
                        ));
                    }
                    Prepared::Dialect(*op, values)
                }
                _ => return Err(EvalError::InvalidAst),
            };
            let op = match &item {
                #[cfg(feature = "unicode")]
                Prepared::Term(op, _) => *op,
                Prepared::Active(op, _)
                | Prepared::Language(op, _)
                | Prepared::Id(op, _)
                | Prepared::Metadata(op, ..)
                | Prepared::Date(op, _)
                | Prepared::Dialect(op, _) => *op,
            };
            if !matches!(item, Prepared::Date(..)) && !matches!(op, Comparison::Eq | Comparison::Ne)
            {
                return Err(EvalError::InvalidAst);
            }
            prepared.push(item);
        }
        let mut write = 0;
        for read in 0..candidates.len() {
            self.tick(1)?;
            let concept = candidates[read];
            let mut found = false;
            for row in index.for_concept(concept) {
                self.tick(1)?;
                if !active_explicit && !index.active(row) {
                    continue;
                }
                let mut matches = true;
                for predicate in &mut prepared {
                    if !self.description_matches(index, row, predicate)? {
                        matches = false;
                        break;
                    }
                }
                if matches {
                    found = true;
                    break;
                }
            }
            if found {
                candidates[write] = concept;
                write += 1;
            }
        }
        candidates.truncate(write);
        for predicate in prepared {
            match predicate {
                Prepared::Metadata(_, _, values) => self.release(values),
                Prepared::Dialect(_, dialects) => {
                    for (values, _) in dialects {
                        self.release(values);
                    }
                }
                _ => {}
            }
        }
        Ok(candidates)
    }

    fn description_matches(
        &mut self,
        index: &DescriptionIndex,
        row: usize,
        predicate: &mut Prepared<'_>,
    ) -> Result<bool> {
        self.tick(1)?;
        let (op, member) = match predicate {
            #[cfg(feature = "unicode")]
            Prepared::Term(op, terms) => {
                self.tick(terms.work_bytes(index.term_bytes(row)))?;
                let matches = index
                    .with_term(row, |text| {
                        terms
                            .matches(text, index.language(row))
                            .map_err(|e| EvalError::Text(format!("{e:?}")))
                    })
                    .map_err(|e| EvalError::Index(e.to_string()))??;
                (*op, matches)
            }
            Prepared::Active(op, value) => (*op, value.is_none_or(|v| v == index.active(row))),
            Prepared::Language(op, values) => {
                self.tick(values.len())?;
                (*op, values.contains(&index.language(row)))
            }
            Prepared::Id(op, values) => {
                self.tick(values.len())?;
                (*op, values.contains(&index.id(row)))
            }
            Prepared::Metadata(op, kind, values) => {
                self.tick(values.len().checked_ilog2().unwrap_or(0) as usize + 1)?;
                (
                    *op,
                    values
                        .binary_search(&if *kind {
                            index.kind(row)
                        } else {
                            index.module(row)
                        })
                        .is_ok(),
                )
            }
            Prepared::Date(op, values) => {
                self.tick(values.len())?;
                let date = index.effective_time(row);
                let actual = (date != 0).then_some(date);
                (
                    *op,
                    values.iter().any(|value| match op {
                        Comparison::Eq | Comparison::Ne => actual == *value,
                        Comparison::Le | Comparison::Ge if actual.is_none() && value.is_none() => {
                            true
                        }
                        _ => actual
                            .zip(*value)
                            .is_some_and(|(a, b)| op.matches(a.cmp(&b))),
                    }),
                )
            }
            Prepared::Dialect(op, dialects) => {
                let mut found = false;
                for (refset, acceptability) in index.dialects(row) {
                    for (values, allowed) in dialects.iter() {
                        self.tick(
                            values.len().checked_ilog2().unwrap_or(0) as usize + allowed.len() + 1,
                        )?;
                        if values.binary_search(&refset).is_ok()
                            && (allowed.is_empty()
                                || allowed.contains(&self.store.ids[acceptability as usize]))
                        {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
                (*op, found)
            }
        };
        Ok(member != (op == Comparison::Ne))
    }
}
