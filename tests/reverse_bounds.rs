//! Attribute refinements test only the concepts that can match.
//!
//! These compare the bounded path against a brute-force count, and against the
//! same refinement with an arm added that can never match but removes the
//! bound, so every focus concept is tested.

use snomed_ecl_engine::ecl::parse;
use snomed_ecl_engine::eval::{evaluate, evaluate_with_limits, Limits};
use snomed_ecl_engine::store::{Adjacency, Attribute, Attributes, NumericStore};
use std::collections::BTreeSet;

const ISA: u64 = 116680003;
const KINDS: [u32; 3] = [0, 1, 2];

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

/// Three attribute types at ordinals 0..3, is-a last, and `n` concepts
/// between, a tenth of them inactive.
fn store(n: usize, seed: u64) -> NumericStore {
    let total = n + 4;
    let isa = total as u32 - 1;
    let mut rng = Rng(seed);
    let mut rows = Vec::new();
    let mut parents = Vec::new();
    for source in 3..isa {
        for _ in 0..rng.below(4) {
            let kind = KINDS[rng.below(3) as usize];
            let value = 3 + rng.below(n as u64 / 8) as u32;
            let group = rng.below(3) as u32;
            rows.push((source, Attribute { group, kind, value }));
        }
        if source > 3 {
            parents.push((source, 3 + rng.below((source - 3) as u64) as u32));
            if rng.below(3) == 0 {
                parents.push((source, 3 + rng.below((source - 3) as u64) as u32));
            }
        }
    }
    parents.sort_unstable();
    parents.dedup();
    let flags = (0..total).map(|i| u8::from(i < 3 || i % 10 != 7)).collect();
    NumericStore {
        descriptions: Default::default(),
        search: Default::default(),
        history: Default::default(),
        member_tables: Default::default(),
        identifiers: Default::default(),
        config: Default::default(),
        ids: (0..isa as u64)
            .map(|i| 1_000_000 + i * 1000)
            .chain([ISA])
            .collect(),
        flags,
        modules: vec![0; total],
        effective_times: vec![20260826; total],
        parents: Adjacency::build(total, parents.clone()).unwrap(),
        children: Adjacency::build(total, parents.iter().map(|&(c, p)| (p, c)).collect()).unwrap(),
        attributes: Attributes::build(total, rows).unwrap(),
        concrete: Attributes::build(total, vec![]).unwrap(),
        concrete_values: vec![],
        membership: None,
    }
}

fn code(store: &NumericStore, ordinal: u32) -> u64 {
    store.ids[ordinal as usize]
}

fn run(store: &NumericStore, query: &str) -> Vec<u32> {
    evaluate(
        store,
        &parse(query).unwrap_or_else(|e| panic!("{query}: {e}")),
    )
    .unwrap_or_else(|e| panic!("{query}: {e}"))
}

/// Distinct active sources whose `kind` (or is-a) points at each concept.
fn pointed_at(store: &NumericStore, kind: u32, values: &BTreeSet<u32>, eq: bool) -> Vec<usize> {
    let mut counts = vec![0; store.ids.len()];
    for source in 0..store.ids.len() as u32 {
        if !store.is_active(source) || values.contains(&source) != eq {
            continue;
        }
        let mut targets: BTreeSet<u32> = store
            .attributes
            .get(source)
            .iter()
            .filter(|row| row.kind == kind)
            .map(|row| row.value)
            .collect();
        if store.ids[kind as usize] == ISA {
            targets.extend(store.parents.get(source));
        }
        for target in targets {
            counts[target as usize] += 1;
        }
    }
    counts
}

#[test]
fn bounded_reverse_matches_a_brute_force_count() {
    for seed in 0..6 {
        let store = store(400, seed);
        let isa = store.ids.len() as u32 - 1;
        let mut rng = Rng(seed + 100);
        for _ in 0..40 {
            let kind = [0, 1, 2, isa][rng.below(4) as usize];
            let value = 3 + rng.below(60) as u32;
            let values: BTreeSet<u32> = run(&store, &format!("<< {}", code(&store, value)))
                .into_iter()
                .collect();
            for eq in [true, false] {
                let counts = pointed_at(&store, kind, &values, eq);
                for (low, high) in [
                    (1, None),
                    (0, None),
                    (0, Some(0)),
                    (2, Some(3)),
                    (1, Some(1)),
                ] {
                    let bound = high.map_or("*".into(), |h: usize| h.to_string());
                    let query = format!(
                        "* : [{low}..{bound}] R {} {} << {}",
                        code(&store, kind),
                        if eq { "=" } else { "!=" },
                        code(&store, value)
                    );
                    let expected: Vec<u32> = (0..store.ids.len() as u32)
                        .filter(|&c| {
                            let count = counts[c as usize];
                            count >= low && high.is_none_or(|h| count <= h)
                        })
                        .collect();
                    assert_eq!(run(&store, &query), expected, "{query}");
                }
            }
        }
    }
}

#[test]
fn bounded_and_unbounded_refinements_agree() {
    for seed in 0..4 {
        let store = store(300, seed);
        let mut rng = Rng(seed + 7);
        // Every value is a concept, so this never holds, and != is never bounded.
        let never = format!("{} != *", code(&store, 0));
        let isa = store.ids.len() as u32 - 1;
        let clause = |rng: &mut Rng| {
            let kind = code(&store, [0, 1, 2, isa][rng.below(4) as usize]);
            let value = code(&store, 3 + rng.below(40) as u32);
            match rng.below(7) {
                5 => format!("{kind} = {value}"),
                6 => format!("[2..3] {kind} = << {value}"),
                0 => format!("R {kind} = << {value}"),
                1 => format!("[2..*] R {kind} = << {value}"),
                2 => format!("[0..1] R {kind} = << {value}"),
                3 => format!("{kind} = << {value}"),
                _ => format!("{{ {kind} = << {value} }}"),
            }
        };
        for _ in 0..150 {
            let refinement = match rng.below(3) {
                0 => clause(&mut rng),
                1 => format!("{} AND {}", clause(&mut rng), clause(&mut rng)),
                _ => format!("{} OR {}", clause(&mut rng), clause(&mut rng)),
            };
            let focus = match rng.below(3) {
                0 => "*".to_string(),
                1 => format!("<< {}", code(&store, 3 + rng.below(20) as u32)),
                _ => format!("(* MINUS << {})", code(&store, 3 + rng.below(20) as u32)),
            };
            let bounded = format!("{focus} : {refinement}");
            let unbounded = format!("{focus} : ({refinement}) OR {never}");
            assert_eq!(run(&store, &bounded), run(&store, &unbounded), "{bounded}");
        }
    }
}

#[test]
fn a_reverse_lookup_does_not_pay_for_the_edition() {
    let store = store(200_000, 1);
    let limits = Limits {
        max_work: 20_000,
        max_live_set_values: 20_000,
    };
    let query = format!("* : R {} = {}", code(&store, 0), code(&store, 5));
    let answer = evaluate_with_limits(&store, &parse(&query).unwrap(), limits, None)
        .expect("a reverse lookup on one value fits a budget far below the edition");
    let counts = pointed_at(&store, 0, &BTreeSet::from([5]), true);
    let expected: Vec<u32> = (0..store.ids.len() as u32)
        .filter(|&c| counts[c as usize] > 0)
        .collect();
    assert_eq!(answer, expected);
}

#[test]
fn a_large_focus_is_checked_from_the_candidates_upward() {
    // Foci far above the 4,096 concepts that are materialised outright.
    let store = store(40_000, 3);
    let isa = store.ids.len() as u32 - 1;
    let never = format!("{} != *", code(&store, 0));
    let mut rng = Rng(11);
    for i in 0..120 {
        let top = code(&store, 3 + rng.below(4) as u32);
        let focus = ["<<", "<", "<!", "<<!"][i % 4];
        let kind = code(&store, [0, 1, 2, isa][rng.below(4) as usize]);
        let value = code(&store, 3 + rng.below(4_000) as u32);
        let refinement = match i % 3 {
            0 => format!("{kind} = {value}"),
            1 => format!("R {kind} = {value}"),
            _ => format!("{kind} = << {value}"),
        };
        let bounded = format!("{focus} {top} : {refinement}");
        let unbounded = format!("{focus} {top} : ({refinement}) OR {never}");
        assert_eq!(run(&store, &bounded), run(&store, &unbounded), "{bounded}");
    }
}

#[test]
fn any_value_agrees_with_the_materialised_set_of_every_concept() {
    let store = store(2_000, 5);
    let isa = store.ids.len() as u32 - 1;
    let every = format!("(* OR {})", code(&store, 3));
    for kind in [0, 1, 2, isa] {
        let kind = code(&store, kind);
        for form in [
            "<< {top} : {kind} {op} {value}",
            "<< {top} : [2..*] {kind} {op} {value}",
            "<< {top} : [0..0] {kind} {op} {value}",
            "<< {top} : [0..1] {kind} {op} {value}",
            "<< {top} : {{ {kind} {op} {value} }}",
            "<< {top} : [1..1] {{ {kind} {op} {value}, {kind} {op} {value} }}",
            "* : R {kind} {op} {value}",
        ] {
            for op in ["=", "!="] {
                let query = |value: &str| {
                    form.replace("{top}", &code(&store, 3).to_string())
                        .replace("{kind}", &kind.to_string())
                        .replace("{op}", op)
                        .replace("{value}", value)
                        .replace("{{", "{")
                        .replace("}}", "}")
                };
                let any = query("*");
                assert_eq!(run(&store, &any), run(&store, &query(&every)), "{any}");
            }
        }
    }
}
