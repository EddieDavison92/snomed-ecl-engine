#![cfg(feature = "import")]

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
    for code in [
        ROOT,
        LEFT,
        RIGHT,
        LEAF,
        INACTIVE,
        KIND,
        TARGET,
        ISA,
        900000000000508004,
        999001261000000100,
        900000000000534007,
    ] {
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
    add("Snapshot/Refset/der2_cRefset_LanguageSnapshot.txt", "id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tacceptabilityId\nsynthetic-gb\t20260826\t1\t1000001\t900000000000508004\t6000012\t900000000000548007\nsynthetic-realm\t20260826\t1\t1000001\t999001261000000100\t6000013\t900000000000548007\n".into());
    add("Snapshot/Terminology/sct2_Description_Snapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\tconceptId\tlanguageCode\ttypeId\tterm\tcaseSignificanceId\n6000011\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic synonym\t900000000000448009\n6000012\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic GB label\t900000000000448009\n6000013\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic realm label\t900000000000448009\n6000014\t20260826\t1\t{ROOT}\t{ROOT}\ten\t900000000000003001\tSynthetic root (test)\t900000000000448009\n6000015\t20260826\t0\t{ROOT}\t{LEAF}\ten\t900000000000013009\tInactive label\t900000000000448009\n"));
    add("Snapshot/Refset/der2_Refset_SimpleSnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\nmember-a\t20260826\t1\t{ROOT}\t{ROOT}\t{LEFT}\nmember-b\t20260826\t1\t{ROOT}\t{ROOT}\t{LEFT}\nmember-c\t20260826\t1\t{ROOT}\t{ROOT}\t{LEAF}\nmember-d\t20260826\t1\t{ROOT}\t{ROOT}\t{INACTIVE}\nmember-e\t20260826\t0\t{ROOT}\t{ROOT}\t{RIGHT}\n"));
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
    assert_eq!(manifest.active_concept_count, 10);
    let store = NumericStore::open(&destination).unwrap();
    assert_eq!(
        store.hierarchy(ROOT, false, false, false),
        [LEFT, RIGHT, LEAF]
    );
    assert_eq!(
        store.hierarchy(LEAF, true, false, true),
        [ROOT, LEFT, RIGHT, LEAF]
    );
    assert_eq!(store.hierarchy(INACTIVE, false, false, true), [INACTIVE]);
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
        membership: None,
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

#[test]
fn cli_parses_before_output_and_batch_recovers_after_query_errors() {
    use std::process::{Command, Stdio};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let binary = env!("CARGO_BIN_EXE_snomed-rust-ecl-engine");
    let invalid = Command::new(binary)
        .arg("expand")
        .arg(&destination)
        .arg("* OR (^ [targetComponentId] 1000001)")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    let display = Command::new(binary)
        .arg("expand")
        .arg(&destination)
        .arg(ROOT.to_string())
        .arg("--display")
        .output()
        .unwrap();
    assert!(display.status.success());
    let row: serde_json::Value = serde_json::from_slice(&display.stdout).unwrap();
    assert_eq!(row["code"], ROOT.to_string());
    assert!(row["display"].is_string());
    fs::rename(
        destination.join("display.bin"),
        destination.join("display.saved"),
    )
    .unwrap();
    let mut process = Command::new(binary)
        .arg("batch")
        .arg(&destination)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = process.stdin.take().unwrap();
        writeln!(
            input,
            "{{\"ecl\":\"* OR (^ [targetComponentId] 1000001)\"}}"
        )
        .unwrap();
        writeln!(input, "{{\"ecl\":\"<< 1000001\"}}").unwrap();
        writeln!(
            input,
            "{{\"ecl\":\"1000001 MINUS 1000001\",\"count_only\":true}}"
        )
        .unwrap();
    }
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success());
    let rows: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["error"], "Unsupported");
    assert!(rows[0].get("codes").is_none());
    assert_eq!(
        rows[1]["codes"],
        serde_json::json!([
            ROOT.to_string(),
            LEFT.to_string(),
            RIGHT.to_string(),
            LEAF.to_string()
        ])
    );
    assert_eq!(rows[2]["total"], 0);
    assert!(rows[2].get("codes").is_none());
}

#[test]
fn cli_import_progress_and_presentation_keep_machine_output_parseable() {
    use std::process::Command;
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let binary = env!("CARGO_BIN_EXE_snomed-rust-ecl-engine");
    let config = options(&archive);
    let imported = Command::new(binary)
        .arg("import")
        .arg(&archive)
        .arg(&destination)
        .args([&config.edition, &config.expected_sha256, "--json"])
        .output()
        .unwrap();
    assert!(imported.status.success(), "{:?}", imported);
    let manifest: serde_json::Value = serde_json::from_slice(&imported.stdout).unwrap();
    assert_eq!(manifest["manifest"]["active_concept_count"], 10);
    assert!(!imported.stderr.is_empty());
    assert!(!imported.stdout.contains(&0x1b));
    for options in [vec!["--count", "--json"], vec!["--json"], vec!["--plain"]] {
        let output = Command::new(binary)
            .arg("expand")
            .arg(&destination)
            .arg(ROOT.to_string())
            .args(&options)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.contains(&0x1b));
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if options.contains(&"--count") {
            assert_eq!(value, serde_json::json!({"total": 1}));
        } else if options.contains(&"--json") {
            assert_eq!(value, serde_json::json!({"code": ROOT.to_string()}));
        } else {
            assert_eq!(value, ROOT);
        }
    }
    let conflict = Command::new(binary)
        .args(["--json", "--plain", "--help"])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    assert!(conflict.stdout.is_empty());
}

#[test]
fn membership_import_distinguishes_component_types_and_old_stores() {
    use snomed_rust_ecl_engine::{
        ecl::parse,
        eval::{evaluate, EvalError},
    };
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let mut manifest = import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let metadata = manifest.membership.as_ref().unwrap();
    assert_eq!(metadata.active_non_concept_rows, 2);
    assert_eq!(metadata.snapshot_files, 3);
    let store = NumericStore::open(&destination).unwrap();
    let result = evaluate(&store, &parse(&format!("^{ROOT}")).unwrap()).unwrap();
    assert_eq!(
        result
            .iter()
            .map(|&i| store.ids[i as usize])
            .collect::<Vec<_>>(),
        [LEFT, LEAF, INACTIVE]
    );
    assert!(evaluate(&store, &parse("^900000000000508004").unwrap())
        .unwrap()
        .is_empty());
    fs::rename(
        destination.join("membership.bin"),
        destination.join("membership.saved"),
    )
    .unwrap();
    assert!(NumericStore::open(&destination).is_err());
    manifest.membership = None;
    manifest
        .capabilities
        .retain(|v| v != "concept-refset-membership");
    serde_json::to_writer(
        File::create(destination.join("manifest.json")).unwrap(),
        &manifest,
    )
    .unwrap();
    let old = NumericStore::open(&destination).unwrap();
    assert!(old.membership.is_none());
    assert!(matches!(
        evaluate(&old, &parse("^1000001").unwrap()),
        Err(EvalError::Unsupported(_))
    ));
    assert_eq!(evaluate(&old, &parse("1000001").unwrap()).unwrap().len(), 1);
}

#[test]
fn membership_reader_rejects_corruption_and_invalid_ordinals() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let mut manifest = import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let path = destination.join("membership.bin");
    let mut bytes = fs::read(&path).unwrap();
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(&path, bytes).unwrap();
    assert!(NumericStore::open(&destination).is_err());
    manifest.membership.as_mut().unwrap().sha256 = sha256(&path).unwrap();
    serde_json::to_writer(
        File::create(destination.join("manifest.json")).unwrap(),
        &manifest,
    )
    .unwrap();
    assert!(NumericStore::open(&destination).is_err());
}

fn supplement_fixture(path: &Path, member: u64, duplicate: bool) {
    let mut archive = zip::ZipWriter::new(File::create(path).unwrap());
    let mut add = |name: &str, content: String| {
        archive
            .start_file(format!("extension/{name}"), SimpleFileOptions::default())
            .unwrap();
        archive.write_all(content.as_bytes()).unwrap();
    };
    add("Snapshot/Terminology/sct2_Concept_Snapshot.txt", "id\teffectiveTime\tactive\tmoduleId\tdefinitionStatusId\n2000001\t20260820\t1\t2000002\t900000000000074008\n2000002\t20260820\t1\t2000002\t900000000000074008\n".into());
    add("Snapshot/Terminology/sct2_Relationship_Snapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\tsourceId\tdestinationId\trelationshipGroup\ttypeId\tcharacteristicTypeId\tmodifierId\n7000002\t20260820\t1\t2000002\t2000001\t{ROOT}\t0\t{ISA}\t900000000000011006\t900000000000451002\n"));
    add("Snapshot/Terminology/sct2_Description_Snapshot.txt", "id\teffectiveTime\tactive\tmoduleId\tconceptId\tlanguageCode\ttypeId\tterm\tcaseSignificanceId\n7000011\t20260820\t1\t2000002\t2000001\ten\t900000000000003001\tSynthetic extra refset (foundation metadata concept)\t900000000000448009\n".into());
    let mut rows = format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\nextra-a\t20260820\t1\t2000002\t2000001\t{member}\nextra-b\t20260820\t1\t2000002\t2000001\t{INACTIVE}\nextra-c\t20260820\t0\t2000002\t2000001\t{RIGHT}\n");
    if duplicate {
        rows.push_str(&format!(
            "extra-a\t20260820\t1\t2000002\t2000001\t{member}\n"
        ));
    }
    add("Snapshot/Refset/der2_Refset_SimpleSnapshot.txt", rows);
    archive.finish().unwrap();
}

#[test]
fn supplementary_refsets_preserve_base_semantics_and_provenance() {
    use snomed_rust_ecl_engine::{ecl::parse, eval::evaluate, import::add_refsets_snapshot};
    let temp = TempDir::new().unwrap();
    let base_archive = temp.path().join("base.zip");
    let base = temp.path().join("base");
    fixture(&base_archive, false, false);
    let original = import_snapshot(&base_archive, &base, &options(&base_archive)).unwrap();
    let extra = temp.path().join("extra.zip");
    supplement_fixture(&extra, LEAF, false);
    let output = temp.path().join("combined");
    let hash = sha256(&extra).unwrap();
    let combined = add_refsets_snapshot(&base, &extra, &output, "20260820", &hash).unwrap();
    assert_eq!(combined.edition, original.edition);
    assert_eq!(combined.archive_sha256, original.archive_sha256);
    assert_eq!(combined.supplements[0].archive_sha256, hash);
    assert_eq!(
        combined.supplements[0].base_core_sha256,
        original.core_sha256
    );
    assert_eq!(combined.supplements[0].added_concepts, 2);
    assert_eq!(combined.concept_count, original.concept_count + 2);
    assert_eq!(
        sha256(&base.join("core.bin")).unwrap(),
        original.core_sha256
    );
    let store = NumericStore::open(&output).unwrap();
    let codes = |query: &str| {
        evaluate(&store, &parse(query).unwrap())
            .unwrap()
            .iter()
            .map(|&i| store.ids[i as usize])
            .collect::<Vec<_>>()
    };
    assert_eq!(codes("^2000001"), [LEAF, INACTIVE]);
    assert_eq!(codes(&format!("(^2000001) AND (<<{ROOT})")), [LEAF]);
    assert!(codes(&format!("^{ROOT}")).contains(&INACTIVE));
    assert!(codes(&format!("^R{LEAF}")).contains(&2000001));
    assert_eq!(codes(&format!("{LEAF} : {KIND} = {TARGET}")), [LEAF]);
    assert_eq!(
        codes(&format!("{LEAF} : {KIND} = #0.100000000000000001")),
        [LEAF]
    );
    let mut displays = DisplayStore::open(&output).unwrap();
    assert_eq!(
        displays
            .get(store.ordinal(LEAF).unwrap())
            .unwrap()
            .as_deref(),
        Some("Synthetic realm label")
    );
    assert_eq!(
        displays
            .get(store.ordinal(2000001).unwrap())
            .unwrap()
            .as_deref(),
        Some("Synthetic extra refset (foundation metadata concept)")
    );
    assert!(add_refsets_snapshot(
        &output,
        &extra,
        &temp.path().join("duplicate"),
        "20260820",
        &hash
    )
    .is_err());
    assert!(add_refsets_snapshot(&base, &extra, &output, "20260820", &hash).is_err());
}

#[test]
fn supplementary_refsets_reject_invalid_snapshots_before_publishing() {
    use snomed_rust_ecl_engine::import::add_refsets_snapshot;
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("base.zip");
    let base = temp.path().join("base");
    fixture(&archive, false, false);
    import_snapshot(&archive, &base, &options(&archive)).unwrap();
    for (name, member, duplicate, date, bad_hash) in [
        ("missing", 7770001, false, "20260820", false),
        ("description", 7770011, false, "20260820", false),
        ("duplicate", LEAF, true, "20260820", false),
        ("date", LEAF, false, "20260819", false),
        ("checksum", LEAF, false, "20260820", true),
    ] {
        let extra = temp.path().join(format!("{name}.zip"));
        let output = temp.path().join(name);
        supplement_fixture(&extra, member, duplicate);
        let hash = if bad_hash {
            "0".repeat(64)
        } else {
            sha256(&extra).unwrap()
        };
        assert!(
            add_refsets_snapshot(&base, &extra, &output, date, &hash).is_err(),
            "{name}"
        );
        assert!(!output.exists(), "{name}");
    }
}
