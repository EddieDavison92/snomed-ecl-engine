//! Sorted ordinal sets with bounded evaluation work and temporary storage.
use crate::ecl::{Expr, Hierarchy, MAX_DEPTH, MAX_NODES};
use crate::store::NumericStore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{error::Error, fmt};
mod membership;
mod refinement;

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_work: u64,
    pub max_live_set_values: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_work: 100_000_000,
            max_live_set_values: 8_000_000,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum EvalError {
    WorkLimit,
    MemoryLimit,
    Cancelled,
    InvalidAst,
    Unsupported(&'static str),
}
impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Evaluation failed: {self:?}")
    }
}
impl Error for EvalError {}
type Result<T> = std::result::Result<T, EvalError>;

pub fn evaluate(store: &NumericStore, expression: &Expr) -> Result<Vec<u32>> {
    evaluate_with_limits(store, expression, Limits::default(), None)
}
pub fn evaluate_with_limits(
    store: &NumericStore,
    expression: &Expr,
    limits: Limits,
    cancelled: Option<&AtomicBool>,
) -> Result<Vec<u32>> {
    Context {
        store,
        limits,
        cancelled,
        work: 0,
        live: 0,
        nodes: 0,
    }
    .eval(expression, 0)
}
struct Context<'a> {
    store: &'a NumericStore,
    limits: Limits,
    cancelled: Option<&'a AtomicBool>,
    work: u64,
    live: usize,
    nodes: usize,
}
impl Context<'_> {
    fn tick(&mut self, work: usize) -> Result<()> {
        if self
            .cancelled
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Err(EvalError::Cancelled);
        }
        self.work = self
            .work
            .checked_add(work as u64)
            .ok_or(EvalError::WorkLimit)?;
        if self.work > self.limits.max_work {
            return Err(EvalError::WorkLimit);
        }
        Ok(())
    }
    fn reserve(&mut self, values: usize) -> Result<Vec<u32>> {
        self.live = self
            .live
            .checked_add(values)
            .ok_or(EvalError::MemoryLimit)?;
        if self.live > self.limits.max_live_set_values {
            return Err(EvalError::MemoryLimit);
        }
        Ok(Vec::with_capacity(values))
    }
    fn release(&mut self, values: Vec<u32>) {
        self.live -= values.capacity();
    }
    fn eval(&mut self, expr: &Expr, depth: usize) -> Result<Vec<u32>> {
        self.tick(1)?;
        self.nodes += 1;
        if depth > MAX_DEPTH * 3 || self.nodes > MAX_NODES {
            return Err(EvalError::InvalidAst);
        }
        match expr {
            Expr::MemberOf(inner) | Expr::RefsetContainingAny(inner) => {
                let candidates = self.eval(inner, depth + 1)?;
                let result =
                    self.membership(&candidates, matches!(expr, Expr::RefsetContainingAny(_)))?;
                self.release(candidates);
                Ok(result)
            }
            Expr::Refined(focus, refinement) => {
                let candidates = self.eval(focus, depth + 1)?;
                let prepared = self.prepare(refinement, depth + 1, false)?;
                let mut result = self.reserve(candidates.len())?;
                for &source in &candidates {
                    if self.matches_refinement(&prepared, source, None)? {
                        result.push(source);
                    }
                }
                self.release(candidates);
                self.release_prepared(prepared);
                Ok(result)
            }
            Expr::Dotted(focus, attributes) => {
                let mut result = self.eval(focus, depth + 1)?;
                for attribute in attributes {
                    let names = self.eval(attribute, depth + 1)?;
                    let next = self.dotted(&result, &names)?;
                    self.release(result);
                    self.release(names);
                    result = next;
                }
                Ok(result)
            }
            Expr::Extremum { top, inner } => {
                let candidates = self.eval(inner, depth + 1)?;
                let excluded = self.hierarchy(
                    if *top {
                        Hierarchy::Descendant
                    } else {
                        Hierarchy::Ancestor
                    },
                    &candidates,
                )?;
                let result = self.merge(&candidates, &excluded, 2)?;
                self.release(candidates);
                self.release(excluded);
                Ok(result)
            }
            Expr::Concept(code) => {
                let ordinal = self.store.ordinal(*code);
                let mut result = self.reserve(usize::from(ordinal.is_some()))?;
                result.extend(ordinal);
                Ok(result)
            }
            Expr::All => {
                self.tick(self.store.ids.len())?;
                let count = self.store.ids.len();
                let mut result = self.reserve(count)?;
                result.extend(0..count as u32);
                Ok(result)
            }
            Expr::Hierarchy(op, inner) => {
                let seeds = self.eval(inner, depth + 1)?;
                let result = self.hierarchy(*op, &seeds)?;
                self.release(seeds);
                Ok(result)
            }
            Expr::And(parts) | Expr::Or(parts) => {
                if parts.len() < 2 {
                    return Err(EvalError::InvalidAst);
                }
                let mut result = self.eval(&parts[0], depth + 1)?;
                for part in &parts[1..] {
                    let right = self.eval(part, depth + 1)?;
                    let merged = self.merge(
                        &result,
                        &right,
                        if matches!(expr, Expr::And(_)) { 0 } else { 1 },
                    )?;
                    self.release(result);
                    self.release(right);
                    result = merged;
                }
                Ok(result)
            }
            Expr::Minus(left, right) => {
                let left = self.eval(left, depth + 1)?;
                let right = self.eval(right, depth + 1)?;
                let result = self.merge(&left, &right, 2)?;
                self.release(left);
                self.release(right);
                Ok(result)
            }
        }
    }
    fn hierarchy(&mut self, op: Hierarchy, seeds: &[u32]) -> Result<Vec<u32>> {
        if seeds.is_empty() {
            return self.reserve(0);
        }
        let n = self.store.ids.len();
        self.tick(n)?;
        let graph = if op.ancestors() {
            &self.store.parents
        } else {
            &self.store.children
        };
        let mut seen = vec![false; n];
        let mut selected = vec![false; n];
        // Each vertex is queued once. Input seeds can still be results of other seeds.
        let mut stack = self.reserve(n)?;
        for &seed in seeds {
            seen[seed as usize] = true;
            stack.push(seed);
            if op.include_self() {
                selected[seed as usize] = true;
            }
        }
        while let Some(node) = stack.pop() {
            self.tick(1 + graph.get(node).len())?;
            for &next in graph.get(node) {
                selected[next as usize] = true;
                if !op.direct() && !seen[next as usize] {
                    seen[next as usize] = true;
                    stack.push(next);
                }
            }
        }
        self.release(stack);
        self.tick(n)?;
        let mut result = self.reserve(selected.iter().filter(|&&b| b).count())?;
        result.extend(
            selected
                .iter()
                .enumerate()
                .filter(|(_, b)| **b)
                .map(|(i, _)| i as u32),
        );
        Ok(result)
    }
    fn merge(&mut self, left: &[u32], right: &[u32], mode: u8) -> Result<Vec<u32>> {
        self.tick(left.len() + right.len())?;
        let capacity = match mode {
            0 => left.len().min(right.len()),
            1 => (left.len() + right.len()).min(self.store.ids.len()),
            _ => left.len(),
        };
        let mut result = self.reserve(capacity)?;
        let (mut a, mut b) = (0, 0);
        while a < left.len() || b < right.len() {
            if b == right.len() || a < left.len() && left[a] < right[b] {
                if mode != 0 {
                    result.push(left[a]);
                }
                a += 1;
            } else if a == left.len() || right[b] < left[a] {
                if mode == 1 {
                    result.push(right[b]);
                }
                b += 1;
            } else {
                if mode != 2 {
                    result.push(left[a]);
                }
                a += 1;
                b += 1;
            }
        }
        Ok(result)
    }
}
