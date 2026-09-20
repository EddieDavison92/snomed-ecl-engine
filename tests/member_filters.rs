use snomed_ecl_engine::ecl::{parse, Expr};
use snomed_ecl_engine::eval::{
    evaluate, evaluate_result, evaluate_result_with_limits, EvalError, Limits, QueryResult,
};
use snomed_ecl_engine::store::{
    MemberColumn as C, MemberStore, MemberTable, MemberValue, NumericStore, TextColumn,
};

fn table() -> MemberTable {
    let mut text = TextColumn::default();
    for term in ["A12.3", "B12.3", "A12.3", "A12.4"] {
        text.push(term).unwrap();
    }
    MemberTable {
        refset: 200001,
        names: [
            "id",
            "effectiveTime",
            "active",
            "moduleId",
            "refsetId",
            "referencedComponentId",
            "mapGroup",
            "mapTarget",
            "targetComponentId",
            "grouped",
            "sourceEffectiveTime",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        columns: vec![
            C::Uuid((1..=4).map(|i| [i; 16]).collect()),
            C::Time(vec![20260826, 20260731, 20260826, 0]),
            C::Boolean(vec![1, 1, 0, 1]),
            C::Id(vec![100001; 4]),
            C::Id(vec![200001; 4]),
            C::Id(vec![300001, 300001, 300002, 300003]),
            C::Integer(vec![1, 2, 1, 2]),
            C::Text(text),
            C::Id(vec![400001, 400002, 400001, 400003]),
            C::Boolean(vec![0, 1, 0, 1]),
            C::Time(vec![20260826, 20260731, 20260826, 0]),
        ],
    }
}
fn fixture() -> NumericStore {
    NumericStore {
        ids: vec![
            100001, 200001, 300001, 300002, 300003, 400001, 400002, 400003,
        ],
        flags: vec![1, 1, 1, 1, 0, 1, 1, 1],
        member_tables: MemberStore::loaded(vec![table()]).unwrap(),
        ..NumericStore::default()
    }
}
fn codes(store: &NumericStore, query: &str) -> Vec<u64> {
    evaluate(store, &parse(query).unwrap())
        .unwrap()
        .into_iter()
        .map(|o| store.ids[o as usize])
        .collect()
}
#[test]
fn member_metadata_numeric_predicates_and_concept_projections_preserve_rows() {
    let store = fixture();
    for (query, expected) in [
        ("^[referencedComponentId]200001", vec![300001, 300003]),
        ("^200001 {{M active=0}}", vec![300002]),
        ("^200001 {{M active=\"*\"}}", vec![300001, 300002, 300003]),
        ("^200001 {{M mapGroup= #1}}", vec![300001]),
        ("^200001 {{M mapGroup!= #1}}", vec![300001, 300003]),
        ("^200001 {{M mapGroup< #1.5}}", vec![300001]),
        (
            "^200001 {{M mapGroup>= #1.000000000000000001}}",
            vec![300001, 300003],
        ),
        ("^200001 {{M moduleId=100001, mapGroup= #1}}", vec![300001]),
        ("^200001 {{M grouped=true}}", vec![300001, 300003]),
        ("^200001 {{M effectiveTime=\"\"}}", vec![300003]),
        (
            "^200001 {{M sourceEffectiveTime<\"20260801\"}}",
            vec![300001],
        ),
        (
            "^200001 {{M effectiveTime=(\"20260826\" \"\")}}",
            vec![300001, 300003],
        ),
        (
            "^[targetComponentId]200001 {{M referencedComponentId=300001}}",
            vec![400001, 400002],
        ),
        (
            "^[targetComponentId]200001 {{M mapGroup=#1}} {{M mapGroup=#2}}",
            vec![],
        ),
        ("^R300002 {{M active=0}}", vec![200001]),
        ("^[targetComponentId]200001 AND 400001", vec![400001]),
    ] {
        assert_eq!(codes(&store, query), expected, "{query}");
    }
}

#[test]
fn arbitrary_date_fields_resolve_ambiguous_quoted_values_from_the_column_type() {
    let mut table = table();
    table.names[10] = "reviewDate".into();
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![table]).unwrap();
    for (predicate, expected) in [
        (r#"="20260826""#, vec![300001]),
        (r#"!="20260826""#, vec![300001, 300003]),
        (r#"=("20260826" "")"#, vec![300001, 300003]),
        (r#"<"20260826""#, vec![300001]),
        (r#">="20260826""#, vec![300001]),
        (r#"="""#, vec![300003]),
    ] {
        assert_eq!(
            codes(&store, &format!("^200001 {{{{M reviewDate{predicate}}}}}")),
            expected
        );
    }
    assert!(matches!(
        evaluate(
            &store,
            &parse(r#"^200001 {{M reviewDate=wild:"20260826"}}"#).unwrap()
        ),
        Err(EvalError::TypeMismatch | EvalError::Unsupported(_))
    ));
    #[cfg(feature = "unicode")]
    {
        let mut table = crate::table();
        let mut text = TextColumn::default();
        for value in ["20260826 suffix", "other", "20260826", "20260731"] {
            text.push(value).unwrap();
        }
        table.columns[7] = C::Text(text);
        store.member_tables = MemberStore::loaded(vec![table]).unwrap();
        assert_eq!(
            codes(&store, r#"^200001 {{M mapTarget="20260826"}}"#),
            vec![300001]
        );
        assert_eq!(
            codes(&store, r#"^200001 {{M mapTarget=("20260826" "20260731")}}"#),
            vec![300001, 300003]
        );
    }
}

#[cfg(feature = "unicode")]
#[test]
fn member_text_collation_uses_the_configured_language() {
    let mut table = table();
    let mut text = TextColumn::default();
    for value in ["sjögren", "other", "other", "other"] {
        text.push(value).unwrap();
    }
    table.columns[7] = C::Text(text);
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![table]).unwrap();
    assert_eq!(
        codes(&store, r#"^200001 {{M mapTarget="sjogren"}}"#),
        vec![300001]
    );
    store.config.member_language = "sv".into();
    assert!(codes(&store, r#"^200001 {{M mapTarget="sjogren"}}"#).is_empty());
    assert_eq!(
        codes(&store, r#"^200001 {{M mapTarget="sjögren"}}"#),
        vec![300001]
    );
}
#[test]
fn terminal_tuples_have_typed_values_and_fail_in_concept_subqueries() {
    let store = fixture();
    let query = parse("^[referencedComponentId,mapTarget,mapGroup]200001").unwrap();
    let QueryResult::Rows(rows) = evaluate_result(&store, &query).unwrap() else {
        panic!()
    };
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["mapTarget"], MemberValue::String("A12.3".into()));
    assert_eq!(rows[0]["mapGroup"], MemberValue::Number("1".into()));
    assert_eq!(
        rows[0]["referencedComponentId"],
        MemberValue::Concept("300001".into())
    );
    assert_eq!(evaluate(&store, &query), Err(EvalError::TypeMismatch));
    for query in [
        "* AND (^[mapTarget]200001)",
        "* AND (^[mapTarget,mapGroup]200001 {{M mapGroup=#99}})",
    ] {
        assert_eq!(
            evaluate_result(&store, &parse(query).unwrap()),
            Err(EvalError::TypeMismatch)
        );
    }
    let QueryResult::Rows(rows) = evaluate_result(&store, &parse("^[*]200001").unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(rows[0].len(), 6);
    assert!(!rows[0].contains_key("id"));
    assert_eq!(
        evaluate_result_with_limits(
            &store,
            &query,
            Limits {
                max_live_set_values: 20,
                ..Limits::default()
            },
            None
        ),
        Err(EvalError::MemoryLimit)
    );
}

#[test]
fn scalar_projections_form_exact_typed_sets() {
    let mut store = fixture();
    let mut table = table();
    let mut numbers = TextColumn::default();
    for value in ["+1.000", "1", "-0.00", "1.000000000000000001"] {
        numbers.push(value).unwrap();
    }
    table.columns[6] = C::Number(numbers);
    store.member_tables = MemberStore::loaded(vec![table]).unwrap();
    for (query, expected) in [
        ("^[mapGroup]200001", vec!["1", "1.000000000000000001"]),
        (
            "(^[mapGroup]200001) AND (^[mapGroup]200001 {{M referencedComponentId=300001}})",
            vec!["1"],
        ),
        (
            "(^[mapGroup]200001) MINUS (^[mapGroup]200001 {{M referencedComponentId=300001}})",
            vec!["1.000000000000000001"],
        ),
        (
            "(^[mapGroup]200001 {{M active=0}}) OR (^[mapGroup]200001)",
            vec!["0", "1", "1.000000000000000001"],
        ),
        (
            "(^[mapGroup]200001 {{M mapGroup=#99}}) OR (^[mapGroup]200001)",
            vec!["1", "1.000000000000000001"],
        ),
    ] {
        let result = evaluate_result(&store, &parse(query).unwrap()).unwrap();
        assert_eq!(
            result,
            QueryResult::Values(
                expected
                    .into_iter()
                    .map(|s| MemberValue::Number(s.into()))
                    .collect()
            ),
            "{query}"
        );
    }
    assert_eq!(
        evaluate_result(&store, &parse("^[grouped]200001").unwrap()).unwrap(),
        QueryResult::Values(vec![
            MemberValue::Boolean(false),
            MemberValue::Boolean(true)
        ])
    );
    for query in [
        "(^[*]200001 {{M mapGroup=#99}}) OR (*)",
        "(^[mapGroup]200001) OR (*)",
        "<< (^[mapGroup]200001)",
    ] {
        assert_eq!(
            evaluate_result(&store, &parse(query).unwrap()),
            Err(EvalError::TypeMismatch)
        );
    }
    assert_eq!(
        evaluate_result_with_limits(
            &store,
            &parse("^[mapGroup]200001").unwrap(),
            Limits {
                max_live_set_values: 10,
                ..Limits::default()
            },
            None
        ),
        Err(EvalError::MemoryLimit)
    );
}
#[test]
fn member_errors_are_explicit_and_store_tables_are_validated() {
    let store = fixture();
    assert_eq!(
        parse("200001 {{M active=1}}").unwrap_err().kind,
        snomed_ecl_engine::ecl::ParseErrorKind::Unsupported
    );
    for query in ["^[missing]200001", "^200001 {{M missing=#1}}"] {
        assert!(matches!(
            evaluate_result(&store, &parse(query).unwrap()),
            Err(EvalError::InvalidField(_))
        ));
    }
    assert_eq!(
        evaluate_result(&store, &parse("^200001 {{M mapGroup=true}}").unwrap()),
        Err(EvalError::TypeMismatch)
    );
    assert!(matches!(
        evaluate(
            &NumericStore::default(),
            &parse("^[targetComponentId]999999").unwrap()
        ),
        Err(EvalError::Unsupported(_))
    ));
    for invalid in [
        "^[]200001",
        "^[mapTarget,]200001",
        "^[mapTarget,mapTarget]200001",
        "^200001 {{M mapGroup=#01}}",
        "^200001 {{M mapGroup=#-+1}}",
    ] {
        assert!(parse(invalid).is_err(), "{invalid}");
    }
    let mut corrupt = table();
    corrupt.names[6] = "id".into();
    assert!(MemberStore::loaded(vec![corrupt]).is_err());
    let mut corrupt = table();
    if let C::Text(text) = &mut corrupt.columns[7] {
        text.offsets[1] = u32::MAX;
    }
    assert!(MemberStore::loaded(vec![corrupt]).is_err());
    assert!(matches!(
        parse("^200001 {{M mapGroup=#1}}").unwrap(),
        Expr::Members(_)
    ));
}

#[test]
fn member_limits_and_dangling_inactive_references_never_return_partial_concept_sets() {
    use std::sync::atomic::AtomicBool;
    let mut table = table();
    let C::Id(references) = &mut table.columns[5] else {
        panic!()
    };
    references[2] = 999001;
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![table]).unwrap();
    let query = parse("^200001 {{M active=0}}").unwrap();
    assert_eq!(
        evaluate_result(&store, &query),
        Err(EvalError::TypeMismatch)
    );
    let QueryResult::Rows(rows) = evaluate_result(
        &store,
        &parse("^[referencedComponentId,mapGroup]200001 {{M active=0}}").unwrap(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        rows[0]["referencedComponentId"],
        MemberValue::Concept("999001".into())
    );
    assert_eq!(
        evaluate_result_with_limits(
            &store,
            &query,
            Limits::default(),
            Some(&AtomicBool::new(true))
        ),
        Err(EvalError::Cancelled)
    );
    assert_eq!(
        evaluate_result_with_limits(
            &store,
            &query,
            Limits {
                max_work: 1,
                ..Limits::default()
            },
            None
        ),
        Err(EvalError::WorkLimit)
    );
}
#[cfg(feature = "unicode")]
#[test]
fn map_target_strings_use_the_same_row_as_numeric_filters() {
    let store = fixture();
    for (query, expected) in [
        (
            r#"^200001 {{M mapTarget=wild:"A12*"}}"#,
            vec![300001, 300003],
        ),
        (r#"^200001 {{M mapTarget="A12"}}"#, vec![300001, 300003]),
        (r#"^200001 {{M mapTarget=wild:"B*",mapGroup=#1}}"#, vec![]),
        (
            r#"^200001 {{M mapTarget=wild:"B*",mapGroup=#2}}"#,
            vec![300001],
        ),
        (r#"^200001 {{M mapTarget!=wild:"A12*"}}"#, vec![300001]),
    ] {
        assert_eq!(codes(&store, query), expected, "{query}");
    }
}
