use snomed_ecl_engine::decimal::Decimal;
use snomed_ecl_engine::ecl::{parse, ParseErrorKind};
use snomed_ecl_engine::eval::{evaluate, EvalError};
use snomed_ecl_engine::store::{Adjacency, Attribute, Attributes, ConcreteValue, NumericStore};

// Sources 0..4, attribute types 5..7, values 8..10, is-a 11.
fn fixture() -> NumericStore {
    let n = 12;
    let rows = [
        (0, 1, 5, 8),
        (0, 1, 6, 9),
        (1, 1, 5, 8),
        (1, 2, 6, 9),
        (2, 0, 5, 8),
        (2, 0, 6, 9),
        (3, 1, 5, 8),
        (3, 2, 5, 8),
        (3, 2, 6, 10),
    ];
    let concrete = [
        (0, 1, 7, 0),
        (1, 1, 7, 1),
        (2, 0, 7, 2),
        (3, 2, 7, 3),
        (4, 0, 7, 4),
    ];
    let build = |rows: &[(u32, u32, u32, u32)]| {
        Attributes::build(
            n,
            rows.iter()
                .map(|&(source, group, kind, value)| (source, Attribute { group, kind, value }))
                .collect(),
        )
        .unwrap()
    };
    NumericStore {
        descriptions: Default::default(),
        search: Default::default(),
        history: Default::default(),
        member_tables: Default::default(),
        identifiers: Default::default(),
        config: Default::default(),
        ids: (0..11)
            .map(|i| 1000000 + i * 1000)
            .chain([116680003])
            .collect(),
        flags: vec![1; n],
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        parents: Adjacency::build(n, vec![(9, 8), (10, 9)]).unwrap(),
        children: Adjacency::build(n, vec![(8, 9), (9, 10)]).unwrap(),
        attributes: build(&rows),
        concrete: build(&concrete),
        concrete_values: vec![
            ConcreteValue::Number("#0.10000000000000000001".into()),
            ConcreteValue::Number("#0.1".into()),
            ConcreteValue::Number("#-2".into()),
            ConcreteValue::Text("\"A\\\"B\"".into()),
            ConcreteValue::Boolean(true),
        ],
        membership: None,
    }
}
fn assert_query(query: &str, expected: &[u32]) {
    assert_eq!(
        evaluate(
            &fixture(),
            &parse(query).unwrap_or_else(|e| panic!("{query}: {e}"))
        )
        .unwrap(),
        expected,
        "{query}"
    );
}

#[test]
fn groups_do_not_cross_match_or_include_group_zero() {
    assert_query("* : 1005000 = 1008000, 1006000 = 1009000", &[0, 1, 2]);
    assert_query("* : { 1005000 = 1008000, 1006000 = 1009000 }", &[0]);
    assert_query("* : [2..2] { 1005000 = 1008000 }", &[3]);
    assert_query("* : { [0..0] 1006000 = * }", &[1, 3]);
    assert_query(
        "* : { (1005000 = 1008000 OR 1006000 = 1010000), 1006000 = * }",
        &[0, 3],
    );
}

#[test]
fn cardinality_and_inequality_preserve_absence_and_reverse_identity() {
    assert_query("* : [2..*] 1005000 = 1008000", &[3]);
    assert_query("(1000000 OR 1004000) : [0..0] 1005000 = *", &[4]);
    assert_query("* : 1006000 != 1009000", &[3]);
    assert_query("* : [4..4] R 1005000 = *", &[8]);
    assert_query("* : [4..4] r 1005000 = *", &[8]);
    assert_query("* : [4..4] r1005000 = *", &[8]);
    assert_query("* : [5..5] R 1005000 = *", &[]);
    assert_query("* : R 1005000 != 1000000", &[8]);
    assert_query("* : 116680003 = 1008000", &[9]);
    assert_query("* : R 116680003 = 1010000", &[9]);
}

#[test]
fn cardinality_bounds_beyond_machine_integers_preserve_finite_store_semantics() {
    for bound in [
        "4294967296",
        "18446744073709551616",
        "99900009990000999000099900009990000999999",
    ] {
        assert_query(&format!("* : [1..{bound}] 1005000 = *"), &[0, 1, 2, 3]);
        assert_query(&format!("* : [{bound}..*] 1005000 = *"), &[]);
        assert_query(&format!("* : [1..{bound}] {{ 1005000 = * }}"), &[0, 1, 3]);
    }
    for query in [
        "* : [18446744073709551617..18446744073709551616] 1005000 = *",
        "* : [18446744073709551616..4294967296] 1005000 = *",
    ] {
        assert_eq!(parse(query).unwrap_err().kind, ParseErrorKind::Syntax);
    }
}

#[test]
fn concrete_values_compare_exactly_and_keep_their_types() {
    assert_query("* : 1007000 > #0.1", &[0]);
    assert_query("* : 1007000 = #+0.100", &[1]);
    assert_query("* : 1007000 < #-1.999000099900009990000", &[2]);
    assert_query("* : 1007000 != #0.1", &[0, 2]);
    assert_query("* : 1007000 = \"A\\\"B\"", &[3]);
    assert_query("* : 1007000 = (\"other\" \"A\\\"B\")", &[3]);
    assert_query("* : 1007000 = \"a\\\"b\"", &[]);
    assert_query("* : 1007000 = \"A\"", &[]);
    assert_query("* : 1007000 = \"*B\"", &[]);
    assert_query("* : 1007000 != \"A\"", &[3]);
    assert_query("* : 1007000 = true", &[4]);
    assert_query("* : 1007000 != false", &[4]);
    assert_query("* : { 1007000 = true }", &[]);
}

#[test]
fn concrete_inequality_matches_a_different_value_even_when_an_equal_value_exists() {
    let mut store = fixture();
    store.concrete_values = vec![
        ConcreteValue::Number("#10".into()),
        ConcreteValue::Number("#20".into()),
    ];
    store.concrete = Attributes::build(
        store.ids.len(),
        vec![
            (
                0,
                Attribute {
                    group: 1,
                    kind: 7,
                    value: 0,
                },
            ),
            (
                0,
                Attribute {
                    group: 2,
                    kind: 7,
                    value: 1,
                },
            ),
            (
                1,
                Attribute {
                    group: 1,
                    kind: 7,
                    value: 0,
                },
            ),
        ],
    )
    .unwrap();
    store.validate().unwrap();
    for (query, expected) in [
        ("* : 1007000 != #10", vec![0]),
        ("* : 1007000 = #10", vec![0, 1]),
        ("* : 1007000 > #10", vec![0]),
        ("* : [1..1] 1007000 != #10", vec![0]),
        ("* : [2..*] 1007000 != #10", vec![]),
        ("1000000 : [0..0] 1007000 = #10", vec![]),
        ("* : { 1007000 != #10 }", vec![0]),
    ] {
        assert_eq!(
            evaluate(&store, &parse(query).unwrap()).unwrap(),
            expected,
            "{query}"
        );
    }
}

#[test]
fn nested_names_values_projection_and_extrema() {
    assert_query("* : (1005000 OR 1006000) = (<< 1009000)", &[0, 1, 2, 3]);
    assert_query("* : 1005000 = (* : R 1005000 = 1000000)", &[0, 1, 2, 3]);
    assert_query("1000000 . (1005000 OR 1006000)", &[8, 9]);
    assert_query("1003000 . 1006000 . 116680003", &[9]);
    assert_query("!!> (1008000 OR 1009000 OR 1010000)", &[8]);
    assert_query("bottom (1008000 OR 1009000 OR 1010000)", &[10]);
    assert!(matches!(
        evaluate(&fixture(), &parse("1000000 . 1007000").unwrap()),
        Err(EvalError::TypeMismatch)
    ));
}

#[test]
fn reverse_flags_inside_groups_or_with_concrete_values_are_semantic_errors() {
    use snomed_ecl_engine::ecl::{
        AttributeConstraint, AttributeValue, Cardinality, Comparison, Expr, Refinement,
    };
    for (query, offset) in [
        ("* : { R 1005000 = * }", 6),
        ("* : {R 1005000 = *}", 5),
        ("* : { reverseOf 1005000 = * }", 6),
        ("* : { r 1005000 = * }", 6),
        ("* : {r1005000 = *}", 5),
        ("* : { 1006000 = *, R 1005000 = * }", 19),
        ("* : { (1006000 = * OR R 1005000 = *) }", 22),
        ("* : [1..1] { [2..*] R 1005000 = * }", 20),
        ("* : 1006000 = *, { R 1005000 = * }", 19),
        ("* : R 1007000 = #1", 4),
        ("* : r 1007000 = #1", 4),
        ("* : R 1007000 = \"A\"", 4),
        ("* : R 1007000 = true", 4),
    ] {
        let error = parse(query).unwrap_err();
        assert_eq!(error.kind, ParseErrorKind::Semantic, "{query}");
        assert_eq!(error.offset, offset, "{query}");
    }
    // The ungrouped forms remain valid; the group boundary alone decides.
    assert_query("* : R 1005000 = * OR { 1006000 = * }", &[0, 1, 3, 8]);
    assert_query("* : (R 1005000 = 1000000) AND 1005000 = *", &[]);
    // A constructed AST receives the same ruling at evaluation.
    let grouped = Expr::Refined(
        Box::new(Expr::All),
        Box::new(Refinement::Group(
            Cardinality::default(),
            Box::new(Refinement::Attribute(AttributeConstraint {
                cardinality: Cardinality::default(),
                reverse: true,
                name: Box::new(Expr::Concept(1005000)),
                comparison: Comparison::Eq,
                value: AttributeValue::Concepts(Box::new(Expr::All)),
            })),
        )),
    );
    assert!(matches!(
        evaluate(&fixture(), &grouped),
        Err(EvalError::Semantic(_))
    ));
}

#[test]
fn concrete_dot_preserves_exact_values_and_rejects_concept_only_operations() {
    use snomed_ecl_engine::{
        eval::{evaluate_result, QueryResult},
        store::MemberValue as V,
    };
    let store = fixture();
    for (query, expected) in [
        (
            "1000000 . 1007000",
            vec![V::Number("0.10000000000000000001".into())],
        ),
        ("1003000 . 1007000", vec![V::String("A\"B".into())]),
        ("1004000 . 1007000", vec![V::Boolean(true)]),
        (
            "(1000000 . 1007000) OR (1001000 . 1007000)",
            vec![
                V::Number("0.1".into()),
                V::Number("0.10000000000000000001".into()),
            ],
        ),
        (
            "1000000 . (1005000 OR 1007000)",
            vec![
                V::Concept("1008000".into()),
                V::Number("0.10000000000000000001".into()),
            ],
        ),
    ] {
        assert_eq!(
            evaluate_result(&store, &parse(query).unwrap()).unwrap(),
            QueryResult::Values(expected),
            "{query}"
        );
    }
    for query in ["1000000 . 1007000 . *", "<< (1000000 . 1007000)"] {
        assert_eq!(
            evaluate_result(&store, &parse(query).unwrap()),
            Err(EvalError::TypeMismatch)
        );
    }
}

#[test]
fn nested_typed_sets_can_feed_concept_operations_after_scalars_are_removed() {
    let scalar = "(1000000 . 1007000)";
    let concepts = format!("((1008000 OR {scalar}) MINUS {scalar})");
    for (query, expected) in [
        (concepts.clone(), vec![8]),
        (format!("< {concepts}"), vec![9, 10]),
        (format!("> ({concepts} AND {scalar})"), vec![]),
        (format!("!!> ({concepts} OR 1009000)"), vec![8]),
        (format!("* : 1005000 = {concepts}"), vec![0, 1, 2, 3]),
        (format!("{concepts} {{{{C active=1}}}}"), vec![8]),
        (format!("{concepts} : R 1005000 = 1000000"), vec![8]),
        (
            format!("(1009000 OR ({concepts} AND {scalar})) . 116680003"),
            vec![8],
        ),
        (
            format!("1000000 . ((1005000 OR {scalar}) MINUS {scalar})"),
            vec![8],
        ),
    ] {
        assert_query(&query, &expected);
    }
    for query in [
        format!("< (1008000 OR {scalar})"),
        format!("(1008000 OR {scalar}) . 116680003"),
        format!("* : 1005000 = (1008000 OR {scalar})"),
    ] {
        assert_eq!(
            evaluate(&fixture(), &parse(&query).unwrap()),
            Err(EvalError::TypeMismatch)
        );
    }
}

#[test]
fn malformed_refinements_fail_and_long_syntax_agrees() {
    for query in [
        "* : [2..1] 1005000 = *",
        "* : [01..2] 1005000 = *",
        "* : 1005000 > 1008000",
        "* : 1007000 = #01",
        "* : 1007000 = #1.",
        "* : 1007000 = #--1",
        "* : 1007000 = wild:\"*B\"",
        "* : 1007000 = match:\"A\"",
        "* : { { 1005000 = * } }",
        "* : [1..*] (1005000 = *)",
        "* : 1005000 = * OR 1006000 = * AND 1007000 = *",
        "!!> <<1008000",
    ] {
        assert_eq!(
            parse(query).unwrap_err().kind,
            ParseErrorKind::Syntax,
            "{query}"
        );
    }
    assert_eq!(
        parse("* : [0..*] R 1005000 != 1008000").unwrap(),
        parse("ANY : [0 to many] reverseOf 1005000 not = 1008000").unwrap()
    );
}

#[test]
fn decimal_order_is_exact_beyond_machine_integer_and_float_ranges() {
    let ordered = [
        "-999000099900009990000999000099900009990000",
        "-10",
        "-0.11",
        "-0.10000000000000000001",
        "-0.1",
        "0",
        "0.00000000000000000001",
        "0.1",
        "0.10000000000000000001",
        "10",
        "999000099900009990000999000099900009990000",
    ];
    for pair in ordered.windows(2) {
        assert!(Decimal::parse(pair[0]).unwrap() < Decimal::parse(pair[1]).unwrap());
    }
    assert_eq!(Decimal::parse("-0.000"), Decimal::parse("+0000"));
    assert_eq!(Decimal::parse("01.1000"), Decimal::parse("1.1"));
}
