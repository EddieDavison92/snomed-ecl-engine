use snomed_ecl_engine::ecl::parse;
use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
use snomed_ecl_engine::store::{Adjacency, Attributes, MembershipIndex, NumericStore};
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

// Ordinal 11 (1011000) is a description-based reference set with no concept members.
// Ordinal 7 (1007000) is a concept-based reference set whose rows are all inactive, so it is
// in the memberOf domain although the active index holds nothing for it. Ordinal 10 is an
// inactive concept whose refset has active members.
fn fixture() -> NumericStore {
    let n = 12;
    let mut flags = vec![1; n];
    flags[2] = 0;
    flags[10] = 0;
    let mut membership = MembershipIndex::build(
        n,
        vec![(8, 0), (8, 0), (8, 1), (8, 2), (9, 1), (9, 3), (10, 4)],
    )
    .unwrap();
    membership.concept_refsets = Some(vec![1007000, 1008000, 1009000, 1010000]);
    membership.non_concept_refsets = Some(vec![1011000]);
    NumericStore {
        descriptions: Default::default(),
        member_tables: Default::default(),
        identifiers: Default::default(),
        config: Default::default(),
        ids: (0..12).map(|i| 1000000 + i * 1000).collect(),
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        flags,
        parents: Adjacency::build(n, vec![(1, 0), (9, 8)]).unwrap(),
        children: Adjacency::build(n, vec![(0, 1), (8, 9)]).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
        membership: Some(membership),
    }
}

#[test]
fn membership_composes_with_hierarchy_sets_and_reverse_lookup() {
    let store = fixture();
    store.validate().unwrap();
    for (query, expected) in [
        ("^ 1008000", vec![0, 1, 2]),
        ("memberOf 1008000 |Synthetic refset|", vec![0, 1, 2]),
        ("^ (<< 1008000)", vec![0, 1, 2, 3]),
        ("^*", vec![0, 1, 2, 3, 4]),
        ("<< ^ 1008000", vec![0, 1, 2]),
        ("< (^ 1008000)", vec![1]),
        ("^ (1008000 OR 1009000) MINUS 1001000", vec![0, 2, 3]),
        ("^R 1001000", vec![8, 9]),
        ("refsetContainingAny (1000000 OR 1003000)", vec![8, 9]),
        ("^r 1001000", vec![8, 9]),
        ("REFSETCONTAININGANY 1001000", vec![8, 9]),
        ("^R*", vec![8, 9, 10]),
        ("^ 1010000", vec![4]),
        ("^R 1004000", vec![10]),
        ("^R 1002000", vec![8]),
        ("^ 9990000", vec![]),
        ("^ (1008000 MINUS 1008000)", vec![]),
        // Mixed selections keep the concept members of concept-based reference sets.
        ("^ (1011000 OR 1008000)", vec![0, 1, 2]),
        ("^ (>> 1011000 OR 1010000)", vec![4]),
        ("^ (* MINUS 1008000)", vec![1, 3, 4]),
        // A concept-based set with no active members is still in the domain.
        ("^ 1007000", vec![]),
        ("^ (1011000 OR 1007000)", vec![]),
        ("^R 1011000", vec![]),
        ("^R (1011000 OR 1000000)", vec![8]),
    ] {
        assert_eq!(
            evaluate(&store, &parse(query).unwrap()).unwrap(),
            expected,
            "{query}"
        );
    }
}

#[test]
fn member_of_a_description_based_reference_set_is_a_semantic_error_not_an_empty_set() {
    let store = fixture();
    for query in [
        "^ 1011000",
        "memberOf 1011000 |Synthetic language refset|",
        "<< (^ 1011000)",
        "(^ 1011000) OR 1000000",
        "1000000 MINUS (^ 1011000)",
        "^ (1011000 OR 9990000)",
        "^ (1011000 OR 1006000)",
        "^ (1011000 MINUS 1008000)",
        "^ (<< 1011000)",
        "* : 1005000 = (^ 1011000)",
    ] {
        assert!(
            matches!(
                evaluate(&store, &parse(query).unwrap()),
                Err(EvalError::Semantic(message)) if message.contains("1011000")
            ),
            "{query}"
        );
    }
    // Indexes built before the classification existed keep returning the empty set.
    let mut legacy = fixture();
    let index = legacy.membership.as_mut().unwrap();
    index.concept_refsets = None;
    index.non_concept_refsets = None;
    legacy.validate().unwrap();
    assert!(evaluate(&legacy, &parse("^ 1011000").unwrap())
        .unwrap()
        .is_empty());
    for (concept, non_concept) in [
        (Some(vec![1008000]), Some(vec![1011000, 1011000])),
        (Some(vec![1008000, 1011000]), Some(vec![1011000])),
        (None, Some(vec![1011000])),
        (Some(vec![1009000, 1008000]), Some(vec![])),
    ] {
        let mut invalid = fixture();
        let index = invalid.membership.as_mut().unwrap();
        index.concept_refsets = concept;
        index.non_concept_refsets = non_concept;
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn missing_index_errors_are_not_hidden_by_empty_boolean_operands() {
    let mut store = fixture();
    store.membership = None;
    for query in [
        "^ 1008000",
        "^R 1001000",
        "* OR (^ 1008000)",
        "9990000 AND (^ 1008000)",
        "^9990000",
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
        "^ [targetComponentId] 1008000",
        "^ 1008000 {{ M active = false }}",
    ] {
        assert!(matches!(
            evaluate(&store, &parse(query).unwrap()),
            Err(EvalError::Unsupported(_))
        ));
    }
    for query in ["^", "^R", "^ < 1008000", "^R ^1008000", "^^1008000"] {
        assert!(parse(query).is_err(), "{query}");
    }
}

#[test]
fn membership_respects_work_memory_and_cancellation_limits() {
    let store = fixture();
    let expr = parse("^1008000").unwrap();
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
        descriptions: Default::default(),
        member_tables: Default::default(),
        identifiers: Default::default(),
        config: Default::default(),
        ids: (0..n).map(|i| 1000000 + (i as u64) * 1000).collect(),
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
                1000000 + (selected[0]) * 1000,
                1000000 + (selected[1]) * 1000
            );
            assert_eq!(
                evaluate(&store, &parse(&query).unwrap()).unwrap(),
                expected.into_iter().collect::<Vec<_>>(),
                "{query}"
            );
        }
    }
}
