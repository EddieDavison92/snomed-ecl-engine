use super::{Context, EvalError, Result};
use crate::store::MembershipIndex;

impl Context<'_> {
    /// Section 6.1 restricts memberOf to reference sets whose referenced components are
    /// concepts. `^ *` is a specified query over a substrate that contains language reference
    /// sets, so the restriction is enforced when no selected reference set is in that domain.
    /// The domain lists come from RF2 descriptors and every Snapshot row, not from which
    /// members happen to be active.
    pub(super) fn check_member_domain(
        &mut self,
        index: &MembershipIndex,
        candidates: &[u32],
    ) -> Result<()> {
        let (Some(concept), Some(non_concept)) =
            (&index.concept_refsets, &index.non_concept_refsets)
        else {
            return Ok(());
        };
        let probe = self.store.ids.len().checked_ilog2().unwrap_or(0) as usize
            + candidates.len().checked_ilog2().unwrap_or(0) as usize
            + 2;
        let selected = |store: &crate::store::NumericStore, id: u64| {
            store
                .ordinal(id)
                .is_some_and(|o| candidates.binary_search(&o).is_ok())
        };
        self.tick(non_concept.len().saturating_mul(probe))?;
        let Some(offending) = non_concept
            .iter()
            .copied()
            .find(|&id| selected(self.store, id))
        else {
            return Ok(());
        };
        self.tick(concept.len().saturating_mul(probe))?;
        if concept.iter().any(|&id| selected(self.store, id)) {
            return Ok(());
        }
        Err(EvalError::Semantic(format!(
            "memberOf applies only to reference sets whose referenced components are concepts; {offending} references descriptions or relationships"
        )))
    }

    pub(super) fn membership(&mut self, candidates: &[u32], reverse: bool) -> Result<Vec<u32>> {
        let index = self
            .store
            .membership
            .as_ref()
            .ok_or(EvalError::Unsupported(
                "Membership index is absent; rebuild this store from RF2",
            ))?;
        if candidates.is_empty() {
            return self.reserve(0);
        }
        if !reverse {
            self.check_member_domain(index, candidates)?;
        }
        if !reverse && candidates.len() == 1 {
            self.tick(index.refsets.len().checked_ilog2().unwrap_or(0) as usize + 1)?;
            let Ok(position) = index.refsets.binary_search(&candidates[0]) else {
                return self.reserve(0);
            };
            let members = index.get(position);
            let mut result = self.reserve(members.len())?;
            for &member in members {
                self.tick(1)?;
                result.push(member);
            }
            return Ok(result);
        }
        if reverse {
            let mut result = self.reserve(index.refsets.len())?;
            for (position, &refset) in index.refsets.iter().enumerate() {
                self.tick(1)?;
                let members = index.get(position);
                let (small, large) = if candidates.len() < members.len() {
                    (candidates, members)
                } else {
                    (members, candidates)
                };
                for value in small {
                    self.tick(large.len().checked_ilog2().unwrap_or(0) as usize + 1)?;
                    if large.binary_search(value).is_ok() {
                        result.push(refset);
                        break;
                    }
                }
            }
            return Ok(result);
        }
        // A bounded marker vector avoids repeatedly sorting/merging overlapping refsets.
        let words = self.store.ids.len().div_ceil(32);
        let mut marked = self.reserve(words)?;
        marked.resize(words, 0);
        self.tick(marked.len())?;
        for (position, refset) in index.refsets.iter().enumerate() {
            self.tick(candidates.len().checked_ilog2().unwrap_or(0) as usize + 1)?;
            if candidates.binary_search(refset).is_ok() {
                for &member in index.get(position) {
                    self.tick(1)?;
                    marked[member as usize / 32] |= 1 << (member % 32);
                }
            }
        }
        self.tick(marked.len())?;
        let count = marked.iter().map(|v| v.count_ones() as usize).sum();
        self.tick(count)?;
        let mut result = self.reserve(count)?;
        for (word, &bits) in marked.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                result.push(word as u32 * 32 + bits.trailing_zeros());
                bits &= bits - 1;
            }
        }
        self.release(marked);
        Ok(result)
    }
}
