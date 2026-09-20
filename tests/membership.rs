use snomed_rust_ecl_engine::ecl::{parse, ParseErrorKind};
use snomed_rust_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
use snomed_rust_ecl_engine::store::{Adjacency, Attributes, MembershipIndex, NumericStore};
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

fn fixture() -> NumericStore {
    let n = 12;
    let mut flags = vec![1; n];
    flags[2] = 0;
    flags[10] = 0;
    NumericStore {
        ids: (1000000..1000012).collect(),
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        flags,
        parents: Adjacency::build(n, vec![(1, 0), (9, 8)]).unwrap(),
        children: Adjacency::build(n, vec![(0, 1), (8, 9)]).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
        membership: Some(
            MembershipIndex::build(
                n,
                vec![(8, 0), (8, 0), (8, 1), (8, 2), (9, 1), (9, 3), (10, 4)],
            )
            .unwrap(),
        ),
    }
}

#[test]
fn membership_composes_with_hierarchy_sets_and_reverse_lookup() {
    let store = fixture();
    store.validate().unwrap();
    for (query, expected) in [
        ("^ 1000008", vec![0, 1, 2]),
        ("memberOf 1000008 |Synthetic refset|", vec![0, 1, 2]),
        ("^ (<< 1000008)", vec![0, 1, 2, 3]),
        ("^*", vec![0, 1, 2, 3, 4]),
        ("<< ^ 1000008", vec![0, 1, 2]),
        ("< (^ 1000008)", vec![1]),
        ("^ (1000008 OR 1000009) MINUS 1000001", vec![0, 2, 3]),
        ("^R 1000001", vec![8, 9]),
        ("refsetContainingAny (1000000 OR 1000003)", vec![8, 9]),
        ("^r 1000001", vec![8, 9]),
        ("^R*", vec![8, 9, 10]),
        ("^ 1000010", vec![4]),
        ("^R 1000004", vec![10]),
        ("^R 1000002", vec![8]),
        ("^ 9999999", vec![]),
        ("^ (1000008 MINUS 1000008)", vec![]),
    ] {
        assert_eq!(
            evaluate(&store, &parse(query).unwrap()).unwrap(),
            expected,
            "{query}"
        );
    }
}

#[test]
fn missing_index_errors_are_not_hidden_by_empty_boolean_operands() {
    let mut store = fixture();
    store.membership = None;
    for query in [
        "^ 1000008",
        "^R 1000001",
        "* OR (^ 1000008)",
        "9999999 AND (^ 1000008)",
        "^9999999",
    ] {
        assert!(
            matches!(
                evaluate(&store, &parse(query).unwrap()),
                Err(EvalError::Unsupported(_))
            ),
            "{query}"
        );
    }
    for query in [
        "^ [targetComponentId] 1000008",
        "^ 1000008 {{ M active = false }}",
    ] {
        assert_eq!(parse(query).unwrap_err().kind, ParseErrorKind::Unsupported);
    }
    for query in ["^", "^R", "^ < 1000008", "^R ^1000008", "^^1000008"] {
        assert!(parse(query).is_err(), "{query}");
    }
}

#[test]
fn membership_respects_work_memory_and_cancellation_limits() {
    let store = fixture();
    let expr = parse("^1000008").unwrap();
    assert_eq!(
        evaluate_with_limits(
            &store,
            &expr,
            Limits {
                max_work: 1,
                ..Limits::default()
            },
            None
        ),
        Err(EvalError::WorkLimit)
    );
    assert_eq!(
        evaluate_with_limits(
            &store,
            &expr,
            Limits {
                max_live_set_values: 2,
                ..Limits::default()
            },
            None
        ),
        Err(EvalError::MemoryLimit)
    );
    assert_eq!(
        evaluate_with_limits(
            &store,
            &expr,
            Limits::default(),
            Some(&AtomicBool::new(true))
        ),
        Err(EvalError::Cancelled)
    );
}

#[test]
fn generated_memberships_match_independent_pair_scan() {
    let n = 75;
    let pairs: Vec<_> = (60..n as u32)
        .flat_map(|r| (0..60).filter_map(move |m| ((r * 7 + m * 3) % 11 < 3).then_some((r, m))))
        .collect();
    let store = NumericStore {
        ids: (0..n).map(|i| 1000000 + i as u64).collect(),
        flags: vec![1; n],
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        parents: Adjacency::build(n, vec![]).unwrap(),
        children: Adjacency::build(n, vec![]).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
        membership: Some(MembershipIndex::build(n, pairs.clone()).unwrap()),
    };
    for start in 0..n as u32 {
        let selected = [start, (start + 7) % n as u32];
        for reverse in [false, true] {
            let expected: BTreeSet<_> = pairs
                .iter()
                .filter_map(|&(r, m)| {
                    let (key, value) = if reverse { (m, r) } else { (r, m) };
                    selected.contains(&key).then_some(value)
                })
                .collect();
            let query = format!(
                "{} ({} OR {})",
                if reverse { "^R" } else { "^" },
                1000000 + selected[0],
                1000000 + selected[1]
            );
            assert_eq!(
                evaluate(&store, &parse(&query).unwrap()).unwrap(),
                expected.into_iter().collect::<Vec<_>>(),
                "{query}"
            );
        }
    }
}
