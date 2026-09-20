use super::{Context, EvalError, Result};
use crate::ecl::{Comparison, ConceptFilter, MAX_NODES};

impl Context<'_> {
    pub(super) fn concept_filters(
        &mut self,
        mut candidates: Vec<u32>,
        filters: &[ConceptFilter],
        depth: usize,
    ) -> Result<Vec<u32>> {
        if filters.is_empty() || filters.len() > MAX_NODES {
            return Err(EvalError::InvalidAst);
        }
        for filter in filters {
            self.tick(1)?;
            self.nodes += 1;
            if self.nodes > MAX_NODES {
                return Err(EvalError::InvalidAst);
            }
            let (comparison, values) = match filter {
                ConceptFilter::Module(op, expression)
                | ConceptFilter::DefinitionStatus(op, expression) => {
                    (*op, Some(self.eval(expression, depth)?))
                }
                ConceptFilter::Active(op, _)
                | ConceptFilter::Defined(op, _)
                | ConceptFilter::EffectiveTime(op, _) => (*op, None),
            };
            if !matches!(filter, ConceptFilter::EffectiveTime(_, _))
                && !matches!(comparison, Comparison::Eq | Comparison::Ne)
            {
                return Err(EvalError::InvalidAst);
            }
            match filter {
                ConceptFilter::Defined(_, v) if v.is_empty() => return Err(EvalError::InvalidAst),
                ConceptFilter::EffectiveTime(_, v) if v.is_empty() => {
                    return Err(EvalError::InvalidAst)
                }
                _ => {}
            }
            let mut write = 0;
            for read in 0..candidates.len() {
                self.tick(1)?;
                let concept = candidates[read];
                let index = concept as usize;
                let member = match filter {
                    ConceptFilter::Active(_, value) => {
                        value.is_none_or(|active| self.store.is_active(concept) == active)
                    }
                    ConceptFilter::Defined(_, allowed) => {
                        self.tick(allowed.len())?;
                        allowed.contains(&(self.store.flags[index] & 2 != 0))
                    }
                    ConceptFilter::Module(_, _) | ConceptFilter::DefinitionStatus(_, _) => {
                        let values = values.as_ref().unwrap();
                        self.tick(values.len().checked_ilog2().unwrap_or(0) as usize + 1)?;
                        let ordinal = if matches!(filter, ConceptFilter::Module(..)) {
                            Some(self.store.modules[index])
                        } else {
                            self.store.ordinal(if self.store.flags[index] & 2 != 0 {
                                900000000000073002
                            } else {
                                900000000000074008
                            })
                        };
                        ordinal.is_some_and(|ordinal| values.binary_search(&ordinal).is_ok())
                    }
                    ConceptFilter::EffectiveTime(_, allowed) => {
                        self.tick(allowed.len())?;
                        let effective = self.store.effective_times[index];
                        let actual = (effective != 0).then_some(effective);
                        allowed.iter().any(|value| match comparison {
                            Comparison::Eq | Comparison::Ne => actual == *value,
                            Comparison::Le | Comparison::Ge
                                if actual.is_none() && value.is_none() =>
                            {
                                true
                            }
                            _ => actual
                                .zip(*value)
                                .is_some_and(|(a, b)| comparison.matches(a.cmp(&b))),
                        })
                    }
                };
                if member != (comparison == Comparison::Ne) {
                    candidates[write] = concept;
                    write += 1;
                }
            }
            candidates.truncate(write);
            if let Some(values) = values {
                self.release(values);
            }
        }
        Ok(candidates)
    }
}
