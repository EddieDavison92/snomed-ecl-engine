//! Subsumption must cost what it touches, not the size of the edition.
//!
//! A code list turned into ECL is a union of many small hierarchies. When each
//! operator paid for every concept in the store, a real edition allowed about
//! forty of them per expression before the work budget ran out. These tests use
//! a store far larger than any answer and a budget far smaller than the store,
//! so an implementation that scans the edition fails them.

use snomed_ecl_engine::ecl::parse;
use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, Limits};
use snomed_ecl_engine::store::{Adjacency, Attributes, NumericStore};
use std::collections::BTreeSet;

const BASE: u64 = 1_000_001;

/// A root with many small, separate trees under it: `branches` subtrees of
/// `size` concepts each, every one a chain of children.
fn forest(branches: usize, size: usize) -> NumericStore {
    let n = 1 + branches * size;
    let mut pairs = Vec::new();
    for branch in 0..branches {
        let first = 1 + branch * size;
        pairs.push((first as u32, 0));
        for step in 1..size {
            pairs.push(((first + step) as u32, (first + step - 1) as u32));
        }
    }
    let store = NumericStore {
        descriptions: Default::default(),
        search: Default::default(),
        history: Default::default(),
        member_tables: Default::default(),
        identifiers: Default::default(),
        config: Default::default(),
        ids: (0..n).map(|i| BASE + i as u64).collect(),
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        flags: vec![1; n],
        parents: Adjacency::build(n, pairs.clone()).unwrap(),
        children: Adjacency::build(n, pairs.iter().map(|&(c, p)| (p, c)).collect()).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
        membership: None,
    };
    store.validate().unwrap();
    store
}

fn code(ordinal: usize) -> u64 {
    BASE + ordinal as u64
}

#[test]
fn a_small_subsumption_does_not_pay_for_the_whole_edition() {
    // 200,000 concepts, and a budget of a thousandth of that.
    let store = forest(20_000, 10);
    let limits = Limits {
        max_work: 200,
        max_live_set_values: 1_000,
    };
    let leaf = parse(&format!("<< {}", code(5))).unwrap();
    let answer = evaluate_with_limits(&store, &leaf, limits, None)
        .expect("a ten-concept subtree fits a budget of 200");
    assert_eq!(answer.len(), 6, "the concept and the five below it");

    let limits = Limits {
        max_work: 200,
        max_live_set_values: 1_000,
    };
    let up = parse(&format!(">> {}", code(10))).unwrap();
    let answer = evaluate_with_limits(&store, &up, limits, None).expect("ancestors are cheap too");
    assert_eq!(answer.len(), 11, "ten in the chain plus the root");
}

#[test]
fn a_union_of_many_subsumptions_fits_where_forty_used_to_be_the_limit() {
    let store = forest(20_000, 10);
    // Four hundred separate subtrees in one expression, the shape a code list
    // produces. Against the edition-sized cost this ran out after about forty.
    let terms: Vec<String> = (0..400).map(|b| format!("<< {}", code(1 + b * 10))).collect();
    let union = parse(&terms.join(" OR ")).unwrap();
    let limits = Limits {
        max_work: 50_000,
        max_live_set_values: 20_000,
    };
    let answer = evaluate_with_limits(&store, &union, limits, None)
        .expect("four hundred ten-concept subtrees fit comfortably");
    assert_eq!(answer.len(), 4_000);

    // And it is the same set as asking for each subtree on its own.
    let mut expected = BTreeSet::new();
    for term in &terms {
        expected.extend(evaluate(&store, &parse(term).unwrap()).unwrap());
    }
    assert_eq!(answer.into_iter().collect::<BTreeSet<_>>(), expected);
}

#[test]
fn a_large_answer_is_still_complete_and_ordered() {
    // Large enough to take the path that reads markers back rather than sorting.
    let store = forest(2_000, 10);
    let everything = evaluate(&store, &parse(&format!("<< {}", code(0))).unwrap()).unwrap();
    assert_eq!(everything.len(), 20_001);
    assert!(everything.windows(2).all(|w| w[0] < w[1]), "sorted and unique");

    let below = evaluate(&store, &parse(&format!("< {}", code(0))).unwrap()).unwrap();
    assert_eq!(below.len(), 20_000);
    assert!(!below.contains(&0), "the root is excluded from its own descendants");
}

#[test]
fn repeated_traversals_in_one_query_do_not_leak_into_each_other() {
    // Hundreds of operators in one expression reuse the same markers under
    // successive stamps, and wrap past the stamp's range. A stale marker would
    // surface as a concept that belongs to a different operator's answer.
    let store = forest(600, 3);
    let terms: Vec<String> = (0..600).map(|b| format!("<< {}", code(1 + b * 3))).collect();
    let expression = parse(&format!(
        "({}) AND ({})",
        terms.join(" OR "),
        terms[..300].join(" OR ")
    ))
    .unwrap();
    let answer = evaluate(&store, &expression).unwrap();
    assert_eq!(answer.len(), 900, "the first three hundred subtrees of three");
}
