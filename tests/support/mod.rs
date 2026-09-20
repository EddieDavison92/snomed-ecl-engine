use snomed_ecl_engine::ecl::{
    AttributeValue, Comparison, Expr, Hierarchy, MemberFilter, MemberPredicate, MemberQuery,
    Refinement,
};
use snomed_ecl_engine::store::NumericStore;
use std::collections::{BTreeSet, VecDeque};

// Deliberately separate per-seed searches and BTreeSet operations from the production evaluator.
pub fn slow(store: &NumericStore, expr: &Expr) -> BTreeSet<u32> {
    match expr {
        Expr::Concept(id) => store.ordinal(*id).into_iter().collect(),
        Expr::All => (0..store.ids.len() as u32).collect(),
        Expr::Hierarchy(op, expr) => {
            let mut result = BTreeSet::new();
            for seed in slow(store, expr) {
                let mut visited = BTreeSet::from([seed]);
                let mut pending = VecDeque::from([seed]);
                if op.include_self() {
                    result.insert(seed);
                }
                while let Some(from) = pending.pop_front() {
                    for child in 0..store.ids.len() as u32 {
                        for &parent in store.parents.get(child) {
                            let (a, b) = if op.ancestors() {
                                (child, parent)
                            } else {
                                (parent, child)
                            };
                            if from == a {
                                result.insert(b);
                                if !op.direct() && visited.insert(b) {
                                    pending.push_back(b);
                                }
                            }
                        }
                    }
                }
            }
            result
        }
        Expr::And(parts) => parts
            .iter()
            .map(|e| slow(store, e))
            .reduce(|a, b| a.intersection(&b).copied().collect())
            .unwrap(),
        Expr::Or(parts) => parts.iter().flat_map(|e| slow(store, e)).collect(),
        Expr::Minus(a, b) => slow(store, a)
            .difference(&slow(store, b))
            .copied()
            .collect(),
        Expr::Extremum { top, inner } => {
            let seeds = slow(store, inner);
            let op = if *top {
                Hierarchy::Descendant
            } else {
                Hierarchy::Ancestor
            };
            seeds
                .difference(&slow(store, &Expr::Hierarchy(op, inner.clone())))
                .copied()
                .collect()
        }
        Expr::Refined(focus, refinement) => slow(store, focus)
            .into_iter()
            .filter(|&source| matches_refinement(store, refinement, source, None))
            .collect(),
        Expr::Dotted(focus, names) => {
            let mut values = slow(store, focus);
            for expr in names {
                let names = slow(store, expr);
                values = values
                    .into_iter()
                    .flat_map(|source| {
                        attributes(store, source)
                            .into_iter()
                            .filter(|(_, kind, _)| names.contains(kind))
                            .map(|(_, _, value)| value)
                    })
                    .collect();
            }
            values
        }
        Expr::MemberOf(inner) | Expr::RefsetContainingAny(inner) => {
            let values = slow(store, inner);
            let membership = store.membership.as_ref().unwrap();
            let mut result = BTreeSet::new();
            for (pos, &refset) in membership.refsets.iter().enumerate() {
                for &member in membership.get(pos) {
                    if matches!(expr, Expr::MemberOf(_)) && values.contains(&refset) {
                        result.insert(member);
                    }
                    if matches!(expr, Expr::RefsetContainingAny(_)) && values.contains(&member) {
                        result.insert(refset);
                    }
                }
            }
            result
        }
        Expr::Members(query) => members(store, query),
        Expr::History(..)
        | Expr::AlternateIdentifier { .. }
        | Expr::DialectAlias(..)
        | Expr::DescriptionFiltered(..)
        | Expr::ConceptFiltered(..) => panic!("Outside the graph/refinement fixture"),
    }
}

// Row-by-row member scan for concept-valued results. memberOf omits identifiers that name no
// concept of the substrate; projected fields are assumed resolvable here. Tuple and text
// predicates are out of scope.
fn members(store: &NumericStore, query: &MemberQuery) -> BTreeSet<u32> {
    use snomed_ecl_engine::store::MemberColumn as C;
    let candidates = slow(store, &query.source);
    let field = match &query.fields {
        None => "referencedComponentId".to_owned(),
        Some(fields) if fields.len() == 1 => fields[0].clone(),
        Some(_) => panic!("Tuple projections are outside the slow evaluator"),
    };
    let active_explicit = query.filters.iter().any(|f| f.field == "active");
    let mut result = BTreeSet::new();
    for refset in store.member_tables.refsets() {
        let refset_ordinal = store.ordinal(refset).unwrap();
        if !query.reverse && !candidates.contains(&refset_ordinal) {
            continue;
        }
        let table = store.member_tables.get(refset).unwrap().unwrap();
        let (Some(C::Id(projected)), Some(C::Boolean(active)), Some(C::Id(referenced))) = (
            table.column(&field),
            table.column("active"),
            table.column("referencedComponentId"),
        ) else {
            continue;
        };
        if query
            .filters
            .iter()
            .any(|f| table.column(&f.field).is_none())
        {
            continue;
        }
        for row in 0..table.len() {
            if !active_explicit && active[row] == 0 {
                continue;
            }
            if !query
                .filters
                .iter()
                .all(|f| member_predicate(store, table.column(&f.field).unwrap(), row, f))
            {
                continue;
            }
            if query.reverse {
                if store
                    .ordinal(referenced[row])
                    .is_some_and(|o| candidates.contains(&o))
                {
                    result.insert(refset_ordinal);
                }
            } else if let Some(ordinal) = store.ordinal(projected[row]) {
                result.insert(ordinal);
            }
        }
    }
    result
}

fn member_predicate(
    store: &NumericStore,
    column: &snomed_ecl_engine::store::MemberColumn,
    row: usize,
    filter: &MemberFilter,
) -> bool {
    use snomed_ecl_engine::decimal::Decimal;
    use snomed_ecl_engine::store::MemberColumn as C;
    let equal = filter.comparison == Comparison::Eq;
    match (column, &filter.value) {
        (C::Id(values), MemberPredicate::Concepts(expr)) => {
            let allowed = slow(store, expr);
            store
                .ordinal(values[row])
                .is_some_and(|o| allowed.contains(&o))
                == equal
        }
        (C::Integer(values), MemberPredicate::Number(target)) => filter.comparison.matches(
            Decimal::parse(&values[row].to_string())
                .unwrap()
                .cmp(target),
        ),
        (C::Number(values), MemberPredicate::Number(target)) => filter
            .comparison
            .matches(Decimal::parse(values.get(row)).unwrap().cmp(target)),
        (C::Boolean(values), MemberPredicate::Boolean(target)) => {
            target.is_none_or(|t| t == (values[row] != 0)) == equal
        }
        (
            C::Time(values),
            MemberPredicate::Dates(dates) | MemberPredicate::DatesOrText { dates, .. },
        ) => {
            let actual = (values[row] != 0).then_some(values[row]);
            let hit = dates.iter().any(|date| match filter.comparison {
                Comparison::Eq | Comparison::Ne => actual == *date,
                Comparison::Le | Comparison::Ge if actual.is_none() && date.is_none() => true,
                op => actual
                    .zip(*date)
                    .is_some_and(|(a, b)| op.matches(a.cmp(&b))),
            });
            hit == !matches!(filter.comparison, Comparison::Ne)
        }
        _ => panic!("Predicate type outside the slow evaluator"),
    }
}

fn attributes(store: &NumericStore, source: u32) -> Vec<(u32, u32, u32)> {
    let mut rows: Vec<_> = store
        .attributes
        .get(source)
        .iter()
        .map(|r| (r.group, r.kind, r.value))
        .collect();
    if let Some(isa) = store.ordinal(116680003) {
        rows.extend(store.parents.get(source).iter().map(|&p| (0, isa, p)));
    }
    rows
}

fn matches_refinement(
    store: &NumericStore,
    expr: &Refinement,
    source: u32,
    group: Option<u32>,
) -> bool {
    match expr {
        Refinement::Attribute(attribute) => {
            let names = slow(store, &attribute.name);
            let AttributeValue::Concepts(value) = &attribute.value else {
                panic!("Concrete values have separate exact-value fixtures")
            };
            let values = slow(store, value);
            let equal = attribute.comparison == Comparison::Eq;
            let count = if attribute.reverse {
                assert!(group.is_none());
                (0..store.ids.len() as u32)
                    .filter(|&candidate| {
                        values.contains(&candidate) == equal
                            && attributes(store, candidate)
                                .iter()
                                .any(|&(_, kind, target)| names.contains(&kind) && target == source)
                    })
                    .count()
            } else {
                attributes(store, source)
                    .iter()
                    .filter(|&&(g, kind, value)| {
                        group.is_none_or(|wanted| wanted == g)
                            && names.contains(&kind)
                            && values.contains(&value) == equal
                    })
                    .count()
            };
            count >= attribute.cardinality.min as usize
                && attribute
                    .cardinality
                    .max
                    .is_none_or(|max| count <= max as usize)
        }
        Refinement::Group(cardinality, inner) => {
            let groups: BTreeSet<_> = attributes(store, source)
                .iter()
                .map(|r| r.0)
                .filter(|&g| g != 0)
                .collect();
            let count = groups
                .into_iter()
                .filter(|&g| matches_refinement(store, inner, source, Some(g)))
                .count();
            count >= cardinality.min as usize
                && cardinality.max.is_none_or(|max| count <= max as usize)
        }
        Refinement::And(parts) => parts
            .iter()
            .all(|p| matches_refinement(store, p, source, group)),
        Refinement::Or(parts) => parts
            .iter()
            .any(|p| matches_refinement(store, p, source, group)),
    }
}
