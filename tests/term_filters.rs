use snomed_ecl_engine::ecl::{parse, DescriptionFilter, Expr, SearchTerm};

#[test]
fn typed_search_terms_preserve_literal_wildcards_and_word_order() {
    let Expr::DescriptionFiltered(_, filters) =
        parse(r#"* {{term=(match:"  gas pain " wild:"*itis" wild:"a\*b")}}"#).unwrap()
    else {
        panic!()
    };
    let DescriptionFilter::Term(_, terms) = &filters[0] else {
        panic!()
    };
    assert_eq!(
        terms,
        &[
            SearchTerm::Match(vec!["gas".into(), "pain".into()]),
            SearchTerm::Wild(vec![None, Some("itis".into())]),
            SearchTerm::Wild(vec![Some("a*b".into())]),
        ]
    );
    for bad in [
        r#"* {{term=""}}"#,
        r#"* {{term="   "}}"#,
        r#"* {{term=wild:""}}"#,
        r#"* {{term=()}}"#,
        r#"* {{term=("a""b")}}"#,
        r#"* {{term=match:"a\*b"}}"#,
        r#"* {{term=wild:"a\qb"}}"#,
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    assert!(parse(&format!("* {{{{term=\"{}\"}}}}", "a ".repeat(4096))).is_err());
}

#[cfg(not(feature = "unicode"))]
#[test]
fn missing_unicode_backend_is_explicit_even_for_empty_candidates() {
    use snomed_ecl_engine::{
        eval::{evaluate, EvalError},
        store::NumericStore,
    };
    assert!(matches!(
        evaluate(
            &NumericStore::default(),
            &parse(r#"999999 {{term="gas"}}"#).unwrap()
        ),
        Err(EvalError::Unsupported(_))
    ));
}

#[cfg(feature = "unicode")]
mod unicode {
    use super::*;
    use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
    use snomed_ecl_engine::store::{Description, DescriptionIndex, DescriptionStore, NumericStore};

    fn fixture() -> NumericStore {
        let mut ids = vec![
            100001,
            100002,
            100003,
            100004,
            100005,
            900000000000013009,
            900000000000003001,
        ];
        ids.sort_unstable();
        let ordinal = |id| ids.binary_search(&id).unwrap() as u32;
        let data = [
            (100001, "gastric pain", true, *b"en"),
            (100001, "Stomach disorder (disorder)", true, *b"en"),
            (100002, "bronchitis", true, *b"en"),
            (100002, "Bronchitis (disorder)", true, *b"en"),
            (100003, "gastroenteritis", true, *b"en"),
            (100003, "gas gangrene", true, *b"en"),
            (100003, "obsolete gas term", false, *b"en"),
            (100004, "RÉSUMÉ syndrome", true, *b"en"),
            (100004, "sjögren syndrome", true, *b"sv"),
            (100005, "SJØGREN syndrome", true, *b"en"),
            (100005, "a*b", true, *b"en"),
        ];
        let rows = data
            .into_iter()
            .enumerate()
            .map(|(i, (concept, term, active, language))| Description {
                id: 200001 + i as u64,
                concept: ordinal(concept),
                module: ordinal(100005),
                kind: ordinal(900000000000013009),
                effective_time: 20260826,
                active,
                language,
                term: term.into(),
                dialects: Vec::new(),
            })
            .collect();
        NumericStore {
            descriptions: DescriptionStore::loaded(
                DescriptionIndex::build(ids.len(), rows).unwrap(),
            ),
            flags: vec![1; ids.len()],
            ids,
            ..NumericStore::default()
        }
    }

    #[test]
    fn prefixes_wildcards_and_asymmetric_unicode_match_complete_expected_sets() {
        let store = fixture();
        for (ecl, expected) in [
            (
                r#"* {{term=(match:"gas" wild:"*itis")}}"#,
                vec![100001, 100002, 100003],
            ),
            (r#"* {{term="gas"}}"#, vec![100001, 100003]),
            (r#"* {{term="pain GAS"}}"#, vec![100001]),
            (r#"* {{term="gas",term!="gas"}}"#, vec![]),
            (r#"100001 {{term!="gas"}}"#, vec![100001]),
            (r#"100003 {{term!="gas"}}"#, vec![]),
            (r#"* {{term=wild:"*itis"}}"#, vec![100002, 100003]),
            (r#"* {{term=wild:"gas*itis"}}"#, vec![100003]),
            (r#"* {{term=wild:"*gas*itis"}}"#, vec![100003]),
            (
                r#"* {{term=wild:"*"}}"#,
                vec![100001, 100002, 100003, 100004, 100005],
            ),
            (r#"* {{term=wild:"a\*b"}}"#, vec![100005]),
            (r#"* {{term=wild:"gastric"}}"#, vec![]),
            (r#"* {{term=wild:"gastric pain"}}"#, vec![100001]),
            (r#"* {{term=wild:"*gas*gas*"}}"#, vec![]),
            (r#"* {{term="gas",term="stomach"}}"#, vec![]),
            (r#"* {{term="gas"}} {{term="stomach"}}"#, vec![100001]),
            (r#"* {{term="gas",term=wild:"*itis"}}"#, vec![100003]),
            (r#"* {{term="resume"}}"#, vec![100004]),
            (r#"* {{term="résumé"}}"#, vec![100004]),
            (r#"* {{term="sjogren"}}"#, vec![100005]),
            (r#"* {{term="sjögren"}}"#, vec![100004]),
            (r#"* {{term="obsolete"}}"#, vec![]),
            (r#"* {{term="obsolete",active=0}}"#, vec![100003]),
            (r#"* {{term!=wild:"*"}}"#, vec![]),
        ] {
            let result: Vec<_> = evaluate(&store, &parse(ecl).unwrap())
                .unwrap()
                .into_iter()
                .map(|r| store.ids[r as usize])
                .collect();
            assert_eq!(result, expected, "{ecl}");
        }
        let query = parse(r#"* {{term="gas"}}"#).unwrap();
        assert_eq!(
            evaluate_with_limits(
                &store,
                &query,
                Limits {
                    max_work: 30,
                    ..Limits::default()
                },
                None
            ),
            Err(EvalError::WorkLimit)
        );
    }

    #[test]
    fn text_definitions_match_unless_the_query_restricts_description_type() {
        let store = NumericStore {
            ids: vec![100001, 900000000000550004],
            flags: vec![1, 1],
            descriptions: DescriptionStore::loaded(
                DescriptionIndex::build(
                    2,
                    vec![Description {
                        id: 200001,
                        concept: 0,
                        module: 1,
                        kind: 1,
                        effective_time: 20260826,
                        active: true,
                        language: *b"en",
                        term: "gas appears in this synthetic definition".into(),
                        dialects: vec![],
                    }],
                )
                .unwrap(),
            ),
            ..NumericStore::default()
        };
        for (ecl, expected) in [
            (r#"100001 {{term="gas"}}"#, vec![0]),
            (r#"100001 {{term="gas",type=def}}"#, vec![0]),
            (r#"100001 {{term="gas",type=(syn fsn)}}"#, vec![]),
        ] {
            assert_eq!(
                evaluate(&store, &parse(ecl).unwrap()).unwrap(),
                expected,
                "{ecl}"
            );
        }
    }
}
