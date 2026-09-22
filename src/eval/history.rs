use super::*;
use crate::ecl::History;
use crate::store::MemberColumn;

const ASSOCIATIONS: u64 = 900000000000522004;
const SAME_AS: u64 = 900000000000527005;
const MOVED_FROM: u64 = 900000000000525002;
const MOVED_TO: u64 = 900000000000524003;

impl Context<'_> {
    pub(super) fn history(
        &mut self,
        inner: &Expr,
        history: &History,
        depth: usize,
    ) -> Result<Vec<u32>> {
        if !self.store.member_tables.is_available() {
            return Err(EvalError::Unsupported(
                "History requires the typed member index; rebuild from RF2",
            ));
        }
        let initial = self.eval(inner, depth + 1)?;
        let maximum = Expr::Hierarchy(Hierarchy::Descendant, Box::new(Expr::Concept(ASSOCIATIONS)));
        let moderate = Expr::Or(vec![
            Expr::Concept(SAME_AS),
            Expr::Concept(900000000000526001),
            Expr::Concept(900000000000528000),
            Expr::Concept(1186924009),
        ]);
        let minimum = Expr::Concept(SAME_AS);
        let selection = match history {
            History::Minimum => &minimum,
            History::Moderate => &moderate,
            History::Maximum => &maximum,
            History::Subset(expr) if matches!(expr.as_ref(), Expr::All) => &maximum,
            History::Subset(expr) => expr,
        };
        let selected = self.eval(selection, depth + 1)?;
        let store = self.store;
        let includes = |id| {
            store
                .ordinal(id)
                .is_some_and(|o| selected.binary_search(&o).is_ok())
        };
        let moved_from = includes(SAME_AS) || includes(MOVED_FROM);
        let wanted = |refset: u64| {
            refset != MOVED_TO && (includes(refset) || refset == MOVED_FROM && moved_from)
        };
        if let Some(index) = self.store.history.get().map_err(|e| EvalError::Index(e.to_string()))? {
            // The index only holds association tables. A subset that selects
            // some other table keeps the scan below, and its behaviour.
            let covered = self
                .store
                .member_tables
                .refsets()
                .filter(|&refset| wanted(refset))
                .all(|refset| index.kind_of(refset).is_some());
            if covered {
                let kinds: Vec<bool> = index.refsets().iter().map(|&r| wanted(r)).collect();
                let reversed = index.kind_of(MOVED_FROM).filter(|&k| kinds[k as usize]);
                let result = self.indexed_history(index, &initial, &kinds, reversed)?;
                self.release(initial);
                self.release(selected);
                return Ok(result);
            }
        }
        let words = self.store.ids.len().div_ceil(32);
        let mut marked = self.reserve(words)?;
        marked.resize(words, 0);
        for &ordinal in &initial {
            self.tick(1)?;
            marked[ordinal as usize / 32] |= 1 << (ordinal % 32);
        }
        for refset in self.store.member_tables.refsets() {
            self.tick(1)?;
            if refset == MOVED_TO || !(includes(refset) || refset == MOVED_FROM && moved_from) {
                continue;
            }
            let table = self
                .store
                .member_tables
                .get(refset)
                .map_err(|e| EvalError::Index(e.to_string()))?
                .unwrap();
            let Some(MemberColumn::Id(targets)) = table.column("targetComponentId") else {
                return Err(EvalError::InvalidField("targetComponentId".into()));
            };
            let MemberColumn::Id(references) = &table.columns[5] else {
                return Err(EvalError::TypeMismatch);
            };
            let MemberColumn::Boolean(active) = &table.columns[2] else {
                return Err(EvalError::TypeMismatch);
            };
            for row in 0..table.len() {
                self.tick(1 + initial.len().checked_ilog2().unwrap_or(0) as usize)?;
                if active[row] == 0 {
                    continue;
                }
                // MOVED FROM has the opposite direction to ordinary historical associations.
                let (target, source) = if refset == MOVED_FROM {
                    (references[row], targets[row])
                } else {
                    (targets[row], references[row])
                };
                if self
                    .store
                    .ordinal(target)
                    .is_some_and(|o| initial.binary_search(&o).is_ok())
                {
                    let ordinal = self.store.ordinal(source).ok_or(EvalError::TypeMismatch)?;
                    marked[ordinal as usize / 32] |= 1 << (ordinal % 32);
                }
            }
        }
        self.release(initial);
        self.release(selected);
        self.tick(words)?;
        let mut result = self.reserve(marked.iter().map(|v| v.count_ones() as usize).sum())?;
        for (word, &bits) in marked.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                self.tick(1)?;
                result.push(word as u32 * 32 + bits.trailing_zeros());
                bits &= bits - 1;
            }
        }
        self.release(marked);
        Ok(result)
    }

    /// The same answer as the scan above, from the index.
    ///
    /// Ordinary associations point a retired concept at its replacement, so
    /// the retired ones are found from the replacement: the backward rows of
    /// each concept in hand. MOVED FROM runs the other way and comes from the
    /// forward rows. What is found is sorted once and merged with the input.
    fn indexed_history(
        &mut self,
        index: &crate::store::HistoryIndex,
        initial: &[u32],
        kinds: &[bool],
        reversed: Option<u8>,
    ) -> Result<Vec<u32>> {
        self.tick(initial.len())?;
        let mut found = Vec::new();
        let mut work = 0usize;
        index.walk(true, initial, |_, others, row_kinds| {
            work += 1 + others.len();
            for (&other, &kind) in others.iter().zip(row_kinds) {
                if kinds[kind as usize] && Some(kind) != reversed {
                    found.push(other);
                }
            }
        });
        if let Some(reversed) = reversed {
            index.walk(false, initial, |_, others, row_kinds| {
                work += 1 + others.len();
                for (&other, &kind) in others.iter().zip(row_kinds) {
                    if kind == reversed {
                        found.push(other);
                    }
                }
            });
        }
        self.tick(work + found.len())?;
        found.sort_unstable();
        found.dedup();
        let result = self.merge(initial, &found, 1)?;
        Ok(result)
    }
}
