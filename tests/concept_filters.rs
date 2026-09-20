use snomed_ecl_engine::ecl::{parse, Comparison, ConceptFilter, Expr, ParseErrorKind};
use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
use snomed_ecl_engine::store::{Adjacency, Attributes, MembershipIndex, NumericStore};
use std::sync::atomic::AtomicBool;

fn fixture() -> NumericStore {
    let ids = vec![
        1000001,
        1000002,
        1000003,
        1000004,
        1000005,
        1000006,
        2000001,
        2000002,
        900000000000073002,
        900000000000074008,
        900000000000444006,
    ];
    let n = ids.len();
    let edges = vec![(1, 0), (2, 1), (4, 0), (5, 0), (7, 6), (8, 10), (9, 10)];
    NumericStore {
        descriptions: Default::default(),
        ids,
        flags: vec![1, 3, 1, 2, 3, 1, 1, 1, 1, 1, 1],
        modules: vec![6, 6, 7, 7, 7, 6, 6, 6, 6, 6, 6],
        effective_times: vec![
            20200131, 20210131, 20240229, 20260717, 0, 20210131, 20260826, 20260826, 20260826,
            20260826, 20260826,
        ],
        parents: Adjacency::build(n, edges.clone()).unwrap(),
        children: Adjacency::build(n, edges.iter().map(|&(a, b)| (b, a)).collect()).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
        membership: Some(MembershipIndex::build(n, vec![(6, 0), (6, 1), (6, 3)]).unwrap()),
    }
}

#[test]
fn concept_metadata_filters_compose_after_hierarchy_membership_and_extrema() {
    let store = fixture();
    store.validate().unwrap();
    for (query, expected) in [
        ("<1000001 {{ C definitionStatus = defined }}", vec![1, 4]),
        (
            "<<1000001 {{c definitionStatus != (primitive)}}",
            vec![1, 4],
        ),
        (
            "<<1000001 {{C definitionStatusId = 900000000000074008}}",
            vec![0, 2, 5],
        ),
        (
            "<<1000001 {{C definitionStatusId = (<900000000000444006)}}",
            vec![0, 1, 2, 4, 5],
        ),
        (
            "<<1000001 {{C definitionStatusId != (900000000000074008 900000000000073002)}}",
            vec![],
        ),
        (
            "<<1000001 {{C definitionStatus = (primitive defined)}}",
            vec![0, 1, 2, 4, 5],
        ),
        ("<<1000001 {{C moduleId = 2000002}}", vec![2, 4]),
        ("<<1000001 {{C moduleId = <2000001}}", vec![2, 4]),
        (
            "<<1000001 {{C moduleId = (2000001 |First| 2000002 |Second|)}}",
            vec![0, 1, 2, 4, 5],
        ),
        (
            "<<1000001 {{C moduleId = (2000001 OR 2000002)}}",
            vec![0, 1, 2, 4, 5],
        ),
        (
            "<<1000001 {{C moduleId != 2000001, definitionStatus = primitive}}",
            vec![2],
        ),
        ("^2000001 {{Cactive=0}}", vec![3]),
        (
            "^2000001 {{c active = false}} {{C definitionStatus = defined}}",
            vec![3],
        ),
        ("^2000001 {{C active != false}}", vec![0, 1]),
        ("^2000001 {{C active = *}}", vec![0, 1, 3]),
        ("^2000001 {{C active != ANY}}", vec![]),
        (
            "!!> (1000001 OR 1000002) {{C definitionStatus=defined}}",
            vec![],
        ),
        (
            "!!> ((1000001 OR 1000002) {{C definitionStatus=defined}})",
            vec![1],
        ),
        (
            "(^2000001 {{C active=false}}) OR (1000001 {{C active=1}})",
            vec![0, 3],
        ),
        (
            "<<1000001 {{C moduleId = (2000001 {{C active=true}})}}",
            vec![0, 1, 5],
        ),
    ] {
        assert_eq!(
            evaluate(&store, &parse(query).unwrap()).unwrap(),
            expected,
            "{query}"
        );
    }
}

#[test]
fn dates_support_sets_empty_values_and_calendar_boundaries() {
    let store = fixture();
    for (predicate, expected) in [
        ("= \"20210131\"", vec![1, 5]),
        ("!= (\"20210131\" \"20240229\")", vec![0, 4]),
        (">= \"20210131\"", vec![1, 2, 5]),
        ("< \"20210131\"", vec![0]),
        ("<= \"20210131\"", vec![0, 1, 5]),
        ("> (\"20210131\" \"20240229\")", vec![2]),
        ("= \"\"", vec![4]),
        ("!= \"\"", vec![0, 1, 2, 5]),
        ("< \"\"", vec![]),
        ("> \"\"", vec![]),
        (">= \"\"", vec![4]),
    ] {
        let query = format!("<<1000001 {{{{ C effectiveTime {predicate} }}}}");
        assert_eq!(
            evaluate(&store, &parse(&query).unwrap()).unwrap(),
            expected,
            "{query}"
        );
    }
    for query in [
        "* {{C effectiveTime = \"20230229\"}}",
        "* {{C effectiveTime = \"20260431\"}}",
        "* {{C effectiveTime = \"20260001\"}}",
        "* {{C effectiveTime = \"20260900\"}}",
        "* {{C effectiveTime = \"02609001\"}}",
        "* {{C effectiveTime = (\"20260101\"\"20260102\")}}",
        "* {{C moduleId = (2000001|x|2000002)}}",
        "* {{C active = 10}}",
        "* {{C active > 0}}",
        "* {{C definitionStatus = ()}}",
        "* {{C moduleId = ()}}",
        "* {{C effectiveTime=()}}",
        "* {{C active=true,}}",
        "* {{C unknown=1}}",
    ] {
        assert!(parse(query).is_err(), "{query}");
    }
}

#[test]
fn filters_keep_errors_visible_and_respect_limits() {
    let mut store = fixture();
    store.membership = None;
    let expression = parse("9999999 {{C moduleId = (^2000001)}}").unwrap();
    assert!(matches!(
        evaluate(&store, &expression),
        Err(EvalError::Unsupported(_))
    ));
    let expression = parse("* {{C active=false, moduleId = (^2000001)}}").unwrap();
    assert!(matches!(
        evaluate(&store, &expression),
        Err(EvalError::Unsupported(_))
    ));
    for query in ["^2000001 {{M active=0}}", "* {{+ HISTORY}}"] {
        assert_eq!(parse(query).unwrap_err().kind, ParseErrorKind::Unsupported);
    }
    let expression = parse("* {{C active=1}}").unwrap();
    assert_eq!(
        evaluate_with_limits(
            &store,
            &expression,
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
            &expression,
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
            &expression,
            Limits::default(),
            Some(&AtomicBool::new(true))
        ),
        Err(EvalError::Cancelled)
    );
    assert_eq!(
        evaluate(&store, &Expr::ConceptFiltered(Box::new(Expr::All), vec![])),
        Err(EvalError::InvalidAst)
    );
    assert_eq!(
        evaluate(
            &store,
            &Expr::ConceptFiltered(
                Box::new(Expr::All),
                vec![ConceptFilter::Active(Comparison::Lt, Some(true))]
            )
        ),
        Err(EvalError::InvalidAst)
    );
}

#[test]
fn generated_module_and_status_filters_match_independent_metadata_scan() {
    let store = fixture();
    for module in [2000001, 2000002, 9999999] {
        for equal in [false, true] {
            for defined in [false, true] {
                let wanted: Vec<_> = store
                    .ids
                    .iter()
                    .enumerate()
                    .filter_map(|(i, _)| {
                        let module_match = store.ids[store.modules[i] as usize] == module;
                        (module_match == equal && (store.flags[i] & 2 != 0) == defined)
                            .then_some(i as u32)
                    })
                    .collect();
                let query = format!(
                    "* {{{{ C moduleId {} {module}, definitionStatus = {} }}}}",
                    if equal { "=" } else { "!=" },
                    if defined { "defined" } else { "primitive" }
                );
                assert_eq!(
                    evaluate(&store, &parse(&query).unwrap()).unwrap(),
                    wanted,
                    "{query}"
                );
            }
        }
    }
}
