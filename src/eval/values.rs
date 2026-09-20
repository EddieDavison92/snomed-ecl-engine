use super::*;
use crate::store::MemberValue;

pub(super) fn value_cost(value: &MemberValue) -> usize {
    let bytes = match value {
        MemberValue::Concept(v)
        | MemberValue::Number(v)
        | MemberValue::Time(v)
        | MemberValue::String(v) => v.len(),
        MemberValue::Boolean(_) => 0,
    };
    16 + bytes.div_ceil(4)
}

impl Context<'_> {
    pub(super) fn result(
        &mut self,
        expr: &Expr,
        depth: usize,
        terminal: bool,
    ) -> Result<QueryResult> {
        self.tick(1)?;
        self.nodes += 1;
        if depth > MAX_DEPTH * 3 || self.nodes > MAX_NODES {
            return Err(EvalError::InvalidAst);
        }
        match expr {
            Expr::Members(query) => self.member_query(query, depth + 1, terminal),
            Expr::And(parts) | Expr::Or(parts) => {
                if parts.len() < 2 {
                    return Err(EvalError::InvalidAst);
                }
                let mut result = self.result(&parts[0], depth + 1, false)?;
                for part in &parts[1..] {
                    let right = self.result(part, depth + 1, false)?;
                    result = self.merge_results(
                        result,
                        right,
                        if matches!(expr, Expr::And(_)) { 0 } else { 1 },
                    )?;
                }
                Ok(result)
            }
            Expr::Minus(left, right) => {
                let left = self.result(left, depth + 1, false)?;
                let right = self.result(right, depth + 1, false)?;
                self.merge_results(left, right, 2)
            }
            Expr::Dotted(focus, attributes) => {
                if attributes.is_empty() {
                    return Err(EvalError::InvalidAst);
                }
                let mut current = self.result(focus, depth + 1, false)?;
                for attribute in attributes {
                    let QueryResult::Concepts(seeds) = current else {
                        return Err(EvalError::TypeMismatch);
                    };
                    let names = self.eval(attribute, depth + 1)?;
                    current = self.project(&seeds, &names)?;
                    self.release(seeds);
                    self.release(names);
                }
                Ok(current)
            }
            _ => self.eval(expr, depth + 1).map(QueryResult::Concepts),
        }
    }

    fn merge_results(
        &mut self,
        left: QueryResult,
        right: QueryResult,
        mode: u8,
    ) -> Result<QueryResult> {
        match (left, right) {
            (QueryResult::Concepts(left), QueryResult::Concepts(right)) => {
                let result = self.merge(&left, &right, mode)?;
                self.release(left);
                self.release(right);
                Ok(QueryResult::Concepts(result))
            }
            (left, right)
                if !matches!(left, QueryResult::Rows(_))
                    && !matches!(right, QueryResult::Rows(_)) =>
            {
                let left = self.scalar_values(left)?;
                let right = self.scalar_values(right)?;
                let old_cost: usize = left.iter().chain(&right).map(value_cost).sum();
                let mut result = Vec::new();
                let mut a = left.into_iter().peekable();
                let mut b = right.into_iter().peekable();
                while a.peek().is_some() || b.peek().is_some() {
                    let compared_bytes =
                        a.peek().map_or(0, value_cost) + b.peek().map_or(0, value_cost);
                    self.tick(compared_bytes)?;
                    let order = match (a.peek(), b.peek()) {
                        (Some(a), Some(b)) => a.cmp(b),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        _ => std::cmp::Ordering::Greater,
                    };
                    let value = match order {
                        std::cmp::Ordering::Less => a.next().filter(|_| mode != 0),
                        std::cmp::Ordering::Greater => b.next().filter(|_| mode == 1),
                        std::cmp::Ordering::Equal => {
                            b.next();
                            a.next().filter(|_| mode != 2)
                        }
                    };
                    if let Some(value) = value {
                        self.claim(value_cost(&value))?;
                        result.push(value);
                    }
                }
                self.live -= old_cost;
                Ok(QueryResult::Values(result))
            }
            _ => Err(EvalError::TypeMismatch),
        }
    }

    fn scalar_values(&mut self, result: QueryResult) -> Result<Vec<MemberValue>> {
        match result {
            QueryResult::Values(values) => Ok(values),
            QueryResult::Concepts(ordinals) => {
                self.tick(
                    ordinals
                        .len()
                        .saturating_mul(20)
                        .saturating_mul(ordinals.len().checked_ilog2().unwrap_or(0) as usize + 1),
                )?;
                let mut values = Vec::new();
                for &ordinal in &ordinals {
                    let value = MemberValue::Concept(self.store.ids[ordinal as usize].to_string());
                    self.claim(value_cost(&value))?;
                    values.push(value);
                }
                self.release(ordinals);
                values.sort_unstable();
                Ok(values)
            }
            QueryResult::Rows(_) => Err(EvalError::TypeMismatch),
        }
    }
}
