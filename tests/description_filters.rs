use snomed_ecl_engine::ecl::parse;
use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
use snomed_ecl_engine::store::{Description, DescriptionIndex, DescriptionStore, NumericStore};
use std::sync::atomic::AtomicBool;

const SYN: u64 = 900000000000013009;
const FSN: u64 = 900000000000003001;
const DEF: u64 = 900000000000550004;
const GB: u64 = 900000000000508004;
const US: u64 = 900000000000509007;
const PREF: u64 = 900000000000548007;
const ACCEPT: u64 = 900000000000549004;

fn fixture() -> NumericStore {
    let mut ids = vec![
        100001, 100002, 100003, 100004, 100005, SYN, FSN, DEF, GB, US, PREF, ACCEPT,
    ];
    ids.sort_unstable();
    let ordinal = |code| ids.binary_search(&code).unwrap() as u32;
    let rows = [
        (200001, 100001, SYN, true, "en", 20260101, vec![(GB, PREF)]),
        (
            200002,
            100001,
            FSN,
            true,
            "sv",
            20260202,
            vec![(US, ACCEPT)],
        ),
        (200003, 100002, SYN, false, "en", 20260303, vec![]),
        (
            200004,
            100003,
            SYN,
            true,
            "en",
            0,
            vec![(GB, ACCEPT), (US, PREF)],
        ),
        (200005, 100004, DEF, true, "en", 20260404, vec![]),
    ]
    .into_iter()
    .map(
        |(id, concept, kind, active, language, effective_time, dialects)| Description {
            id,
            concept: ordinal(concept),
            kind: ordinal(kind),
            module: ordinal(100005),
            active,
            language: language.as_bytes().try_into().unwrap(),
            effective_time,
            term: format!("Synthetic term {id}"),
            dialects: dialects
                .into_iter()
                .map(|(r, a)| (ordinal(r), ordinal(a)))
                .collect(),
        },
    )
    .collect();
    NumericStore {
        descriptions: DescriptionStore::loaded(DescriptionIndex::build(ids.len(), rows).unwrap()),
        flags: vec![1; ids.len()],
        ids,
        ..NumericStore::default()
    }
}

fn expand(store: &NumericStore, ecl: &str) -> Vec<u64> {
    evaluate(store, &parse(ecl).unwrap_or_else(|e| panic!("{ecl}: {e}")))
        .unwrap_or_else(|e| panic!("{ecl}: {e}"))
        .into_iter()
        .map(|r| store.ids[r as usize])
        .collect()
}

#[test]
fn description_filters_share_a_row_within_each_block() {
    let store = fixture();
    for (ecl, expected) in [
        ("* {{D language=en}}", vec![100001, 100003, 100004]),
        ("* {{language=(EN sv)}}", vec![100001, 100003, 100004]),
        ("* {{D language=en, type=fsn}}", vec![]),
        ("* {{D language=en}} {{D type=fsn}}", vec![100001]),
        ("* {{D type=(syn def)}}", vec![100001, 100003, 100004]),
        (
            "* {{D typeId=(900000000000013009 OR 900000000000550004)}}",
            vec![100001, 100003, 100004],
        ),
        ("* {{D active=0}}", vec![100002]),
        ("* {{D active!=true}}", vec![100002]),
        ("* {{D active=*}}", vec![100001, 100002, 100003, 100004]),
        ("* {{D active!=*}}", vec![]),
        ("* {{D id=(200002 200003)}}", vec![100001]),
        ("* {{D id=200003, active=*}}", vec![100002]),
        ("* {{D id!=200001}}", vec![100001, 100003, 100004]),
        (
            "* {{D moduleId=100005, effectiveTime >= \"20260201\"}}",
            vec![100001, 100004],
        ),
        ("* {{moduleId=100005}}", vec![100001, 100003, 100004]),
        ("* {{MODULEID=100005}}", vec![100001, 100003, 100004]),
        ("* {{D effectiveTime=\"\"}}", vec![100003]),
        ("* {{D effectiveTime!=\"\"}}", vec![100001, 100004]),
        (
            "* {{D effectiveTime=(\"\" \"20260404\")}}",
            vec![100003, 100004],
        ),
    ] {
        assert_eq!(expand(&store, ecl), expected, "{ecl}");
    }
}

#[test]
fn dialects_bind_acceptability_to_the_same_language_member() {
    let store = fixture();
    for (ecl, expected) in [
        ("* {{dialect=en-gb}}", vec![100001, 100003]),
        ("* {{D dialect=en-gb (prefer)}}", vec![100001]),
        ("* {{D dialect=en-us (prefer)}}", vec![100003]),
        (
            "* {{D dialect=(en-gb (prefer) en-us (accept))}}",
            vec![100001],
        ),
        (
            "* {{D dialect=(en-gb en-us) (prefer)}}",
            vec![100001, 100003],
        ),
        (
            "* {{D dialectId=900000000000508004 (900000000000548007)}}",
            vec![100001],
        ),
        (
            "* {{D dialectId=(900000000000508004 (prefer) 900000000000509007 (accept))}}",
            vec![100001],
        ),
        (
            "* {{D dialectId=(900000000000508004 OR 900000000000509007)}}",
            vec![100001, 100003],
        ),
        ("* {{D dialect!=en-gb}}", vec![100001, 100004]),
        ("* {{D dialect=en-gb, dialect=en-us}}", vec![100003]),
        (
            "* {{D dialect=en-gb}} {{D dialect=en-us}}",
            vec![100001, 100003],
        ),
    ] {
        assert_eq!(expand(&store, ecl), expected, "{ecl}");
    }
}

#[test]
fn description_limits_and_missing_data_never_produce_partial_results() {
    let store = fixture();
    let query = parse("* {{D language=en}}").unwrap();
    assert_eq!(
        evaluate_with_limits(
            &store,
            &query,
            Limits {
                max_work: 2,
                ..Limits::default()
            },
            None
        ),
        Err(EvalError::WorkLimit)
    );
    assert_eq!(
        evaluate_with_limits(
            &store,
            &query,
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
            &query,
            Limits::default(),
            Some(&AtomicBool::new(true))
        ),
        Err(EvalError::Cancelled)
    );
    assert!(matches!(
        evaluate(&store, &parse("999999 {{D moduleId=^100001}}").unwrap()),
        Err(EvalError::Unsupported(_))
    ));
    assert!(matches!(
        evaluate(
            &NumericStore::default(),
            &parse("999999 {{D active=*}}").unwrap()
        ),
        Err(EvalError::Unsupported(_))
    ));
    for invalid in [
        "* {{D language=eng}}",
        "* {{D type=banana}}",
        "* {{D language=()}}",
        "* {{D id=012345}}",
        "* {{D active>0}}",
        "* {{D dialect=()}}",
    ] {
        assert!(parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn configured_dialect_aliases_resolve_at_evaluation() {
    let mut store = fixture();
    store
        .config
        .dialects
        .insert("local-dialect".into(), 900000000000508004);
    assert_eq!(
        evaluate(
            &store,
            &parse("* {{D dialect=local-dialect (prefer)}}").unwrap()
        )
        .unwrap(),
        evaluate(
            &store,
            &parse("* {{D dialectId=900000000000508004 (prefer)}}").unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        evaluate(&store, &parse("* {{D dialect=unknown-dialect}}").unwrap()),
        Err(EvalError::UnconfiguredAlias("unknown-dialect".into()))
    );
    for (brief, long) in [
        (
            "* {{D type=(syn fsn def)}}",
            "* {{D type=(synonym fullySpecifiedName definition)}}",
        ),
        (
            "* {{D dialect=en-gb (prefer accept)}}",
            "* {{D dialect=en-gb (preferred acceptable)}}",
        ),
    ] {
        assert_eq!(parse(brief).unwrap(), parse(long).unwrap());
        assert_eq!(
            evaluate(&store, &parse(brief).unwrap()),
            evaluate(&store, &parse(long).unwrap())
        );
    }
}
