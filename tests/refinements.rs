use snomed_rust_ecl_engine::decimal::Decimal;
use snomed_rust_ecl_engine::ecl::{parse, ParseErrorKind};
use snomed_rust_ecl_engine::eval::{evaluate, EvalError};
use snomed_rust_ecl_engine::store::{
    Adjacency, Attribute, Attributes, ConcreteValue, NumericStore,
};

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
        ids: (1000000..1000011).chain([116680003]).collect(),
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
        evaluate(&fixture(), &parse(query).unwrap()).unwrap(),
        expected,
        "{query}"
    );
}

#[test]
fn groups_do_not_cross_match_or_include_group_zero() {
    assert_query("* : 1000005 = 1000008, 1000006 = 1000009", &[0, 1, 2]);
    assert_query("* : { 1000005 = 1000008, 1000006 = 1000009 }", &[0]);
    assert_query("* : [2..2] { 1000005 = 1000008 }", &[3]);
    assert_query("* : { [0..0] 1000006 = * }", &[1, 3]);
    assert_query(
        "* : { (1000005 = 1000008 OR 1000006 = 1000010), 1000006 = * }",
        &[0, 3],
    );
}

#[test]
fn cardinality_and_inequality_preserve_absence_and_reverse_identity() {
    assert_query("* : [2..*] 1000005 = 1000008", &[3]);
    assert_query("(1000000 OR 1000004) : [0..0] 1000005 = *", &[4]);
    assert_query("* : 1000006 != 1000009", &[3]);
    assert_query("* : [4..4] R 1000005 = *", &[8]);
    assert_query("* : [5..5] R 1000005 = *", &[]);
    assert_query("* : R 1000005 != 1000000", &[8]);
    assert_query("* : 116680003 = 1000008", &[9]);
    assert_query("* : R 116680003 = 1000010", &[9]);
}

#[test]
fn concrete_values_compare_exactly_and_keep_their_types() {
    assert_query("* : 1000007 > #0.1", &[0]);
    assert_query("* : 1000007 = #+0.100", &[1]);
    assert_query("* : 1000007 < #-1.999999999999999999999", &[2]);
    assert_query("* : 1000007 != #0.1", &[0, 2]);
    assert_query("* : 1000007 = \"A\\\"B\"", &[3]);
    assert_query("* : 1000007 = (\"other\" \"A\\\"B\")", &[3]);
    assert_query("* : 1000007 = true", &[4]);
    assert_query("* : 1000007 != false", &[4]);
    assert_query("* : { 1000007 = true }", &[]);
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
        ("* : 1000007 != #10", vec![0]),
        ("* : 1000007 = #10", vec![0, 1]),
        ("* : 1000007 > #10", vec![0]),
        ("* : [1..1] 1000007 != #10", vec![0]),
        ("* : [2..*] 1000007 != #10", vec![]),
        ("1000000 : [0..0] 1000007 = #10", vec![]),
        ("* : { 1000007 != #10 }", vec![0]),
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
    assert_query("* : (1000005 OR 1000006) = (<< 1000009)", &[0, 1, 2, 3]);
    assert_query("* : 1000005 = (* : R 1000005 = 1000000)", &[0, 1, 2, 3]);
    assert_query("1000000 . (1000005 OR 1000006)", &[8, 9]);
    assert_query("1000003 . 1000006 . 116680003", &[9]);
    assert_query("!!> (1000008 OR 1000009 OR 1000010)", &[8]);
    assert_query("bottom (1000008 OR 1000009 OR 1000010)", &[10]);
    assert!(matches!(
        evaluate(&fixture(), &parse("1000000 . 1000007").unwrap()),
        Err(EvalError::Unsupported(_))
    ));
    assert!(matches!(
        evaluate(&fixture(), &parse("* : { R 1000005 = * }").unwrap()),
        Err(EvalError::Unsupported(_))
    ));
}

#[test]
fn malformed_refinements_fail_and_long_syntax_agrees() {
    for query in [
        "* : [2..1] 1000005 = *",
        "* : [01..2] 1000005 = *",
        "* : 1000005 > 1000008",
        "* : 1000007 = #01",
        "* : 1000007 = #1.",
        "* : 1000007 = #--1",
        "* : R 1000007 = #1",
        "* : { { 1000005 = * } }",
        "* : [1..*] (1000005 = *)",
        "* : 1000005 = * OR 1000006 = * AND 1000007 = *",
        "!!> <<1000008",
    ] {
        assert_eq!(
            parse(query).unwrap_err().kind,
            ParseErrorKind::Syntax,
            "{query}"
        );
    }
    assert_eq!(
        parse("* : [0..*] R 1000005 != 1000008").unwrap(),
        parse("ANY : [0 to many] reverseOf 1000005 not = 1000008").unwrap()
    );
}

#[test]
fn decimal_order_is_exact_beyond_machine_integer_and_float_ranges() {
    let ordered = [
        "-999999999999999999999999999999999999999999",
        "-10",
        "-0.11",
        "-0.10000000000000000001",
        "-0.1",
        "0",
        "0.00000000000000000001",
        "0.1",
        "0.10000000000000000001",
        "10",
        "999999999999999999999999999999999999999999",
    ];
    for pair in ordered.windows(2) {
        assert!(Decimal::parse(pair[0]).unwrap() < Decimal::parse(pair[1]).unwrap());
    }
    assert_eq!(Decimal::parse("-0.000"), Decimal::parse("+0000"));
    assert_eq!(Decimal::parse("01.1000"), Decimal::parse("1.1"));
}
