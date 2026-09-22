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
    /// The engine does not implement this valid ECL form yet.
    Unsupported(&'static str),
    /// The requested combination has no supported semantic interpretation.
    Semantic(String),
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
        marks: Marks::borrow(),
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
        marks: Marks::borrow(),
    }
    .eval(expression, 0)
}
/// Past this many concepts a descendant focus is not materialised when the
/// refinement names fewer candidates than the edition's sixteenth.
const SMALL_FOCUS: usize = 4096;

/// The focus of a refinement, as far as it has been evaluated.
enum Focus {
    Everything,
    Set(Vec<u32>),
    /// A descendant or child operator whose answer exceeds `SMALL_FOCUS`.
    Below(Hierarchy, Vec<u32>),
}

struct Context<'a> {
    store: &'a NumericStore,
    limits: Limits,
    cancelled: Option<&'a AtomicBool>,
    work: u64,
    live: usize,
    nodes: usize,
    marks: Marks,
}

/// Visit markers shared by every hierarchy operator.
///
/// A marker is set when it holds the current stamp, so starting a traversal is
/// an increment rather than clearing an array the size of the edition. Stamps
/// are single bytes, which keeps a full traversal's memory traffic the same as
/// a plain boolean array; after 255 traversals both arrays are cleared once,
/// which costs about a millisecond and happens rarely.
///
/// The arrays outlive the query: each thread keeps its own, so a process that
/// answers many questions allocates them once rather than page-faulting a
/// fresh pair in for every expression.
#[derive(Default)]
struct Marks {
    seen: Vec<u8>,
    selected: Vec<u8>,
    stamp: u8,
}
impl Marks {
    fn next(&mut self, n: usize) -> u8 {
        if self.seen.len() != n {
            self.seen = vec![0; n];
            self.selected = vec![0; n];
            self.stamp = 0;
        }
        if self.stamp == u8::MAX {
            self.seen.fill(0);
            self.selected.fill(0);
            self.stamp = 0;
        }
        self.stamp += 1;
        self.stamp
    }
    /// This thread's markers, left behind by its previous query if any.
    fn borrow() -> Self {
        MARKS.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
    }
}

thread_local! {
    static MARKS: std::cell::RefCell<Marks> = std::cell::RefCell::new(Marks::default());
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        let marks = std::mem::take(&mut self.marks);
        MARKS.with(|slot| *slot.borrow_mut() = marks);
    }
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
                let prepared = self.prepare(refinement, depth + 1, false)?;
                // Test only concepts that can satisfy the refinement, when it
                // names them. `*` then needn't be materialised at all, nor a
                // large descendant focus: the few candidates are checked
                // against its seeds by walking up instead.
                let focus = match focus.as_ref() {
                    Expr::All => Focus::Everything,
                    Expr::Hierarchy(op, inner) if !op.ancestors() => {
                        let seeds = self.eval(inner, depth + 1)?;
                        match self.hierarchy_within(*op, &seeds, SMALL_FOCUS)? {
                            Some(set) => {
                                self.release(seeds);
                                Focus::Set(set)
                            }
                            None => Focus::Below(*op, seeds),
                        }
                    }
                    other => Focus::Set(self.eval(other, depth + 1)?),
                };
                let limit = match &focus {
                    Focus::Set(set) => set.len(),
                    _ => self.store.ids.len(),
                };
                let n = self.store.ids.len();
                let candidates = match (self.candidates(&prepared, limit)?, focus) {
                    (Some(bound), Focus::Everything) => {
                        self.tick(bound.len())?;
                        self.claim(bound.capacity())?;
                        bound
                    }
                    (Some(bound), Focus::Below(op, seeds)) if bound.len() <= n / 16 => {
                        let tested = self.below(op, &seeds, &bound)?;
                        self.release(seeds);
                        tested
                    }
                    (bound, Focus::Below(op, seeds)) => {
                        let set = self.hierarchy(op, &seeds)?;
                        self.release(seeds);
                        match bound {
                            Some(bound) => self.within(set, &bound)?,
                            None => set,
                        }
                    }
                    (Some(bound), Focus::Set(set)) => self.within(set, &bound)?,
                    (None, Focus::Set(set)) => set,
                    (None, Focus::Everything) => self.eval(&Expr::All, depth + 1)?,
                };
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
            Expr::Extremum { top: true, inner } => {
                let candidates = self.eval(inner, depth + 1)?;
                let result = self.top(&candidates)?;
                self.release(candidates);
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
    /// Walks the hierarchy from `seeds`, paying only for what it touches.
    ///
    /// The earlier version charged the size of the whole edition twice per
    /// operator and scanned every concept to collect its answer, so `<< X`
    /// cost the same whether X had three descendants or a hundred thousand,
    /// and one expression could hold only about forty subsumptions before the
    /// work budget ran out. Here the work is the edges walked, and the answer
    /// is collected as it is found and sorted, so a union of hundreds of small
    /// hierarchies costs what those hierarchies cost.
    fn hierarchy(&mut self, op: Hierarchy, seeds: &[u32]) -> Result<Vec<u32>> {
        Ok(self.hierarchy_within(op, seeds, usize::MAX)?.unwrap())
    }
    /// The hierarchy of `seeds`, or `None` once it exceeds `cap` concepts.
    fn hierarchy_within(
        &mut self,
        op: Hierarchy,
        seeds: &[u32],
        cap: usize,
    ) -> Result<Option<Vec<u32>>> {
        if seeds.is_empty() {
            return self.reserve(0).map(Some);
        }
        let store = self.store;
        let graph = if op.ancestors() {
            &store.parents
        } else {
            &store.children
        };
        let n = store.ids.len();
        let stamp = self.marks.next(n);
        self.tick(seeds.len())?;
        // Past this many results, reading the markers back in order beats
        // sorting a list, so the list stops growing and the markers are used.
        let large = n / 16;
        let mut count = 0usize;
        let mut stack = Vec::with_capacity(seeds.len());
        let mut found = Vec::new();
        for &seed in seeds {
            let i = seed as usize;
            if self.marks.seen[i] != stamp {
                self.marks.seen[i] = stamp;
                stack.push(seed);
            }
            if op.include_self() && self.marks.selected[i] != stamp {
                self.marks.selected[i] = stamp;
                count += 1;
                if count <= large {
                    found.push(seed);
                }
            }
        }
        while let Some(node) = stack.pop() {
            let next = graph.get(node);
            self.tick(1 + next.len())?;
            for &concept in next {
                let i = concept as usize;
                if self.marks.selected[i] != stamp {
                    self.marks.selected[i] = stamp;
                    count += 1;
                    if count <= large {
                        found.push(concept);
                    }
                    if count > cap {
                        return Ok(None);
                    }
                }
                if !op.direct() && self.marks.seen[i] != stamp {
                    self.marks.seen[i] = stamp;
                    stack.push(concept);
                }
            }
        }
        // Every set operation downstream merges sorted lists. A small answer is
        // sorted, at k log k; a large one is read back from the markers in
        // order, at one sequential pass over the edition.
        if count > large {
            self.tick(n)?;
            found = Vec::with_capacity(count);
            found.extend(
                self.marks
                    .selected
                    .iter()
                    .enumerate()
                    .filter(|&(_, &mark)| mark == stamp)
                    .map(|(i, _)| i as u32),
            );
        } else {
            self.tick(found.len())?;
            found.sort_unstable();
        }
        found.shrink_to_fit();
        self.claim(found.capacity())?;
        Ok(Some(found))
    }
    /// The members of `set` with no proper ancestor in it.
    ///
    /// Walks upward, so the cost is the set's ancestors rather than its
    /// descendants, which for a broad concept are much of the edition.
    fn top(&mut self, set: &[u32]) -> Result<Vec<u32>> {
        let stamp = self.mark_seeds(set)?;
        let mut result = self.reserve(set.len())?;
        let mut stack = Vec::new();
        for &member in set {
            if !self.has_seed_above(member, stamp, &mut stack)? {
                result.push(member);
            }
        }
        Ok(result)
    }

    /// The members of `candidates` in `op` of `seeds`, for a descendant or
    /// child operator, found by walking up from each candidate rather than
    /// down from the seeds. Cheaper when the candidates are few and the
    /// seeds' descendants many.
    fn below(&mut self, op: Hierarchy, seeds: &[u32], candidates: &[u32]) -> Result<Vec<u32>> {
        debug_assert!(!op.ancestors());
        let stamp = self.mark_seeds(seeds)?;
        let mut result = self.reserve(candidates.len())?;
        let mut stack = Vec::new();
        for &candidate in candidates {
            let is_seed = seeds.binary_search(&candidate).is_ok();
            self.tick(1)?;
            let inside = if op.include_self() && is_seed {
                true
            } else if op.direct() {
                let parents = self.store.parents.get(candidate);
                self.tick(parents.len())?;
                parents.iter().any(|p| seeds.binary_search(p).is_ok())
            } else {
                self.has_seed_above(candidate, stamp, &mut stack)?
            };
            if inside {
                result.push(candidate);
            }
        }
        Ok(result)
    }

    /// Starts a memoised upward search: `seen` means the answer for a concept
    /// is known, `selected` that the concept is a seed or lies below one.
    fn mark_seeds(&mut self, seeds: &[u32]) -> Result<u8> {
        let stamp = self.marks.next(self.store.ids.len());
        self.tick(seeds.len())?;
        for &seed in seeds {
            self.marks.seen[seed as usize] = stamp;
            self.marks.selected[seed as usize] = stamp;
        }
        Ok(stamp)
    }

    /// Whether a proper ancestor of `concept` is a seed. Each concept's answer
    /// is kept, so a search costs the ancestors not already resolved.
    fn has_seed_above(
        &mut self,
        concept: u32,
        stamp: u8,
        stack: &mut Vec<(u32, usize)>,
    ) -> Result<bool> {
        let store = self.store;
        let parents = store.parents.get(concept);
        self.tick(1 + parents.len())?;
        for &parent in parents {
            if self.marks.seen[parent as usize] != stamp {
                self.marks.seen[parent as usize] = stamp;
                stack.push((parent, 0));
                while let Some((node, next)) = stack.last_mut() {
                    let Some(&up) = store.parents.get(*node).get(*next) else {
                        // Nothing above reaches a seed.
                        stack.pop();
                        continue;
                    };
                    *next += 1;
                    self.tick(1)?;
                    if self.marks.seen[up as usize] != stamp {
                        self.marks.seen[up as usize] = stamp;
                        stack.push((up, 0));
                    } else if self.marks.selected[up as usize] == stamp {
                        // Every concept on the stack lies below `up`.
                        for (node, _) in stack.drain(..) {
                            self.marks.selected[node as usize] = stamp;
                        }
                    }
                }
            }
            if self.marks.selected[parent as usize] == stamp {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// `set` restricted to the sorted `bound`, releasing `set`.
    fn within(&mut self, set: Vec<u32>, bound: &[u32]) -> Result<Vec<u32>> {
        self.tick(set.len().min(bound.len()) + bound.len())?;
        let tested = refinement::intersect(&set, bound);
        self.release(set);
        self.claim(tested.capacity())?;
        Ok(tested)
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
