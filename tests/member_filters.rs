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
            "effectiveTime",
            "active",
            "moduleId",
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
            C::Time(vec![20260826, 20260731, 20260826, 0]),
            C::Boolean(vec![1, 1, 0, 1]),
            C::Id(vec![100001; 4]),
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
        ("^200001 {{Mactive=0}}", vec![300002]),
        ("^200001 {{mmoduleId=100001}}", vec![300001, 300003]),
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
    table.names[8] = "reviewDate".into();
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
        table.columns[5] = C::Text(text);
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
    table.columns[5] = C::Text(text);
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
        "<< (^[mapTarget]200001)",
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
    table.columns[4] = C::Number(numbers);
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
fn heterogeneous_sets_preserve_types_and_empty_dot_sets_are_neutral() {
    let store = fixture();
    assert_eq!(
        evaluate_result(&store, &parse("300001 OR (^[mapGroup]200001)").unwrap()).unwrap(),
        QueryResult::Values(vec![
            MemberValue::Concept("300001".into()),
            MemberValue::Number("1".into()),
            MemberValue::Number("2".into())
        ])
    );
    assert_eq!(
        evaluate_result(&store, &parse("* AND (^[mapGroup]200001)").unwrap()).unwrap(),
        QueryResult::Values(vec![])
    );
    assert_eq!(
        evaluate_result(&store, &parse("(^[mapGroup]200001) MINUS *").unwrap()).unwrap(),
        QueryResult::Values(vec![
            MemberValue::Number("1".into()),
            MemberValue::Number("2".into())
        ])
    );
    let mut complete = store;
    complete.attributes =
        snomed_ecl_engine::store::Attributes::build(complete.ids.len(), vec![]).unwrap();
    complete.concrete =
        snomed_ecl_engine::store::Attributes::build(complete.ids.len(), vec![]).unwrap();
    assert_eq!(
        evaluate_result(
            &complete,
            &parse("(300001 . 400001) OR (^[mapGroup]200001)").unwrap()
        )
        .unwrap(),
        QueryResult::Values(vec![
            MemberValue::Number("1".into()),
            MemberValue::Number("2".into())
        ])
    );
}

#[test]
fn member_subqueries_accept_concept_sets_recovered_from_typed_operations() {
    let store = fixture();
    for query in [
        "^[referencedComponentId]((200001 OR (^[mapGroup]200001)) MINUS (^[mapGroup]200001))",
        "^200001 {{M referencedComponentId = ((300001 OR (^[mapGroup]200001)) AND *)}}",
    ] {
        let expected = if query.contains("{{M") {
            vec![300001]
        } else {
            vec![300001, 300003]
        };
        assert_eq!(codes(&store, query), expected);
    }
}

#[test]
fn recovered_concepts_keep_numeric_order_and_evaluation_limits() {
    use snomed_ecl_engine::eval::evaluate_with_limits;
    use std::sync::atomic::AtomicBool;
    let mut store = fixture();
    store.ids.push(10000000);
    store.flags.push(1);
    let query =
        parse("((10000000 OR 300001) OR (^[mapGroup]200001)) MINUS (^[mapGroup]200001)").unwrap();
    assert_eq!(evaluate(&store, &query).unwrap(), vec![2, 8]);
    assert_eq!(
        evaluate_with_limits(
            &store,
            &query,
            Limits::default(),
            Some(&AtomicBool::new(true))
        ),
        Err(EvalError::Cancelled)
    );
    for (limits, expected) in [
        (
            Limits {
                max_live_set_values: 20,
                ..Limits::default()
            },
            EvalError::MemoryLimit,
        ),
        (
            Limits {
                max_work: 20,
                ..Limits::default()
            },
            EvalError::WorkLimit,
        ),
    ] {
        assert_eq!(
            evaluate_with_limits(&store, &query, limits, None),
            Err(expected)
        );
    }
    // An empty operand must not hide an invalid field in the other operand.
    let invalid = parse("(999999 AND (^[mapGroup]200001)) AND (^[missing]200001)").unwrap();
    assert!(matches!(
        evaluate(&store, &invalid),
        Err(EvalError::InvalidField(_))
    ));
}

#[test]
fn field_projection_across_refsets_keeps_each_columns_type() {
    let mut first = table();
    first.names[4] = "target".into();
    first.columns[4] = C::Id(vec![300001; 4]);
    let mut second = first.clone();
    second.refset = 400001;
    second.columns[4] = C::Integer(vec![300001; 4]);
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![first, second]).unwrap();
    assert_eq!(
        evaluate_result(&store, &parse("^[target](200001 OR 400001)").unwrap()).unwrap(),
        QueryResult::Values(vec![
            MemberValue::Concept("300001".into()),
            MemberValue::Number("300001".into())
        ])
    );
}
#[test]
fn member_errors_are_explicit_and_store_tables_are_validated() {
    let store = fixture();
    assert_eq!(
        parse("200001 {{M active=1}}").unwrap_err().kind,
        snomed_ecl_engine::ecl::ParseErrorKind::Semantic
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
    corrupt.names[4] = "active".into();
    assert!(MemberStore::loaded(vec![corrupt]).is_err());
    let mut corrupt = table();
    if let C::Text(text) = &mut corrupt.columns[5] {
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
    let C::Id(references) = &mut table.columns[3] else {
        panic!()
    };
    references[2] = 999001;
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![table]).unwrap();
    // memberOf is the set of referenced concepts, so an identifier naming no concept of this
    // substrate adds none. Tuple projections keep the row and its original identifier.
    let query = parse("^200001 {{M active=0}}").unwrap();
    assert_eq!(evaluate(&store, &query).unwrap(), Vec::<u32>::new());
    assert_eq!(
        codes(&store, "^[referencedComponentId]200001 {{M active=\"*\"}}"),
        vec![300001, 300003]
    );
    assert!(codes(&store, "^R (300001 OR 999001) {{M active=0}}").is_empty());
    assert_eq!(
        codes(&store, "^R (300001 OR 300003) {{M active=\"*\"}}"),
        vec![200001]
    );
    assert_eq!(
        evaluate_result(
            &store,
            &parse("^200001 {{M active=0, mapGroup=#1}}").unwrap()
        )
        .unwrap(),
        QueryResult::Concepts(vec![])
    );
    let QueryResult::Rows(rows) = evaluate_result(
        &store,
        &parse("^[referencedComponentId,mapGroup]200001 {{M active=0}}").unwrap(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["referencedComponentId"],
        MemberValue::Concept("999001".into())
    );
    let query = parse("^200001 {{M active=0}}").unwrap();
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
mod support;

#[test]
fn generated_member_queries_match_the_independent_row_scan() {
    use snomed_ecl_engine::store::Adjacency;
    use std::collections::BTreeSet;
    // Identifiers 1000000 + 1000k all carry concept partition 00; 1098000 and 1099000 are absent.
    let id = |k: usize| 1000000 + k as u64 * 1000;
    let n = 30usize;
    let ids: Vec<u64> = (0..n).map(id).collect();
    let pairs: Vec<_> = (1..12u32).map(|c| (c, (c - 1) / 2)).collect();
    let mut tables = Vec::new();
    for t in 0..3usize {
        let refset = id(20 + t);
        let rows = 12 + t * 5;
        // Only referencedComponentId carries absent identifiers: memberOf omits them, whereas a
        // projected targetComponentId would return them as values outside the concept API.
        let pick = |i: usize, salt: usize| -> u64 {
            match (i * 7 + salt * 3 + t) % 9 {
                0 if salt == 0 => id(99),
                1 if salt == 0 => id(98),
                k => id((i * 5 + k + salt) % 16),
            }
        };
        let mut text = TextColumn::default();
        for i in 0..rows {
            text.push(&format!("T{}", i % 4)).unwrap();
        }
        tables.push(MemberTable {
            refset,
            names: [
                "effectiveTime",
                "active",
                "moduleId",
                "referencedComponentId",
                "mapGroup",
                "mapTarget",
                "targetComponentId",
                "grouped",
                "reviewDate",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            columns: vec![
                C::Time(
                    (0..rows)
                        .map(|i| [20260826, 20260731, 0, 20250101][i % 4])
                        .collect(),
                ),
                C::Boolean((0..rows).map(|i| u8::from((i * 3 + t) % 4 != 0)).collect()),
                C::Id((0..rows).map(|i| id(i % 2)).collect()),
                C::Id((0..rows).map(|i| pick(i, 0)).collect()),
                C::Integer((0..rows).map(|i| (i % 3) as i64 - 1).collect()),
                C::Text(text),
                C::Id((0..rows).map(|i| pick(i, 1)).collect()),
                C::Boolean((0..rows).map(|i| u8::from(i % 5 == 0)).collect()),
                C::Time(
                    (0..rows)
                        .map(|i| [0, 20260826, 20240229, 20260826][i % 4])
                        .collect(),
                ),
            ],
        });
    }
    // Plain memberOf uses the active-membership index; keep it consistent with the tables.
    let mut membership = Vec::new();
    for table in &tables {
        let (C::Boolean(active), C::Id(referenced)) = (&table.columns[1], &table.columns[3]) else {
            panic!()
        };
        for row in 0..table.len() {
            if active[row] != 0 {
                if let Ok(member) = ids.binary_search(&referenced[row]) {
                    let refset = ids.binary_search(&table.refset).unwrap() as u32;
                    membership.push((refset, member as u32));
                }
            }
        }
    }
    let store = NumericStore {
        ids,
        membership: Some(snomed_ecl_engine::store::MembershipIndex::build(n, membership).unwrap()),
        // Inactive concepts sit outside the small hierarchy so the store validates.
        flags: (0..n).map(|i| u8::from(i < 12 || i % 7 != 3)).collect(),
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        parents: Adjacency::build(n, pairs.clone()).unwrap(),
        children: Adjacency::build(n, pairs.iter().map(|&(a, b)| (b, a)).collect()).unwrap(),
        attributes: snomed_ecl_engine::store::Attributes::build(n, vec![]).unwrap(),
        concrete: snomed_ecl_engine::store::Attributes::build(n, vec![]).unwrap(),
        member_tables: MemberStore::loaded(tables).unwrap(),
        ..NumericStore::default()
    };
    store.validate().unwrap();
    let source = |i: usize| match i % 5 {
        0 => id(20).to_string(),
        1 => format!("({} OR {})", id(20), id(22)),
        2 => "*".to_owned(),
        3 => format!("(<< {} OR {})", id(0), id(20 + i % 3)),
        _ => id(21).to_string(),
    };
    let filter = |i: usize| match i % 13 {
        0 => String::new(),
        1 => " {{M active=0}}".into(),
        2 => " {{M active=\"*\"}}".into(),
        3 => format!(
            " {{{{M mapGroup{}#{}}}}}",
            ["=", "!=", ">", "<="][i % 4],
            (i % 3) as i64 - 1
        ),
        4 => format!(
            " {{{{M referencedComponentId=(<<{} OR {})}}}}",
            id(0),
            id(i % 16)
        ),
        5 => format!(" {{{{M moduleId={}}}}}", id(1)),
        6 => " {{M grouped=true}}".into(),
        7 => format!(
            " {{{{M reviewDate{}\"{}\"}}}}",
            ["=", "!=", "<", ">="][i % 4],
            ["20260826", ""][i % 2]
        ),
        8 => " {{M effectiveTime=(\"20260731\" \"\")}}".into(),
        9 => format!(
            " {{{{M targetComponentId!={}}}}} {{{{M active=\"*\"}}}}",
            id(99)
        ),
        // One concept on an identifier column reads only its rows.
        11 => format!(" {{{{M referencedComponentId={}}}}}", id(i % 16)),
        12 => format!(
            " {{{{M targetComponentId=({} OR {}), mapGroup!=#0}}}}",
            id(i % 16),
            id(98)
        ),
        _ => " {{M mapGroup=#-1, grouped=false}}".into(),
    };
    for i in 0..600usize {
        let operator = match i % 4 {
            0 => "^",
            1 => "^[targetComponentId]",
            2 => "^R",
            _ => "^[referencedComponentId]",
        };
        let query = format!(
            "({operator} {}{}) {} ({} {})",
            source(i / 4),
            filter(i / 7),
            ["OR", "AND", "MINUS"][i % 3],
            if i % 2 == 0 { "^" } else { "<<" },
            source(i / 9)
        );
        let expression = parse(&query).unwrap_or_else(|e| panic!("{query}: {e}"));
        let actual: BTreeSet<_> = evaluate(&store, &expression)
            .unwrap_or_else(|e| panic!("{query}: {e}"))
            .into_iter()
            .collect();
        assert_eq!(actual, support::slow(&store, &expression), "{query}");
    }
}

#[test]
fn projected_component_fields_return_identifiers_that_name_no_substrate_concept() {
    let mut projected = table();
    let C::Id(targets) = &mut projected.columns[6] else {
        panic!()
    };
    // 999001 is a concept-partition identifier absent from the substrate.
    *targets = vec![400001, 999001, 400001, 400003];
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![projected]).unwrap();
    store.parents = snomed_ecl_engine::store::Adjacency::build(store.ids.len(), vec![]).unwrap();
    store.children = snomed_ecl_engine::store::Adjacency::build(store.ids.len(), vec![]).unwrap();
    let projection = parse("^[targetComponentId]200001").unwrap();
    assert_eq!(
        evaluate_result(&store, &projection).unwrap(),
        QueryResult::Values(vec![
            MemberValue::Concept("400001".into()),
            MemberValue::Concept("400003".into()),
            MemberValue::Concept("999001".into()),
        ])
    );
    // Concept operations cannot use an identifier outside the substrate.
    assert_eq!(
        evaluate(&store, &projection),
        Err(EvalError::MissingReference("999001".into()))
    );
    assert_eq!(
        evaluate_result(&store, &parse("<< (^[targetComponentId]200001)").unwrap()),
        Err(EvalError::MissingReference("999001".into()))
    );
    // Restricting to the substrate first recovers a concept set.
    assert_eq!(
        codes(&store, "<< ((^[targetComponentId]200001) AND *)"),
        vec![400001, 400003]
    );
    assert_eq!(
        evaluate_result(
            &store,
            &parse("(^[targetComponentId]200001) MINUS *").unwrap()
        )
        .unwrap(),
        QueryResult::Values(vec![MemberValue::Concept("999001".into())])
    );
    // A predicate on the field never matches an absent concept, so only != keeps that row.
    assert_eq!(
        codes(&store, "^200001 {{M targetComponentId=999001}}"),
        Vec::<u64>::new()
    );
    assert_eq!(
        codes(&store, "^200001 {{M targetComponentId!=400001}}"),
        vec![300001, 300003]
    );
    let QueryResult::Rows(rows) = evaluate_result(
        &store,
        &parse("^[referencedComponentId,targetComponentId]200001").unwrap(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        rows[1]["targetComponentId"],
        MemberValue::Concept("999001".into())
    );
    // Tables with a promoted integer column merge with 64-bit integer tables exactly.
    let mut base = table();
    let mut promoted = table();
    let mut numbers = TextColumn::default();
    for value in ["3", "99999999999999999999999", "-1", "0"] {
        numbers.push(value).unwrap();
    }
    promoted.columns[4] = C::Number(numbers);
    base.append(promoted.clone()).unwrap();
    let C::Number(merged) = &base.columns[4] else {
        panic!()
    };
    assert_eq!(
        (0..8).map(|i| merged.get(i)).collect::<Vec<_>>(),
        [
            "1",
            "2",
            "1",
            "2",
            "3",
            "99999999999999999999999",
            "-1",
            "0"
        ]
    );
    let mut reversed = promoted;
    reversed.append(table()).unwrap();
    let C::Number(merged) = &reversed.columns[4] else {
        panic!()
    };
    assert_eq!(merged.get(7), "2");
}

#[test]
fn component_fields_keep_non_concept_identifiers_out_of_concept_sets() {
    // 400011 and 400012 carry the description partition; 400021 the relationship partition.
    let mut table = table();
    let C::Id(targets) = &mut table.columns[6] else {
        panic!()
    };
    *targets = vec![400001, 400011, 400021, 400012];
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![table]).unwrap();
    store.parents = snomed_ecl_engine::store::Adjacency::build(store.ids.len(), vec![]).unwrap();
    store.children = snomed_ecl_engine::store::Adjacency::build(store.ids.len(), vec![]).unwrap();
    let projection = parse("^[targetComponentId]200001").unwrap();
    assert_eq!(
        evaluate_result(&store, &projection).unwrap(),
        QueryResult::Values(vec![
            MemberValue::Concept("400001".into()),
            MemberValue::Component("400011".into()),
            MemberValue::Component("400012".into()),
        ])
    );
    assert_eq!(evaluate(&store, &projection), Err(EvalError::TypeMismatch));
    assert_eq!(
        evaluate_result(
            &store,
            &parse("^[targetComponentId]200001 {{M active=0}}").unwrap()
        )
        .unwrap(),
        QueryResult::Values(vec![MemberValue::Component("400021".into())])
    );
    assert_eq!(
        evaluate_result(
            &store,
            &parse("(^[targetComponentId]200001) AND (400001 OR 400011)").unwrap()
        )
        .unwrap(),
        QueryResult::Values(vec![MemberValue::Concept("400001".into())])
    );
    // Recovering the concepts after removing every component value restores the concept API.
    assert_eq!(
        codes(
            &store,
            "<< ((^[targetComponentId]200001) MINUS (^[targetComponentId]200001 {{M mapGroup=#2}}))"
        ),
        vec![400001]
    );
    // A concept expression never matches a description identifier, so inequality keeps such rows.
    assert_eq!(
        codes(&store, "^200001 {{M targetComponentId=400001}}"),
        vec![300001]
    );
    assert_eq!(
        codes(&store, "^200001 {{M targetComponentId!=400001}}"),
        vec![300001, 300003]
    );
    let QueryResult::Rows(rows) =
        evaluate_result(&store, &parse("^[*]200001 {{M active=\"*\"}}").unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        rows[2]["targetComponentId"],
        MemberValue::Component("400021".into())
    );
    assert_eq!(
        serde_json::to_string(&rows[2]["targetComponentId"]).unwrap(),
        r#"{"type":"component","value":"400021"}"#
    );
}

#[test]
fn tuple_projections_are_rejected_in_every_subquery_position() {
    let store = fixture();
    let tuple = "(^[referencedComponentId,mapGroup]200001)";
    for query in [
        format!("{tuple} AND {tuple}"),
        format!("{tuple} OR 300001"),
        format!("300001 MINUS {tuple}"),
        format!("{tuple} MINUS {tuple}"),
        format!("< {tuple}"),
        format!(">> {tuple}"),
        format!("!!> {tuple}"),
        format!("^ {tuple}"),
        format!("^R {tuple}"),
        format!("^[mapGroup] {tuple}"),
        format!("^200001 {{{{M referencedComponentId = {tuple}}}}}"),
        format!("* : 100001 = {tuple}"),
        format!("* : {tuple} = *"),
        format!("{tuple} : 100001 = *"),
        format!("{tuple} . 100001"),
        format!("300001 . {tuple}"),
        format!("{tuple} {{{{C active = 1}}}}"),
        format!("{tuple} {{{{ +HISTORY }}}}"),
        format!("^[*] {tuple}"),
        "(^[*]200001) AND *".to_owned(),
    ] {
        assert_eq!(
            evaluate_result(&store, &parse(&query).unwrap()),
            Err(EvalError::TypeMismatch),
            "{query}"
        );
    }
    // A single projected field is a plain value set and composes normally.
    assert_eq!(
        codes(
            &store,
            "(^[targetComponentId]200001 {{M mapGroup=#1}}) AND *"
        ),
        vec![400001]
    );
}

#[test]
fn field_and_predicate_type_combinations_fail_with_type_mismatch() {
    let store = fixture();
    for query in [
        "^200001 {{M active=#1}}",
        "^200001 {{M active=300001}}",
        "^200001 {{M effectiveTime=#20260826}}",
        "^200001 {{M effectiveTime=true}}",
        "^200001 {{M effectiveTime=300001}}",
        "^200001 {{M moduleId=#1}}",
        "^200001 {{M moduleId=true}}",
        "^200001 {{M referencedComponentId=\"20260826\"}}",
        "^200001 {{M referencedComponentId=#300001}}",
        "^200001 {{M mapGroup=\"20260826\"}}",
        "^200001 {{M mapGroup=300001}}",
        "^200001 {{M mapGroup=false}}",
        "^200001 {{M mapTarget=#1}}",
        "^200001 {{M mapTarget=300001}}",
        "^200001 {{M mapTarget=true}}",
        "^200001 {{M mapTarget=\"\"}}",
        "^200001 {{M targetComponentId=#1}}",
        "^200001 {{M grouped=#1}}",
        "^200001 {{M grouped=\"20260826\"}}",
        "^200001 {{M grouped=300001}}",
        "^200001 {{M sourceEffectiveTime=true}}",
        "^200001 {{M sourceEffectiveTime=#2026}}",
        "^200001 {{M sourceEffectiveTime=300001}}",
        "^200001 {{M mapGroup=#1, mapTarget=#1}}",
        // The wrong type fails even when an earlier predicate already excludes every row.
        "^200001 {{M mapGroup=#99}} {{M mapTarget=#1}}",
    ] {
        assert_eq!(
            evaluate_result(&store, &parse(query).unwrap()),
            Err(EvalError::TypeMismatch),
            "{query}"
        );
    }
    // Non-date quoted text is a string predicate, which needs the Unicode backend before
    // its column type is checked. With that backend, the column type decides.
    let concepts = |codes: &[u64]| {
        Ok(QueryResult::Concepts(
            codes
                .iter()
                .map(|code| store.ids.binary_search(code).unwrap() as u32)
                .collect(),
        ))
    };
    for (query, with_unicode) in [
        (
            "^200001 {{M mapTarget=\"A12\"}}",
            concepts(&[300001, 300003]),
        ),
        (
            "^200001 {{M mapTarget=wild:\"A*\"}}",
            concepts(&[300001, 300003]),
        ),
        (
            "^200001 {{M effectiveTime=wild:\"2026*\"}}",
            Err(EvalError::TypeMismatch),
        ),
        (
            "^200001 {{M targetComponentId=\"J45\"}}",
            Err(EvalError::TypeMismatch),
        ),
        (
            "^200001 {{M mapGroup=match:\"1\"}}",
            Err(EvalError::TypeMismatch),
        ),
        (
            "^200001 {{M grouped=\"yes\"}}",
            Err(EvalError::TypeMismatch),
        ),
    ] {
        let result = evaluate_result(&store, &parse(query).unwrap());
        if cfg!(feature = "unicode") {
            assert_eq!(result, with_unicode, "{query}");
        } else {
            assert!(
                matches!(result, Err(EvalError::Unsupported(_))),
                "{query}: {result:?}"
            );
        }
    }
}

#[test]
fn member_queries_on_description_based_reference_sets_are_semantic_errors() {
    use snomed_ecl_engine::store::MembershipIndex;
    let mut store = fixture();
    // 200001 has concept members; 100001 stands in for a language reference set; 400002 is a
    // concept-based reference set whose rows are all inactive.
    let mut inactive = table();
    inactive.refset = 400002;
    inactive.columns[1] = C::Boolean(vec![0; 4]);
    store.member_tables = MemberStore::loaded(vec![table(), inactive]).unwrap();
    let mut membership = MembershipIndex::build(store.ids.len(), vec![(1, 2), (1, 4)]).unwrap();
    membership.concept_refsets = Some(vec![200001, 400002]);
    membership.non_concept_refsets = Some(vec![100001]);
    membership.validate(store.ids.len()).unwrap();
    store.membership = Some(membership);
    for query in [
        "^100001",
        "^[referencedComponentId]100001",
        "^[*]100001",
        "^100001 {{M active=0}}",
        "^100001 {{M active=\"*\"}}",
        "^[mapTarget,mapGroup]100001 {{M active=\"*\"}}",
        "^(100001 OR 400001) {{M active=1}}",
    ] {
        assert!(
            matches!(
                evaluate_result(&store, &parse(query).unwrap()),
                Err(EvalError::Semantic(message)) if message.contains("100001")
            ),
            "{query}"
        );
    }
    // Wildcard and mixed selections still return the concept-based rows.
    assert_eq!(codes(&store, "^* {{M mapGroup=#1}}"), vec![300001]);
    assert_eq!(
        codes(&store, "^(100001 OR 200001) {{M mapGroup=#1}}"),
        vec![300001]
    );
    assert_eq!(
        codes(&store, "^[targetComponentId]*"),
        vec![400001, 400002, 400003]
    );
    assert_eq!(codes(&store, "^R (300001 OR 100001)"), vec![200001]);
    assert_eq!(codes(&store, "^R * {{M active=0}}"), vec![200001, 400002]);
    // Inactive members of a concept-based set stay reachable through the active predicate,
    // alone and beside a description-based set; the domain does not depend on active rows.
    assert_eq!(codes(&store, "^400002"), Vec::<u64>::new());
    assert_eq!(
        codes(&store, "^400002 {{M active=0}}"),
        vec![300001, 300002, 300003]
    );
    assert_eq!(
        codes(&store, "^400002 {{M active=\"*\"}}"),
        vec![300001, 300002, 300003]
    );
    assert_eq!(
        codes(&store, "^(100001 OR 400002) {{M active=0}}"),
        vec![300001, 300002, 300003]
    );
    assert_eq!(codes(&store, "^(100001 OR 400002)"), Vec::<u64>::new());
    assert_eq!(
        codes(
            &store,
            "^[targetComponentId](100001 OR 400002) {{M active=0, mapGroup=#2}}"
        ),
        vec![400002, 400003]
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

#[test]
fn rows_found_through_an_identifier_column_keep_table_order() {
    // Enough rows that one referenced concept is looked up rather than scanned.
    let rows = 400usize;
    let referenced: Vec<u64> = (0..rows)
        .map(|i| [300001, 300002, 300003][i * 7 % 3])
        .collect();
    let targets: Vec<u64> = (0..rows).map(|i| [400001, 400002, 400003][i % 3]).collect();
    let mut text = TextColumn::default();
    for i in 0..rows {
        text.push(&format!("T{i}")).unwrap();
    }
    let mut base = table();
    base.columns = vec![
        C::Time(vec![20260826; rows]),
        C::Boolean((0..rows).map(|i| u8::from(i % 5 != 0)).collect()),
        C::Id(vec![100001; rows]),
        C::Id(referenced.clone()),
        C::Integer((0..rows).map(|i| (i % 4) as i64).collect()),
        C::Text(text),
        C::Id(targets.clone()),
        C::Boolean(vec![0; rows]),
        C::Time(vec![20260826; rows]),
    ];
    let mut store = fixture();
    store.member_tables = MemberStore::loaded(vec![base]).unwrap();
    let QueryResult::Rows(found) = evaluate_result(
        &store,
        &parse("^[mapTarget, targetComponentId] 200001 {{M referencedComponentId = 300002}}")
            .unwrap(),
    )
    .unwrap() else {
        panic!()
    };
    let expected: Vec<String> = (0..rows)
        .filter(|&i| i % 5 != 0 && referenced[i] == 300002)
        .map(|i| format!("T{i}"))
        .collect();
    let actual: Vec<String> = found
        .iter()
        .map(|row| match &row["mapTarget"] {
            MemberValue::String(text) => text.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(
        codes(
            &store,
            "^[targetComponentId] 200001 {{M referencedComponentId = 300003, mapGroup = #2}}"
        ),
        (0..rows)
            .filter(|&i| i % 5 != 0 && referenced[i] == 300003 && i % 4 == 2)
            .map(|i| targets[i])
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
}

#[test]
fn member_uuid_and_refset_id_are_not_fields() {
    // Appendix E names reference set fields from referencedComponentId on; the
    // member UUID and refsetId are metadata ECL gives no meaning, and are not stored.
    let store = fixture();
    for (query, field) in [
        ("^200001 {{M id=#1}}", "id"),
        (
            "^200001 {{M id=\"01010101-0101-0101-0101-010101010101\"}}",
            "id",
        ),
        ("^[id] 200001", "id"),
        ("^200001 {{M refsetId=200001}}", "refsetid"),
        ("^[referencedComponentId, refsetId] 200001", "refsetid"),
    ] {
        assert_eq!(
            evaluate_result(&store, &parse(query).unwrap()),
            Err(EvalError::InvalidField(field.into())),
            "{query}"
        );
    }
}
