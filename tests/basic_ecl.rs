use snomed_rust_ecl_engine::ecl::{parse, Expr, Hierarchy, ParseErrorKind};
use snomed_rust_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
use snomed_rust_ecl_engine::store::{Adjacency, Attributes, NumericStore};
use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::AtomicBool;

fn store(n: usize) -> NumericStore {
    let pairs: Vec<_> = (1..n - 1)
        .flat_map(|child| {
            (0..child)
                .filter(move |parent| *parent == child - 1 || (child * 19 + parent * 7) % 11 < 2)
                .map(move |parent| (child as u32, parent as u32))
        })
        .collect();
    let mut flags = vec![1; n];
    flags[n - 1] = 0;
    NumericStore {
        ids: (0..n).map(|i| 1000001 + i as u64).collect(),
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        flags,
        parents: Adjacency::build(n, pairs.clone()).unwrap(),
        children: Adjacency::build(n, pairs.iter().map(|&(a, b)| (b, a)).collect()).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
    }
}

// Deliberately separate per-seed searches and BTreeSet operations from the production evaluator.
fn slow(store: &NumericStore, expr: &Expr) -> BTreeSet<u32> {
    match expr {
        Expr::Concept(id) => store
            .ordinal(*id)
            .filter(|&i| store.is_active(i))
            .into_iter()
            .collect(),
        Expr::All => (0..store.ids.len() as u32)
            .filter(|&i| store.is_active(i))
            .collect(),
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
        Expr::Refined(..) | Expr::Dotted(..) => panic!("Outside the basic hierarchy fixture"),
    }
}

#[test]
fn brief_long_terms_comments_and_boolean_grouping() {
    let equivalent = [
        ("1000001", "1000001 |term /* explanation | */ |"),
        ("1000001", "1000001 |/* literal */|"),
        ("<<195967001", "descendantOrSelfOf 195967001 |Asthma|"),
        ("<!195967001", "CHILDOF/* note */195967001"),
        (">>!195967001", "parentOrSelfOf 195967001"),
        ("*", "aNy"),
        ("1000001,1000002 AND 1000003", "1000001 AND 1000002,1000003"),
        (
            "< (1000001 OR 1000002)",
            "descendantOf (1000001 or 1000002)",
        ),
        (
            "1000001 |Synthetic café|",
            "/* leading */ 1000001 /* trailing */",
        ),
    ];
    for (a, b) in equivalent {
        assert_eq!(parse(a).unwrap(), parse(b).unwrap(), "{a}");
    }
    for bad in [
        "",
        "1000001 OR",
        "(1000001",
        "1000001)",
        "< <1000001",
        "1000001 1000002",
        "12345",
        "0123456",
        "1234567890123456789",
        "1000001 | |",
        "1000001 |bad\tterm|",
        "1000001 |unclosed",
        "/* unclosed",
        "1000001 OR(1000002)",
        "1000001 AND1000002",
        "childOf(1000001)",
        "1000001 AND 1000002 OR 1000003",
        "1000001 MINUS 1000002 MINUS 1000003",
        "1000001 OR 1000002 MINUS 1000003",
    ] {
        assert_eq!(
            parse(bad).unwrap_err().kind,
            ParseErrorKind::Syntax,
            "{bad}"
        );
    }
    assert!(parse("1000001 AND (1000002 OR 1000003)").is_ok());
    assert!(parse("1000001 MINUS (1000002 MINUS 1000003)").is_ok());
}

#[test]
fn unsupported_features_never_become_partial_success() {
    for query in [
        "* OR (^ 1000001)",
        "* {{ C active = false }}",
        "* {{ +HISTORY }}",
        "^R 1000001",
        "scheme#code",
        "^ [targetComponentId] 1000001",
    ] {
        assert_eq!(
            parse(query).unwrap_err().kind,
            ParseErrorKind::Unsupported,
            "{query}"
        );
    }
}

#[test]
fn all_hierarchy_operators_match_independent_evaluator_for_overlapping_seeds() {
    let store = store(24);
    store.validate().unwrap();
    for op in ["<", "<<", "<!", "<<!", ">", ">>", ">!", ">>!"] {
        for seed in [
            "1000001",
            "1000010",
            "1000024",
            "9999999",
            "*",
            "(1000001 OR 1000010)",
            "(1000001 AND 1000010)",
            "(<1000001 MINUS <1000010)",
        ] {
            let query = format!("{op} {seed}");
            let expr = parse(&query).unwrap();
            assert_eq!(
                evaluate(&store, &expr).unwrap(),
                slow(&store, &expr).into_iter().collect::<Vec<_>>(),
                "{query}"
            );
        }
    }
    // Strict descendants of a union can contain one of the input concepts.
    assert!(evaluate(&store, &parse("< (1000001 OR 1000010)").unwrap())
        .unwrap()
        .contains(&9));
}

#[test]
fn generated_boolean_expressions_match_slow_sets() {
    let store = store(24);
    for seed in 0..80u64 {
        let a = Expr::Hierarchy(
            Hierarchy::DescendantOrSelf,
            Box::new(Expr::Concept(1000001 + seed % 24)),
        );
        let b = Expr::Hierarchy(
            Hierarchy::Ancestor,
            Box::new(Expr::Concept(1000001 + seed * 7 % 24)),
        );
        let c = Expr::Concept(1000001 + seed * 11 % 24);
        for expr in [
            Expr::And(vec![a.clone(), b.clone()]),
            Expr::Or(vec![a.clone(), b.clone(), c.clone()]),
            Expr::Minus(Box::new(a.clone()), Box::new(b.clone())),
            Expr::Minus(
                Box::new(Expr::All),
                Box::new(Expr::Or(vec![a, Expr::And(vec![b, c])])),
            ),
        ] {
            assert_eq!(
                evaluate(&store, &expr).unwrap(),
                slow(&store, &expr).into_iter().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn limits_and_cancellation_fail_without_results() {
    assert_eq!(
        parse(&format!("{}1000001{}", "(".repeat(65), ")".repeat(65)))
            .unwrap_err()
            .kind,
        ParseErrorKind::Limit
    );
    assert_eq!(
        parse(&" ".repeat(65537)).unwrap_err().kind,
        ParseErrorKind::Limit
    );
    let store = store(24);
    let expr = parse("< * OR > *").unwrap();
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
                max_live_set_values: 1,
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
    assert_eq!(
        evaluate(&store, &Expr::And(vec![])),
        Err(EvalError::InvalidAst)
    );
}
