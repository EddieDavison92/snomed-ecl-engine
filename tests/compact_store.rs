use snomed_rust_ecl_engine::import::{import_snapshot, ImportOptions, UK_DISPLAY_REFSETS};
use snomed_rust_ecl_engine::store::{
    sha256, Adjacency, Attributes, ConcreteValue, DisplayStore, Manifest, NumericStore,
};
use std::collections::{BTreeSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;

const ROOT: u64 = 1000001;
const LEFT: u64 = 1000002;
const RIGHT: u64 = 1000003;
const LEAF: u64 = 1000004;
const INACTIVE: u64 = 1000005;
const KIND: u64 = 9000001;
const TARGET: u64 = 9000002;
const ISA: u64 = 116680003;

fn fixture(path: &Path, cycle: bool, duplicate: bool) {
    let mut archive = zip::ZipWriter::new(File::create(path).unwrap());
    let mut add = |name: &str, body: String| {
        archive
            .start_file(format!("Synthetic/{name}"), SimpleFileOptions::default())
            .unwrap();
        archive.write_all(body.as_bytes()).unwrap();
    };
    add(
        "release_package_information.json",
        r#"{"effectiveTime":"20260826"}"#.into(),
    );
    let mut concepts = "id\teffectiveTime\tactive\tmoduleId\tdefinitionStatusId\n".to_owned();
    for code in [ROOT, LEFT, RIGHT, LEAF, INACTIVE, KIND, TARGET, ISA] {
        concepts.push_str(&format!(
            "{code}\t20260826\t{}\t{ROOT}\t900000000000074008\n",
            u8::from(code != INACTIVE)
        ));
    }
    if duplicate {
        concepts.push_str(&format!(
            "{LEAF}\t20260826\t1\t{ROOT}\t900000000000074008\n"
        ));
    }
    add("Snapshot/Terminology/sct2_Concept_Snapshot.txt", concepts);
    let mut relationships = "id\teffectiveTime\tactive\tmoduleId\tsourceId\tdestinationId\trelationshipGroup\ttypeId\tcharacteristicTypeId\tmodifierId\n".to_owned();
    let mut edges = vec![
        (LEFT, ROOT, 0, ISA),
        (RIGHT, ROOT, 0, ISA),
        (LEAF, LEFT, 0, ISA),
        (LEAF, RIGHT, 0, ISA),
        (LEAF, TARGET, 1, KIND),
        (LEAF, TARGET, 2, KIND),
    ];
    if cycle {
        edges.push((ROOT, LEAF, 0, ISA));
    }
    for (i, (source, target, group, kind)) in edges.into_iter().enumerate() {
        relationships.push_str(&format!("{}\t20260826\t1\t{ROOT}\t{source}\t{target}\t{group}\t{kind}\t900000000000011006\t900000000000451002\n", 3000001 + i));
    }
    add(
        "Snapshot/Terminology/sct2_Relationship_Snapshot.txt",
        relationships,
    );
    add("Snapshot/Terminology/sct2_RelationshipConcreteValues_Snapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\tsourceId\tvalue\trelationshipGroup\ttypeId\tcharacteristicTypeId\tmodifierId\n4000001\t20260826\t1\t{ROOT}\t{LEAF}\t#0.100000000000000001\t2\t{KIND}\t900000000000011006\t900000000000451002\n4000002\t20260826\t1\t{ROOT}\t{LEAF}\t\"synthetic value\"\t3\t{KIND}\t900000000000011006\t900000000000451002\n"));
    add("Snapshot/Refset/der2_ssRefset_ModuleDependencySnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tsourceEffectiveTime\ttargetEffectiveTime\nsynthetic-dependency\t20260826\t1\t{ROOT}\t900000000000534007\t{LEFT}\t20260826\t20260826\n"));
    add("Snapshot/Refset/der2_cRefset_LanguageSnapshot.txt", "id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tacceptabilityId\nsynthetic-gb\t20260826\t1\t1000001\t900000000000508004\t6000002\t900000000000548007\nsynthetic-realm\t20260826\t1\t1000001\t999001261000000100\t6000003\t900000000000548007\n".into());
    add("Snapshot/Terminology/sct2_Description_Snapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\tconceptId\tlanguageCode\ttypeId\tterm\tcaseSignificanceId\n6000001\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic synonym\t900000000000448009\n6000002\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic GB label\t900000000000448009\n6000003\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic realm label\t900000000000448009\n6000004\t20260826\t1\t{ROOT}\t{ROOT}\ten\t900000000000003001\tSynthetic root (test)\t900000000000448009\n6000005\t20260826\t0\t{ROOT}\t{LEAF}\ten\t900000000000013009\tInactive label\t900000000000448009\n"));
    archive.finish().unwrap();
}

fn options(path: &Path) -> ImportOptions {
    ImportOptions {
        edition: format!("http://snomed.info/sct/{ROOT}/version/20260826"),
        expected_sha256: sha256(path).unwrap(),
        display_refsets: UK_DISPLAY_REFSETS.to_vec(),
    }
}

#[test]
fn roundtrip_preserves_groups_precision_and_separate_displays() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let manifest = import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    assert_eq!(manifest.active_concept_count, 7);
    let store = NumericStore::open(&destination).unwrap();
    assert_eq!(
        store.hierarchy(ROOT, false, false, false),
        [LEFT, RIGHT, LEAF]
    );
    assert_eq!(
        store.hierarchy(LEAF, true, false, true),
        [ROOT, LEFT, RIGHT, LEAF]
    );
    assert!(store.hierarchy(INACTIVE, false, false, true).is_empty());
    assert!(store.hierarchy(7777777, false, false, true).is_empty());
    let ordinal = store.ordinal(LEAF).unwrap();
    let rows = store.attributes.get(ordinal);
    assert_eq!(rows.iter().map(|r| r.group).collect::<Vec<_>>(), [1, 2]);
    assert!(rows
        .iter()
        .all(|r| store.ids[r.kind as usize] == KIND && store.ids[r.value as usize] == TARGET));
    assert!(store
        .concrete_values
        .contains(&ConcreteValue::Number("#0.100000000000000001".into())));
    assert!(store
        .concrete_values
        .contains(&ConcreteValue::Text("\"synthetic value\"".into())));
    let mut display = DisplayStore::open(&destination).unwrap();
    assert_eq!(
        display.get(ordinal).unwrap().as_deref(),
        Some("Synthetic realm label")
    );
    assert_eq!(
        display
            .get(store.ordinal(ROOT).unwrap())
            .unwrap()
            .as_deref(),
        Some("Synthetic root (test)")
    );
    assert_eq!(display.get(store.ordinal(INACTIVE).unwrap()).unwrap(), None);
    drop(display);
    fs::rename(
        destination.join("display.bin"),
        destination.join("display.saved"),
    )
    .unwrap();
    assert_eq!(
        NumericStore::open(&destination)
            .unwrap()
            .hierarchy(ROOT, false, false, false),
        [LEFT, RIGHT, LEAF]
    );
    assert!(DisplayStore::open(&destination).is_err());
    assert!(import_snapshot(&archive, &destination, &options(&archive)).is_err());
}

#[test]
fn import_rejects_cycles_duplicates_mismatched_release_and_checksum() {
    let temp = TempDir::new().unwrap();
    for (name, cycle, duplicate) in [("cycle", true, false), ("duplicate", false, true)] {
        let archive = temp.path().join(format!("{name}.zip"));
        fixture(&archive, cycle, duplicate);
        let destination = temp.path().join(name);
        assert!(import_snapshot(&archive, &destination, &options(&archive)).is_err());
        assert!(!destination.exists());
    }
    let archive = temp.path().join("valid.zip");
    fixture(&archive, false, false);
    let mut changed = options(&archive);
    changed.edition = format!("http://snomed.info/sct/{ROOT}/version/20260825");
    assert!(import_snapshot(&archive, &temp.path().join("date"), &changed).is_err());
    changed = options(&archive);
    changed.expected_sha256 = "0".repeat(64);
    assert!(import_snapshot(&archive, &temp.path().join("hash"), &changed).is_err());
}

#[test]
fn reader_rejects_corruption_and_forged_section_lengths() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let core = destination.join("core.bin");
    let original = fs::read(&core).unwrap();
    fs::write(&core, &original[..original.len() - 1]).unwrap();
    assert!(NumericStore::open(&destination).is_err());
    let mut forged = original;
    forged[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    fs::write(&core, forged).unwrap();
    let mut manifest = Manifest::read(&destination).unwrap();
    manifest.core_sha256 = sha256(&core).unwrap();
    serde_json::to_writer(
        File::create(destination.join("manifest.json")).unwrap(),
        &manifest,
    )
    .unwrap();
    assert!(NumericStore::open(&destination).is_err());
}

#[test]
fn hierarchy_matches_slow_edge_scan_on_generated_dag() {
    let n = 40;
    let pairs: Vec<_> = (1..n)
        .flat_map(|child| {
            (0..child)
                .filter(move |parent| (child * 19 + parent * 7) % 11 < 2)
                .map(move |parent| (child as u32, parent as u32))
        })
        .collect();
    let store = NumericStore {
        ids: (0..n).map(|i| ROOT + i as u64).collect(),
        modules: vec![0; n],
        effective_times: vec![20260826; n],
        flags: vec![1; n],
        parents: Adjacency::build(n, pairs.clone()).unwrap(),
        children: Adjacency::build(n, pairs.iter().map(|&(a, b)| (b, a)).collect()).unwrap(),
        attributes: Attributes::build(n, vec![]).unwrap(),
        concrete: Attributes::build(n, vec![]).unwrap(),
        concrete_values: vec![],
    };
    store.validate().unwrap();
    for start in 0..n as u32 {
        for ancestors in [false, true] {
            for direct in [false, true] {
                for include_self in [false, true] {
                    let mut expected = BTreeSet::new();
                    let mut pending = VecDeque::from([start]);
                    while let Some(node) = pending.pop_front() {
                        for &(child, parent) in &pairs {
                            let (from, to) = if ancestors {
                                (child, parent)
                            } else {
                                (parent, child)
                            };
                            if from == node && expected.insert(to) && !direct {
                                pending.push_back(to);
                            }
                        }
                    }
                    if include_self {
                        expected.insert(start);
                    }
                    let expected: Vec<_> = expected.into_iter().map(|i| ROOT + i as u64).collect();
                    assert_eq!(
                        store.hierarchy(ROOT + start as u64, ancestors, direct, include_self),
                        expected
                    );
                }
            }
        }
    }
}
