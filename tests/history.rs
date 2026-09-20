use snomed_ecl_engine::ecl::parse;
use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, EvalError, Limits};
use snomed_ecl_engine::store::{
    Adjacency, MemberColumn as C, MemberStore, MemberTable, NumericStore,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

const ROOT: u64 = 900000000000522004;
const SAME: u64 = 900000000000527005;
const REPLACED: u64 = 900000000000526001;
const WAS: u64 = 900000000000528000;
const PARTIAL: u64 = 1186924009;
const POSSIBLE: u64 = 900000000000523009;
const FROM: u64 = 900000000000525002;
const TO: u64 = 900000000000524003;
type Row = (u64, u64, u64, bool);

fn fixture() -> (NumericStore, Vec<Row>) {
    let rows = vec![
        (SAME, 100003001, 100001001, true),
        (REPLACED, 100004001, 100002001, true),
        (POSSIBLE, 100005001, 100001001, true),
        (FROM, 100001001, 100006001, true),
        (SAME, 100007001, 100003001, true),
        (SAME, 100008001, 100001001, false),
        (PARTIAL, 100009001, 100001001, true),
        (WAS, 100010001, 100001001, true),
        (TO, 100011001, 100001001, true),
    ];
    let mut ids: Vec<_> = (100001001..=100011001)
        .step_by(1000)
        .chain([ROOT, SAME, REPLACED, WAS, PARTIAL, POSSIBLE, FROM, TO])
        .collect();
    ids.sort_unstable();
    let ordinal = |id| ids.binary_search(&id).unwrap() as u32;
    let mut edges = vec![(ordinal(100002001), ordinal(100001001))];
    edges.extend(
        [SAME, REPLACED, WAS, PARTIAL, POSSIBLE, FROM, TO].map(|id| (ordinal(id), ordinal(ROOT))),
    );
    let parents = Adjacency::build(ids.len(), edges.clone()).unwrap();
    let children =
        Adjacency::build(ids.len(), edges.into_iter().map(|(a, b)| (b, a)).collect()).unwrap();
    let mut groups: BTreeMap<u64, Vec<_>> = BTreeMap::new();
    for (i, &(refset, source, target, active)) in rows.iter().enumerate() {
        groups
            .entry(refset)
            .or_default()
            .push((i, source, target, active));
    }
    let tables = groups
        .into_iter()
        .map(|(refset, rows)| MemberTable {
            refset,
            names: [
                "id",
                "effectiveTime",
                "active",
                "moduleId",
                "refsetId",
                "referencedComponentId",
                "targetComponentId",
            ]
            .map(str::to_owned)
            .to_vec(),
            columns: vec![
                C::Uuid(rows.iter().map(|r| [r.0 as u8; 16]).collect()),
                C::Time(vec![20260826; rows.len()]),
                C::Boolean(rows.iter().map(|r| u8::from(r.3)).collect()),
                C::Id(vec![100001001; rows.len()]),
                C::Id(vec![refset; rows.len()]),
                C::Id(rows.iter().map(|r| r.1).collect()),
                C::Id(rows.iter().map(|r| r.2).collect()),
            ],
        })
        .collect();
    let flags = ids
        .iter()
        .map(|id| u8::from(!(100003001..=100011001).contains(id)))
        .collect();
    (
        NumericStore {
            ids,
            flags,
            parents,
            children,
            member_tables: MemberStore::loaded(tables).unwrap(),
            ..NumericStore::default()
        },
        rows,
    )
}
fn codes(store: &NumericStore, query: &str) -> Vec<u64> {
    evaluate(store, &parse(query).unwrap())
        .unwrap()
        .into_iter()
        .map(|o| store.ids[o as usize])
        .collect()
}

#[test]
fn profiles_subsets_and_moved_from_preserve_one_hop_semantics() {
    let (store, _) = fixture();
    for (suffix, expected) in [
        ("-MIN", vec![100001001, 100002001, 100003001, 100006001]),
        (
            "_mOd",
            vec![
                100001001, 100002001, 100003001, 100004001, 100006001, 100009001, 100010001,
            ],
        ),
        (
            "-MAX",
            vec![
                100001001, 100002001, 100003001, 100004001, 100005001, 100006001, 100009001,
                100010001,
            ],
        ),
        (
            " (*)",
            vec![
                100001001, 100002001, 100003001, 100004001, 100005001, 100006001, 100009001,
                100010001,
            ],
        ),
        (
            "",
            vec![
                100001001, 100002001, 100003001, 100004001, 100005001, 100006001, 100009001,
                100010001,
            ],
        ),
    ] {
        assert_eq!(
            codes(&store, &format!("<<100001001 {{{{ + HISTORY{suffix} }}}}")),
            expected
        );
    }
    assert_eq!(
        codes(
            &store,
            &format!("<<100001001 {{{{+HISTORY ({REPLACED})}}}}")
        ),
        [100001001, 100002001, 100004001]
    );
    assert_eq!(
        codes(&store, "(100001001 {{+HISTORY-MIN}}) {{+HISTORY-MIN}}"),
        [100001001, 100003001, 100006001, 100007001]
    );
    assert_eq!(
        codes(&store, "100001001 {{+HISTORY-MIN}} MINUS 100006001"),
        [100001001, 100003001]
    );
}

#[test]
fn generated_history_subsets_match_an_independent_row_scan() {
    let (store, rows) = fixture();
    for seed in (100001001..=100011001).step_by(1000) {
        for bits in 1..16 {
            let selected: Vec<_> = [SAME, REPLACED, PARTIAL, POSSIBLE]
                .into_iter()
                .enumerate()
                .filter_map(|(i, id)| (bits & (1 << i) != 0).then_some(id))
                .collect();
            let mut expected = BTreeSet::from([seed]);
            for &(refset, source, target, active) in &rows {
                if active && selected.contains(&refset) && target == seed {
                    expected.insert(source);
                }
                if active && refset == FROM && selected.contains(&SAME) && source == seed {
                    expected.insert(target);
                }
            }
            let subset = selected
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(" OR ");
            assert_eq!(
                codes(&store, &format!("{seed} {{{{+HISTORY ({subset})}}}}")),
                expected.into_iter().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn history_syntax_types_and_limits_fail_explicitly() {
    let (store, _) = fixture();
    for query in [
        "100001001 {{+HISTORY-MIN (*)}}",
        "100001001 {{+HISTORY -MIN}}",
        "100001001 {{+HISTORY-OTHER}}",
        "100001001 {{+HISTORY()}}",
        "100001001 {{+HISTORY}} {{C active=1}}",
        "100001001 {{+HISTORY}} {{+HISTORY}}",
    ] {
        assert!(parse(query).is_err(), "{query}");
    }
    assert_eq!(
        parse("100001001 {{+HiStOrY_min}}").unwrap(),
        parse("100001001 {{+HISTORY-MIN}}").unwrap()
    );
    let query = parse("100001001 {{+HISTORY-MAX}}").unwrap();
    assert!(matches!(
        evaluate(&NumericStore::default(), &query),
        Err(EvalError::Unsupported(_))
    ));
    assert_eq!(
        evaluate_with_limits(
            &store,
            &query,
            Limits::default(),
            Some(&AtomicBool::new(true))
        ),
        Err(EvalError::Cancelled)
    );
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
}
