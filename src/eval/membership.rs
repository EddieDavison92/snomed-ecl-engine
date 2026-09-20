use super::{Context, EvalError, Result};

impl Context<'_> {
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
