use snomed_ecl_engine::config::QueryConfig;
use snomed_ecl_engine::ecl::{parse, Expr};
use snomed_ecl_engine::eval::{evaluate, EvalError};
use snomed_ecl_engine::store::{
    Adjacency, Identifier, IdentifierIndex, IdentifierStore, NumericStore,
};

fn fixture() -> NumericStore {
    let mut rows = Vec::new();
    for (scheme, code, reference, active) in [
        (200001, "A.1", 100001, true),
        (200001, "B-2", 100002, true),
        (200001, "old", 100003, false),
        (200001, "desc", 100011, true),
        (300001, "A.1", 100003, true),
        (200001, "café + test", 100003, true),
        (200001, "a\u{85}", 100001, true),
    ] {
        rows.push(Identifier {
            scheme,
            code: code.into(),
            referenced_component: reference,
            module: 200001,
            effective_time: 20260826,
            active,
        });
    }
    let mut config = QueryConfig::default();
    config.identifier_schemes.insert("demo".into(), 200001);
    config.identifier_schemes.insert("other".into(), 300001);
    NumericStore {
        ids: vec![100001, 100002, 100003, 200001, 300001],
        flags: vec![1, 1, 0, 1, 1],
        parents: Adjacency::build(5, vec![(1, 0)]).unwrap(),
        children: Adjacency::build(5, vec![(0, 1)]).unwrap(),
        identifiers: IdentifierStore::loaded(IdentifierIndex::build(rows).unwrap()).unwrap(),
        config,
        ..NumericStore::default()
    }
}
#[test]
fn alternate_identifiers_resolve_schemes_exact_codes_and_active_associations() {
    let store = fixture();
    for (query, expected) in [
        ("demo#A.1", vec![100001]),
        ("DEMO#B-2", vec![100002]),
        ("demo#old", vec![]),
        ("demo#desc", vec![]),
        ("other#A.1", vec![100003]),
        ("demo#a.1", vec![]),
        ("< demo#A.1", vec![100002]),
        ("\"demo#café + test\"", vec![100003]),
        ("demo#\"café + test\"", vec![100003]),
        ("\"demo#a\u{85}\"", vec![100001]),
        (
            "demo#A.1 |Ignored label| OR other#A.1",
            vec![100001, 100003],
        ),
    ] {
        let actual: Vec<_> = evaluate(&store, &parse(query).unwrap())
            .unwrap()
            .into_iter()
            .map(|o| store.ids[o as usize])
            .collect();
        assert_eq!(actual, expected, "{query}");
    }
    assert_eq!(
        evaluate(&store, &parse("unknown#A.1").unwrap()),
        Err(EvalError::UnconfiguredAlias("unknown".into()))
    );
    assert!(matches!(
        parse("demo#A.1").unwrap(),
        Expr::AlternateIdentifier { .. }
    ));
    for alias in [
        "top",
        "bottom",
        "descendantOf",
        "memberOf",
        "refsetContainingAny",
        "any",
        "true",
        "R",
    ] {
        assert!(matches!(
            parse(&format!("{alias}#A.1")).unwrap(),
            Expr::AlternateIdentifier { .. }
        ));
        assert!(parse(&format!("* : {alias}#kind = {alias}#A.1")).is_ok());
    }
}
#[test]
fn alias_configuration_and_identifier_keys_are_validated() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("aliases.json");
    std::fs::write(
        &path,
        r#"{"identifier_schemes":{"DEMO":"200001"},"dialects":{"my-dialect":"300001"}}"#,
    )
    .unwrap();
    let config = QueryConfig::read(&path).unwrap();
    assert_eq!(config.identifier_schemes["demo"], 200001);
    assert_eq!(config.dialects["my-dialect"], 300001);
    for text in [
        r#"{"dialects":{"1bad":"300001"}}"#,
        r#"{"dialects":{"a":"300001","A":"300002"}}"#,
        r#"{"dialects":{"a":"0"}}"#,
        r#"{"unknown":{}}"#,
        r#"{"member_language":"eng"}"#,
        r#"{"member_language":"12"}"#,
    ] {
        std::fs::write(&path, text).unwrap();
        assert!(QueryConfig::read(&path).is_err());
    }
    let store = fixture();
    let mut rows = store.identifiers.get().unwrap().unwrap().rows.clone();
    rows.push(rows[0].clone());
    assert!(IdentifierIndex::build(rows).is_err());
    for query in [
        "demo#",
        "\"demo#\"",
        "1demo#abc",
        "\"demo#a\\b\"",
        "\"demo#abc",
        "demo#\"abc",
    ] {
        assert!(parse(query).is_err(), "{query}");
    }
}
