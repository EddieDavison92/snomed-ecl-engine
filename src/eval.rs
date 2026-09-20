//! Sorted ordinal sets with bounded evaluation work and temporary storage.
use crate::ecl::{Expr, Hierarchy, MAX_DEPTH, MAX_NODES};
use crate::store::NumericStore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{error::Error, fmt};
mod descriptions;
mod filters;
mod history;
mod members;
mod membership;
pub use members::QueryResult;
mod refinement;
mod values;

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
    Index(String),
    Text(String),
    InvalidField(String),
    TypeMismatch,
    /// A projected concept identifier names no concept of this substrate.
    MissingReference(String),
    UnconfiguredAlias(String),
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
pub fn evaluate_result(store: &NumericStore, expression: &Expr) -> Result<QueryResult> {
    evaluate_result_with_limits(store, expression, Limits::default(), None)
}
pub fn evaluate_result_with_limits(
    store: &NumericStore,
    expression: &Expr,
    limits: Limits,
    cancelled: Option<&AtomicBool>,
) -> Result<QueryResult> {
    let mut context = Context {
        store,
        limits,
        cancelled,
        work: 0,
        live: 0,
        nodes: 0,
    };
    context.result(expression, 0, true)
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
        self.claim(values)?;
        Ok(Vec::with_capacity(values))
    }
    fn claim(&mut self, values: usize) -> Result<()> {
        self.live = self
            .live
            .checked_add(values)
            .ok_or(EvalError::MemoryLimit)?;
        if self.live > self.limits.max_live_set_values {
            return Err(EvalError::MemoryLimit);
        }
        Ok(())
    }
    fn release(&mut self, values: Vec<u32>) {
        self.live -= values.capacity();
    }
    fn eval(&mut self, expr: &Expr, depth: usize) -> Result<Vec<u32>> {
        let result = self.result(expr, depth, false)?;
        self.concept_values(result)
    }
    fn eval_concepts(&mut self, expr: &Expr, depth: usize) -> Result<Vec<u32>> {
        self.tick(1)?;
        self.nodes += 1;
        if depth > MAX_DEPTH * 3 || self.nodes > MAX_NODES {
            return Err(EvalError::InvalidAst);
        }
        match expr {
            Expr::AlternateIdentifier { scheme, code } => {
                let id = self
                    .store
                    .config
                    .identifier_schemes
                    .get(&scheme.to_ascii_lowercase())
                    .ok_or_else(|| EvalError::UnconfiguredAlias(scheme.clone()))?;
                let index = self
                    .store
                    .identifiers
                    .get()
                    .map_err(|e| EvalError::Index(e.to_string()))?
                    .ok_or(EvalError::Unsupported(
                        "Identifier index is absent; rebuild from RF2",
                    ))?;
                self.tick(index.rows.len().checked_ilog2().unwrap_or(0) as usize + code.len() + 1)?;
                let mut result = self.reserve(1)?;
                if let Some(code) = index.lookup(*id, code) {
                    let ordinal = self.store.ordinal(code).ok_or_else(|| {
                        EvalError::Index("Identifier refers to an absent concept".into())
                    })?;
                    result.push(ordinal);
                }
                Ok(result)
            }
            Expr::DialectAlias(alias) => {
                let id = self
                    .store
                    .config
                    .dialects
                    .get(&alias.to_ascii_lowercase())
                    .ok_or_else(|| EvalError::UnconfiguredAlias(alias.clone()))?;
                let mut result = self.reserve(1)?;
                if let Some(ordinal) = self.store.ordinal(*id) {
                    result.push(ordinal);
                }
                Ok(result)
            }
            Expr::History(inner, supplement) => self.history(inner, supplement, depth + 1),
            Expr::DescriptionFiltered(inner, filters) => {
                let candidates = self.eval(inner, depth + 1)?;
                self.description_filters(candidates, filters, depth + 1)
            }
            Expr::ConceptFiltered(inner, filters) => {
                let candidates = self.eval(inner, depth + 1)?;
                self.concept_filters(candidates, filters, depth + 1)
            }
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
            Expr::Members(_)
            | Expr::Dotted(_, _)
            | Expr::And(_)
            | Expr::Or(_)
            | Expr::Minus(_, _) => Err(EvalError::InvalidAst),
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
