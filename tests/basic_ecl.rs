use snomed_ecl_engine::ecl::{parse, Expr, Hierarchy, ParseErrorKind};
use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
use snomed_ecl_engine::store::{Adjacency, Attributes, NumericStore};
use std::collections::BTreeSet;
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
        descriptions: Default::default(),
        member_tables: Default::default(),
        identifiers: Default::default(),
        config: Default::default(),
        ids: (0..n).map(|i| 1000001 + i as u64).collect(),
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        flags,
        parents: Adjacency::build(n, pairs.clone()).unwrap(),
        children: Adjacency::build(n, pairs.iter().map(|&(a, b)| (b, a)).collect()).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
        membership: None,
    }
}

mod support;
use support::slow;

#[test]
fn generated_refinements_groups_cardinalities_and_membership_match_slow_scans() {
    use snomed_ecl_engine::store::{Attribute, MembershipIndex};
    let mut store = store(13);
    store.attributes = Attributes::build(
        13,
        (0..12u32)
            .flat_map(|source| {
                (0..4u32).flat_map(move |group| {
                    (8..11u32).filter_map(move |kind| {
                        ((source * 17 + group * 11 + kind) % 5 < 3).then_some((
                            source,
                            Attribute {
                                group,
                                kind,
                                value: (source * 3 + kind + group) % 8,
                            },
                        ))
                    })
                })
            })
            .collect(),
    )
    .unwrap();
    store.membership = Some(
        MembershipIndex::build(
            13,
            (9..12u32)
                .flat_map(|refset| {
                    (0..13u32).filter_map(move |member| {
                        ((refset * 13 + member * 7) % 5 < 2).then_some((refset, member))
                    })
                })
                .collect(),
        )
        .unwrap(),
    );
    store.validate().unwrap();
    let concept = |i: usize| match i % 7 {
        0 => "*".to_owned(),
        1 => format!("<< {}", 1000001 + i % 8),
        2 => format!("> {}", 1000001 + i % 8),
        3 => format!("^ {}", 1000010 + i % 3),
        4 => format!("^R {}", 1000001 + i % 13),
        _ => (1000001 + i % 13).to_string(),
    };
    for i in 0..1000usize {
        let name = if i % 4 == 0 {
            "*".into()
        } else {
            (1000009 + i % 3).to_string()
        };
        let min = i % 3;
        let max = if i % 5 == 0 {
            "*".into()
        } else {
            (min + i % 4).to_string()
        };
        let comparison = if i % 2 == 0 { "=" } else { "!=" };
        let reverse = if i % 9 == 0 { "R " } else { "" };
        let a = format!(
            "[{min}..{max}] {reverse}{name} {comparison} {}",
            concept(i / 3)
        );
        let b = format!("{} = {}", 1000009 + i % 3, concept(i / 7));
        let refinement = match i % 4 {
            0 => a,
            1 => format!("({a}) OR ({b})"),
            2 => format!("[0..2] {{ {b} }} AND ({a})"),
            _ => format!("{{ ({b}) OR ([0..0] 1000009 = {}) }}", concept(i / 11)),
        };
        let query = format!(
            "({} : {refinement}) OR ({} MINUS {})",
            concept(i / 13),
            concept(i / 17),
            concept(i / 19)
        );
        let expression = parse(&query).unwrap();
        let actual: BTreeSet<_> = evaluate(&store, &expression).unwrap().into_iter().collect();
        assert_eq!(actual, slow(&store, &expression), "{query}");
    }
}

#[test]
fn default_substrate_includes_inactive_concepts_but_only_active_edges() {
    let store = store(4);
    for (query, expected) in [
        ("*", vec![0, 1, 2, 3]),
        ("1000004", vec![3]),
        ("<<1000004", vec![3]),
        ("<1000004", vec![]),
        ("* MINUS (<<1000001)", vec![3]),
        ("1000004 : [0..0] 1000001 = *", vec![3]),
    ] {
        assert_eq!(
            evaluate(&store, &parse(query).unwrap()).unwrap(),
            expected,
            "{query}"
        );
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
            "1000001 |Synthetic cafÃ©|",
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
    assert_eq!(
        parse("unknown#code")
            .map(|e| evaluate(&NumericStore::default(), &e))
            .unwrap()
            .unwrap_err(),
        snomed_ecl_engine::eval::EvalError::UnconfiguredAlias("unknown".into())
    );
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
