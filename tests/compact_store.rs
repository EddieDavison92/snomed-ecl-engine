#![cfg(feature = "import")]

use snomed_ecl_engine::import::{import_snapshot, ImportOptions, UK_DISPLAY_REFSETS};
use snomed_ecl_engine::store::{
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

#[test]
fn legacy_identifier_columns_resolve_the_same_codes() {
    use std::io::Read;
    let temp = TempDir::new().unwrap();
    let original = temp.path().join("original.zip");
    fixture(&original, false, false);
    let archive = temp.path().join("legacy.zip");
    let mut source = zip::ZipArchive::new(File::open(&original).unwrap()).unwrap();
    let mut output = zip::ZipWriter::new(File::create(&archive).unwrap());
    for i in 0..source.len() {
        let mut file = source.by_index(i).unwrap();
        let mut body = String::new();
        file.read_to_string(&mut body).unwrap();
        if file.name().contains("sct2_Identifier_") {
            body = body
                .lines()
                .map(|line| {
                    let fields: Vec<_> = line.split('\t').collect();
                    [4, 0, 1, 2, 3, 5].map(|p| fields[p]).join("\t") + "\n"
                })
                .collect();
        }
        output
            .start_file(file.name(), SimpleFileOptions::default())
            .unwrap();
        output.write_all(body.as_bytes()).unwrap();
    }
    output.finish().unwrap();
    let destination = temp.path().join("store");
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let store = NumericStore::open(&destination).unwrap();
    let index = store.identifiers.get().unwrap().unwrap();
    assert_eq!(index.lookup(ROOT, "A.1"), Some(LEAF));
    assert_eq!(index.lookup(KIND, "A.1"), Some(ROOT));
    assert_eq!(index.lookup(ROOT, "old"), None);
    assert_eq!(index.lookup(ROOT, "description"), None);
}

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
        900000000000003001,
        900000000000013009,
        900000000000548007,
        900000000000550004,
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
    add("Snapshot/Terminology/sct2_Identifier_Snapshot.txt", format!("alternateIdentifier\teffectiveTime\tactive\tmoduleId\tidentifierSchemeId\treferencedComponentId\nA.1\t20260826\t1\t{ROOT}\t{ROOT}\t{LEAF}\nA.1\t20260826\t1\t{ROOT}\t{KIND}\t{ROOT}\nold\t20260826\t0\t{ROOT}\t{ROOT}\t{LEAF}\ndescription\t20260826\t1\t{ROOT}\t{ROOT}\t6000012\n"));
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
    add("Snapshot/Refset/der2_ssRefset_ModuleDependencySnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tsourceEffectiveTime\ttargetEffectiveTime\n00000000-0000-4000-8000-000000000001\t20260826\t1\t{ROOT}\t900000000000534007\t{LEFT}\t20260826\t20260826\n"));
    add("Snapshot/Refset/der2_cRefset_LanguageSnapshot.txt", "id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tacceptabilityId\nsynthetic-gb\t20260826\t1\t1000001\t900000000000508004\t6000012\t900000000000548007\nsynthetic-realm\t20260826\t1\t1000001\t999001261000000100\t6000013\t900000000000548007\n".into());
    add("Snapshot/Terminology/sct2_Description_Snapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\tconceptId\tlanguageCode\ttypeId\tterm\tcaseSignificanceId\n6000011\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic synonym\t900000000000448009\n6000012\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic GB label\t900000000000448009\n6000013\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic realm label\t900000000000448009\n6000014\t20260826\t1\t{ROOT}\t{ROOT}\ten\t900000000000003001\tSynthetic root (test)\t900000000000448009\n6000015\t20260826\t0\t{ROOT}\t{LEAF}\ten\t900000000000013009\tInactive label\t900000000000448009\n"));
    add("Snapshot/Refset/der2_Refset_SimpleSnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\n00000000-0000-4000-8000-000000000002\t20260826\t1\t{ROOT}\t{ROOT}\t{LEFT}\n00000000-0000-4000-8000-000000000003\t20260826\t1\t{ROOT}\t{ROOT}\t{LEFT}\n00000000-0000-4000-8000-000000000004\t20260826\t1\t{ROOT}\t{ROOT}\t{LEAF}\n00000000-0000-4000-8000-000000000005\t20260826\t1\t{ROOT}\t{ROOT}\t{INACTIVE}\n00000000-0000-4000-8000-000000000006\t20260826\t0\t{ROOT}\t{ROOT}\t{RIGHT}\n"));
    add("Snapshot/Terminology/sct2_TextDefinition_Snapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\tconceptId\tlanguageCode\ttypeId\tterm\tcaseSignificanceId\n6000016\t20260826\t1\t{ROOT}\t{RIGHT}\ten\t900000000000550004\tSynthetic definition\t900000000000448009\n"));
    archive.finish().unwrap();
}

fn options(path: &Path) -> ImportOptions {
    ImportOptions {
        edition: format!("http://snomed.info/sct/{ROOT}/version/20260826"),
        expected_sha256: sha256(path).unwrap(),
        display_refsets: UK_DISPLAY_REFSETS.to_vec(),
    }
}

fn descriptor_fixture(path: &Path, invalid_decimal: bool) {
    use std::io::Read;
    fixture(path, false, false);
    let mut original = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut files = Vec::new();
    for index in 0..original.len() {
        let mut file = original.by_index(index).unwrap();
        let mut body = String::new();
        file.read_to_string(&mut body).unwrap();
        let name = file.name().to_owned();
        if name.contains("sct2_Concept_") {
            for id in [
                800001u64,
                800002,
                1119403002,
                900000000000456007,
                900000000000461009,
                900000000000474003,
                900000000000475002,
            ] {
                body.push_str(&format!("{id}\t20260826\t1\t{ROOT}\t900000000000074008\n"));
            }
        }
        if name.contains("sct2_Relationship_Snapshot") {
            body.push_str(&format!("3999901\t20260826\t1\t{ROOT}\t800001\t800002\t0\t{ISA}\t900000000000011006\t900000000000451002\n"));
        }
        files.push((name, body));
    }
    drop(original);
    let mut descriptors = "id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tattributeDescription\tattributeType\tattributeOrder\n".to_owned();
    for (position, kind) in [
        900000000000461009u64,
        1119403002,
        900000000000475002,
        900000000000474003,
    ]
    .into_iter()
    .enumerate()
    {
        descriptors.push_str(&format!("00000000-0000-4000-8000-00000000100{position}\t20260826\t1\t{ROOT}\t900000000000456007\t800002\t{ROOT}\t{kind}\t{position}\n"));
    }
    files.push((
        "Synthetic/Snapshot/Refset/der2_cciRefset_RefsetDescriptorSnapshot.txt".into(),
        descriptors,
    ));
    let amount = if invalid_decimal {
        "NaN"
    } else {
        "0.100000000000000001"
    };
    files.push(("Synthetic/Snapshot/Refset/der2_sssRefset_CustomSnapshot.txt".into(), format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tcustom Amount\tReview Date\tlinkUuid\n00000000-0000-4000-8000-000000002001\t20260826\t1\t{ROOT}\t800001\t{LEAF}\t{amount}\t20260801\t00000000-0000-4000-8000-000000009001\n00000000-0000-4000-8000-000000002002\t20260826\t1\t{ROOT}\t800001\t{RIGHT}\t0.1\t\t00000000-0000-4000-8000-000000009002\n")));
    let mut archive = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, body) in files {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(body.as_bytes()).unwrap();
    }
    archive.finish().unwrap();
}

#[test]
fn rf2_descriptors_preserve_inherited_decimal_date_and_uuid_types() {
    use snomed_ecl_engine::{
        ecl::parse,
        eval::{evaluate, evaluate_result, QueryResult},
        store::{MemberColumn, MemberValue},
    };
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    descriptor_fixture(&archive, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let store = NumericStore::open(&destination).unwrap();
    for (query, expected) in [
        ("^800001 {{M customAmount>#0.1}}", vec![LEAF]),
        ("^800001 {{M customAmount=#0.1000}}", vec![RIGHT]),
        (r#"^800001 {{M reviewDate="20260801"}}"#, vec![LEAF]),
        (r#"^800001 {{M reviewDate=""}}"#, vec![RIGHT]),
        (
            r#"^800001 {{M reviewDate=("20260801" "")}}"#,
            vec![RIGHT, LEAF],
        ),
    ] {
        assert_eq!(
            evaluate(&store, &parse(query).unwrap())
                .unwrap()
                .iter()
                .map(|&o| store.ids[o as usize])
                .collect::<Vec<_>>(),
            expected,
            "{query}"
        );
    }
    let QueryResult::Rows(rows) = evaluate_result(
        &store,
        &parse("^[customAmount,reviewDate,linkUuid]800001").unwrap(),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        rows[0]["customAmount"],
        MemberValue::Number("0.100000000000000001".into())
    );
    assert_eq!(rows[0]["ReviewDate"], MemberValue::Time("20260801".into()));
    assert_eq!(
        rows[0]["linkUuid"],
        MemberValue::String("00000000-0000-4000-8000-000000009001".into())
    );
    let mut table = store.member_tables.get(800001).unwrap().unwrap().clone();
    let MemberColumn::Number(number) = &mut table.columns[6] else {
        panic!()
    };
    number.text.replace_range(..3, "NaN");
    assert!(table.validate().is_err());
    let bad_archive = temp.path().join("bad.zip");
    let bad_store = temp.path().join("bad-store");
    descriptor_fixture(&bad_archive, true);
    assert!(import_snapshot(&bad_archive, &bad_store, &options(&bad_archive)).is_err());
    assert!(!bad_store.exists());
}

#[test]
fn roundtrip_preserves_groups_precision_and_separate_displays() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let manifest = import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    assert_eq!(manifest.active_concept_count, 14);
    let store = NumericStore::open(&destination).unwrap();
    assert_eq!(store.identifiers.get().unwrap().unwrap().rows.len(), 4);
    assert_eq!(
        store
            .identifiers
            .get()
            .unwrap()
            .unwrap()
            .lookup(ROOT, "A.1"),
        Some(LEAF)
    );
    assert_eq!(
        store
            .identifiers
            .get()
            .unwrap()
            .unwrap()
            .lookup(ROOT, "old"),
        None
    );
    assert_eq!(
        store
            .identifiers
            .get()
            .unwrap()
            .unwrap()
            .lookup(ROOT, "description"),
        None
    );
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
        descriptions: Default::default(),
        member_tables: Default::default(),
        identifiers: Default::default(),
        config: Default::default(),
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
    let binary = env!("CARGO_BIN_EXE_snomed-ecl-engine");
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
        writeln!(
            input,
            "{{\"ecl\":\"^[referencedComponentId,sourceEffectiveTime]900000000000534007\"}}"
        )
        .unwrap();
        writeln!(
            input,
            "{{\"ecl\":\"^[sourceEffectiveTime]900000000000534007\",\"count_only\":true}}"
        )
        .unwrap();
        writeln!(
            input,
            "{{\"ecl\":\"^[sourceEffectiveTime]900000000000534007\"}}"
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
    assert_eq!(rows.len(), 6);
    assert!(rows[0]["error"]
        .as_str()
        .unwrap()
        .starts_with("InvalidField"));
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
    assert_eq!(rows[3]["result_type"], "rows");
    assert_eq!(
        rows[3]["rows"][0]["sourceEffectiveTime"],
        serde_json::json!({"type":"time","value":"20260826"})
    );
    assert_eq!(
        rows[3]["rows"][0]["referencedComponentId"],
        serde_json::json!({"type":"concept","value":LEFT.to_string()})
    );
    assert!(rows[3].get("codes").is_none());
    assert_eq!(rows[4]["total"], 1);
    assert_eq!(rows[4]["result_type"], "values");
    assert!(rows[4].get("rows").is_none());
    assert!(rows[4].get("values").is_none());
    assert_eq!(rows[5]["result_type"], "values");
    assert_eq!(
        rows[5]["values"],
        serde_json::json!([{"type":"time", "value":"20260826"}])
    );
    assert!(rows[5].get("codes").is_none());
    let invalid_display = Command::new(binary)
        .arg("expand")
        .arg(&destination)
        .arg("^[sourceEffectiveTime]900000000000534007")
        .arg("--display")
        .output()
        .unwrap();
    assert!(!invalid_display.status.success());
    assert!(invalid_display.stdout.is_empty());
}

#[test]
fn typed_members_load_lazily_and_reject_corruption_even_with_a_forged_hash() {
    use snomed_ecl_engine::{
        ecl::parse,
        eval::{evaluate, EvalError},
    };
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let mut manifest = import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let path = destination.join("members").join(format!("{ROOT}.bin"));
    let original = fs::read(&path).unwrap();
    let query = parse(&format!("^{ROOT} {{{{M active=0}}}}")).unwrap();
    let loaded = NumericStore::open(&destination).unwrap();
    assert_eq!(
        evaluate(&loaded, &query).unwrap(),
        [loaded.ordinal(RIGHT).unwrap()]
    );
    fs::remove_file(&path).unwrap();
    let loaded = NumericStore::open(&destination).unwrap();
    assert!(evaluate(&loaded, &parse(&format!("<<{ROOT}")).unwrap()).is_ok());
    assert!(matches!(
        evaluate(&loaded, &query),
        Err(EvalError::Index(_))
    ));
    let mut corrupt = original.clone();
    corrupt[0] ^= 1;
    fs::write(&path, &corrupt).unwrap();
    let loaded = NumericStore::open(&destination).unwrap();
    assert!(matches!(
        evaluate(&loaded, &query),
        Err(EvalError::Index(_))
    ));
    // Duplicate the first UUID into the second row, while supplying a matching file hash.
    let mut corrupt = original;
    let schema_bytes = u64::from_le_bytes(corrupt[16..24].try_into().unwrap()) as usize;
    let first_uuid = 24 + schema_bytes + 4 + 8;
    corrupt.copy_within(first_uuid..first_uuid + 16, first_uuid + 16);
    fs::write(&path, &corrupt).unwrap();
    manifest
        .member_tables
        .as_mut()
        .unwrap()
        .iter_mut()
        .find(|m| m.refset == ROOT)
        .unwrap()
        .sha256 = sha256(&path).unwrap();
    fs::write(
        destination.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let loaded = NumericStore::open(&destination).unwrap();
    assert!(matches!(
        evaluate(&loaded, &query),
        Err(EvalError::Index(_))
    ));
}

#[test]
fn cli_import_progress_and_presentation_keep_machine_output_parseable() {
    use std::process::Command;
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let binary = env!("CARGO_BIN_EXE_snomed-ecl-engine");
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
    assert_eq!(manifest["manifest"]["active_concept_count"], 14);
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
    use snomed_ecl_engine::{
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
    let mut rows = format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\n00000000-0000-4000-8000-000000000007\t20260820\t1\t2000002\t2000001\t{member}\n00000000-0000-4000-8000-000000000008\t20260820\t1\t2000002\t2000001\t{INACTIVE}\n00000000-0000-4000-8000-000000000009\t20260820\t0\t2000002\t2000001\t{RIGHT}\n");
    if duplicate {
        rows.push_str(&format!(
            "00000000-0000-4000-8000-000000000007\t20260820\t1\t2000002\t2000001\t{member}\n"
        ));
    }
    add("Snapshot/Refset/der2_Refset_SimpleSnapshot.txt", rows);
    add("Snapshot/Refset/der2_ssRefset_ModuleDependencySnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tsourceEffectiveTime\ttargetEffectiveTime\n00000000-0000-4000-8000-000000000010\t20260820\t1\t2000002\t900000000000534007\t{ROOT}\t20260820\t20260826\n"));
    archive.finish().unwrap();
}

#[test]
fn supplementary_refsets_preserve_base_semantics_and_provenance() {
    use snomed_ecl_engine::{ecl::parse, eval::evaluate, import::add_refsets_snapshot};
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
    assert_eq!(
        store
            .identifiers
            .get()
            .unwrap()
            .unwrap()
            .lookup(ROOT, "A.1"),
        Some(LEAF)
    );
    let codes = |query: &str| {
        evaluate(&store, &parse(query).unwrap())
            .unwrap()
            .iter()
            .map(|&i| store.ids[i as usize])
            .collect::<Vec<_>>()
    };
    assert_eq!(codes("^2000001"), [LEAF, INACTIVE]);
    assert_eq!(codes("^2000001 {{M active=0}}"), [RIGHT]);
    assert_eq!(codes("^900000000000534007 {{M active=1}}"), [ROOT, LEFT]);
    assert_eq!(
        codes("^900000000000534007 {{M sourceEffectiveTime=\"20260820\"}}"),
        [ROOT]
    );
    assert_eq!(codes("2000001 {{D type=fsn}}"), [2000001]);
    assert_eq!(
        codes(&format!("{LEAF} {{{{D dialect=en-gb (prefer)}}}}")),
        [LEAF]
    );
    assert_eq!(codes("* {{D type=def}}"), [RIGHT]);
    assert_eq!(store.descriptions.get().unwrap().unwrap().len(), 7);
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
fn identifier_cli_configuration_and_lazy_integrity() {
    use snomed_ecl_engine::{
        config::QueryConfig,
        ecl::parse,
        eval::{evaluate, EvalError},
    };
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let mut manifest = import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let config = temp.path().join("aliases.json");
    fs::write(
        &config,
        format!(r#"{{"identifier_schemes":{{"demo":"{ROOT}"}}}}"#),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .arg("expand")
        .arg(&destination)
        .arg("demo#A.1")
        .arg("--config")
        .arg(&config)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        LEAF.to_string()
    );
    let path = destination.join("identifiers.json");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, b"invalid").unwrap();
    let mut store = NumericStore::open(&destination).unwrap();
    store.config = QueryConfig::read(&config).unwrap();
    assert_eq!(
        evaluate(&store, &parse(&ROOT.to_string()).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        evaluate(&store, &parse("demo#A.1").unwrap()),
        Err(EvalError::Index(_))
    ));
    let mut index: snomed_ecl_engine::store::IdentifierIndex =
        serde_json::from_slice(&bytes).unwrap();
    index.rows.push(index.rows[0].clone());
    fs::write(&path, serde_json::to_vec(&index).unwrap()).unwrap();
    let metadata = manifest.identifiers.as_mut().unwrap();
    metadata.bytes = fs::metadata(&path).unwrap().len();
    metadata.sha256 = sha256(&path).unwrap();
    metadata.rows = index.rows.len();
    serde_json::to_writer(
        File::create(destination.join("manifest.json")).unwrap(),
        &manifest,
    )
    .unwrap();
    assert!(NumericStore::open(&destination)
        .unwrap()
        .identifiers
        .get()
        .is_err());
}

#[test]
fn supplementary_refsets_reject_invalid_snapshots_before_publishing() {
    use snomed_ecl_engine::import::add_refsets_snapshot;
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

#[test]
fn descriptions_load_lazily_preserve_definitions_and_reject_corruption() {
    use snomed_ecl_engine::{
        ecl::parse,
        eval::{evaluate, EvalError},
    };
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    let mut manifest = import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let metadata = manifest.descriptions.as_ref().unwrap();
    assert_eq!(
        (
            metadata.descriptions,
            metadata.active_descriptions,
            metadata.language_memberships
        ),
        (6, 5, 2)
    );
    let path = destination.join("descriptions.bin");
    let saved = destination.join("descriptions.saved");
    fs::rename(&path, &saved).unwrap();
    let store = NumericStore::open(&destination).unwrap();
    assert_eq!(
        evaluate(&store, &parse(&ROOT.to_string()).unwrap())
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        evaluate(&store, &parse("* {{D type=def}}").unwrap()),
        Err(EvalError::Index(_))
    ));
    fs::rename(&saved, &path).unwrap();
    let store = NumericStore::open(&destination).unwrap();
    assert_eq!(
        evaluate(&store, &parse("* {{D type=def}}").unwrap()).unwrap(),
        [store.ordinal(RIGHT).unwrap()]
    );
    let descriptions = store.descriptions.get().unwrap().unwrap();
    assert_eq!(descriptions.len(), 6);
    assert_eq!(
        descriptions.term(
            descriptions
                .for_concept(store.ordinal(RIGHT).unwrap())
                .start
        ),
        "Synthetic definition"
    );
    assert_eq!(
        descriptions.for_concept(store.ordinal(LEAF).unwrap()).len(),
        4
    );
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() = 255;
    fs::write(&path, &bytes).unwrap();
    assert!(NumericStore::open(&destination)
        .unwrap()
        .descriptions
        .get()
        .is_err());
    manifest.descriptions.as_mut().unwrap().sha256 = sha256(&path).unwrap();
    serde_json::to_writer(
        File::create(destination.join("manifest.json")).unwrap(),
        &manifest,
    )
    .unwrap();
    assert!(NumericStore::open(&destination)
        .unwrap()
        .descriptions
        .get()
        .is_err());
}
