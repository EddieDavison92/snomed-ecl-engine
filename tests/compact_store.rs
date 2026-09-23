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
/// A reference set whose only Snapshot row is an inactive description member.
const INACTIVE_DESCRIPTION_REFSET: u64 = 9000003;
/// A reference set with no rows whose descriptor declares description members.
const DECLARED_DESCRIPTION_REFSET: u64 = 9000004;
/// A reference set whose only Snapshot row is an inactive concept member.
const INACTIVE_CONCEPT_REFSET: u64 = 9000005;
const DESCRIPTOR_REFSET: u64 = 900000000000456007;
const DESCRIPTION_TYPE: u64 = 900000000000462002;
const ISA: u64 = 116680003;

#[test]
fn stored_terms_preserve_long_unicode_text_concurrency_and_failed_reads() {
    use snomed_ecl_engine::store::{pack, Description, DescriptionIndex};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let directory = temp.path().join("store");
    fixture(&archive, false, false);
    let mut manifest = import_snapshot(&archive, &directory, &options(&archive)).unwrap();
    let core = NumericStore::open(&directory).unwrap();
    let leaf = core.ordinal(LEAF).unwrap();
    let texts: Vec<_> = [1, 65530, 12, 131090, 70, 1, 65535]
        .into_iter()
        .map(|n| format!("{}é🦀", "a".repeat(n)))
        .collect();
    let descriptions = texts
        .iter()
        .enumerate()
        .map(|(row, term)| Description {
            id: 7000011 + row as u64 * 100,
            concept: leaf,
            module: core.ordinal(ROOT).unwrap(),
            kind: core.ordinal(900000000000013009).unwrap(),
            effective_time: 20260826,
            active: row % 2 == 0,
            language: *b"en",
            term: term.clone(),
            dialects: vec![],
        })
        .collect();
    let built = DescriptionIndex::build(core.ids.len(), descriptions).unwrap();
    let path = directory.join("descriptions.bin");
    fs::remove_file(&path).unwrap();
    manifest.descriptions = Some(built.write(&path).unwrap());
    serde_json::to_writer(
        File::create(directory.join("manifest.json")).unwrap(),
        &manifest,
    )
    .unwrap();
    let original = fs::read(&path).unwrap();
    let packed = temp.path().join("packed.ecl");
    pack(&directory, &packed).unwrap();
    for source in [&directory, &packed] {
        let store = NumericStore::open(source).unwrap();
        let index = store.descriptions.get().unwrap().unwrap();
        let rewritten = temp.path().join(if source == &directory {
            "directory.bin"
        } else {
            "packed.bin"
        });
        index.write(&rewritten).unwrap();
        assert_eq!(fs::read(rewritten).unwrap(), original);
        std::thread::scope(|scope| {
            for shift in 0..8 {
                let texts = &texts;
                scope.spawn(move || {
                    for step in 0..80 {
                        let row = (step * 3 + shift) % texts.len();
                        index
                            .with_term(row, |text| assert_eq!(text, texts[row]))
                            .unwrap();
                    }
                });
            }
        });
    }
    let store = NumericStore::open(&directory).unwrap();
    let index = store.descriptions.get().unwrap().unwrap();
    assert_eq!(index.term(0).unwrap(), texts[0]);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(8)
        .unwrap();
    assert!(index.term(texts.len() - 1).is_err());
    assert!(index.term(0).is_err());
    fs::write(&path, &original).unwrap();
    assert_eq!(index.term(0).unwrap(), texts[0]);
}

#[test]
fn packed_store_preserves_lazy_sections_queries_displays_and_supplements() {
    use snomed_ecl_engine::{
        ecl::parse,
        eval::evaluate_result,
        store::{pack, verify},
    };
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let directory = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &directory, &options(&archive)).unwrap();
    let packed = temp.path().join("edition.ecl");
    pack(&directory, &packed).unwrap();
    let original_hash = sha256(&packed).unwrap();
    assert!(pack(&directory, &packed).is_err());
    assert_eq!(sha256(&packed).unwrap(), original_hash);
    let source = NumericStore::open(&directory).unwrap();
    let loaded = NumericStore::open(&packed).unwrap();
    for query in [
        "*",
        "<< 1000001",
        "* : {9000001=9000002}",
        "1000004 . 9000001",
        "* {{D active=\"*\"}}",
        "^[referencedComponentId]1000001",
        "* {{C active=0}}",
    ] {
        let ast = parse(query).unwrap();
        assert_eq!(
            evaluate_result(&loaded, &ast).unwrap(),
            evaluate_result(&source, &ast).unwrap(),
            "{query}"
        );
    }
    let source_ids = source.identifiers.get().unwrap().unwrap();
    let packed_ids = loaded.identifiers.get().unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(source_ids).unwrap(),
        serde_json::to_value(packed_ids).unwrap()
    );
    let original_display = DisplayStore::open(&directory).unwrap();
    let packed_display = DisplayStore::open(&packed).unwrap();
    for ordinal in 0..source.ids.len() as u32 {
        assert_eq!(
            original_display.get(ordinal).unwrap(),
            packed_display.get(ordinal).unwrap()
        );
    }
    let before = verify(&directory).unwrap();
    let after = verify(&packed).unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    let repacked = temp.path().join("again.ecl");
    pack(&packed, &repacked).unwrap();
    assert_eq!(sha256(&repacked).unwrap(), original_hash);
    let supplement = temp.path().join("extra.zip");
    supplement_fixture(&supplement, LEAF, false);
    let added = temp.path().join("added");
    snomed_ecl_engine::import::add_refsets_snapshot(
        &packed,
        &supplement,
        &added,
        "20260820",
        &sha256(&supplement).unwrap(),
    )
    .unwrap();
    verify(&added).unwrap();
}

#[test]
fn packed_store_rejects_corrupt_tables_offsets_and_lazy_payloads() {
    use sha2::{Digest, Sha256};
    use snomed_ecl_engine::store::{pack, verify};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let directory = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &directory, &options(&archive)).unwrap();
    let packed = temp.path().join("edition.ecl");
    pack(&directory, &packed).unwrap();
    let original = fs::read(&packed).unwrap();
    let table_len = u64::from_le_bytes(original[16..24].try_into().unwrap()) as usize;
    let table: serde_json::Value = serde_json::from_slice(&original[56..56 + table_len]).unwrap();
    let bad = temp.path().join("bad.ecl");
    for mutate in 0..7 {
        let mut copy = original.clone();
        let mut metadata = table.clone();
        match mutate {
            0 => metadata["sections"][0]["offset"] = serde_json::json!(0),
            1 => metadata["sections"][0]["length"] = serde_json::json!(u64::MAX),
            2 => metadata["sections"][0]["name"] = serde_json::json!("../core.bin"),
            3 => metadata["sections"][1]["offset"] = metadata["sections"][0]["offset"].clone(),
            4 => {
                metadata["sections"].as_array_mut().unwrap().pop();
            }
            5 => metadata["manifest"]["format"] = serde_json::json!(99),
            _ => metadata["sections"][0]["codec"] = serde_json::json!(99),
        }
        let encoded = serde_json::to_vec(&metadata).unwrap();
        assert!(56 + encoded.len() < table["sections"][0]["offset"].as_u64().unwrap() as usize);
        copy[16..24].copy_from_slice(&(encoded.len() as u64).to_le_bytes());
        copy[24..56].copy_from_slice(&Sha256::digest(&encoded));
        copy[56..56 + encoded.len()].copy_from_slice(&encoded);
        fs::write(&bad, &copy).unwrap();
        assert!(Manifest::read(&bad).is_err(), "mutation {mutate}");
    }
    fs::write(&bad, &original[..original.len() - 1]).unwrap();
    assert!(NumericStore::open(&bad).is_err());
    let mut copy = original.clone();
    copy[56] ^= 1;
    fs::write(&bad, &copy).unwrap();
    assert!(Manifest::read(&bad).is_err());
    let section = table["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "descriptions.bin")
        .unwrap();
    let offset = section["offset"].as_u64().unwrap() as usize;
    let mut copy = original;
    copy[offset + 8] ^= 1;
    fs::write(&bad, &copy).unwrap();
    let lazy = NumericStore::open(&bad).unwrap();
    assert!(lazy.descriptions.get().is_err());
    assert!(verify(&bad).is_err());
    let failed = temp.path().join("failed.ecl");
    assert!(pack(&bad, &failed).is_err());
    assert!(!failed.exists());
    assert!(!fs::read_dir(temp.path()).unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("partial-")));
}

#[test]
fn opening_skips_the_core_checksum_that_verify_still_catches() {
    // Opening stopped hashing sections so a query does not read the core twice.
    // A flipped byte that keeps every length and offset valid is therefore
    // invisible to `open`, and `verify` is the only thing that catches it.
    use snomed_ecl_engine::store::{pack_with_options, verify, PackOptions};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let directory = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &directory, &options(&archive)).unwrap();
    let packed = temp.path().join("edition.ecl");
    // Uncompressed, so a byte in the file is the same byte in the section.
    let raw = PackOptions {
        compress: false,
        ..PackOptions::default()
    };
    pack_with_options(&directory, &packed, raw).unwrap();

    let original = fs::read(&packed).unwrap();
    let table_len = u64::from_le_bytes(original[16..24].try_into().unwrap()) as usize;
    let table: serde_json::Value = serde_json::from_slice(&original[56..56 + table_len]).unwrap();
    let core = table["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "core.bin")
        .expect("core section");
    assert_eq!(core["codec"], 0, "test needs an uncompressed section");
    let offset = core["offset"].as_u64().unwrap() as usize;
    let concepts = Manifest::read(&packed).unwrap().concept_count;

    // The module column: magic, count, ids, then its own count. Nothing bounds
    // checks or semantically validates a module id, so only the hash sees this.
    let modules = offset + 8 + 8 + concepts * 8 + 8;
    let mut copy = original;
    copy[modules] ^= 1;
    let bad = temp.path().join("bad.ecl");
    fs::write(&bad, &copy).unwrap();

    let store = NumericStore::open(&bad).expect("open does not hash the core");
    store.validate().expect("the flip breaks no semantic invariant");
    assert_eq!(store.ids.len(), concepts);

    let error = verify(&bad).expect_err("verify hashes every section").to_string();
    assert!(error.contains("checksum"), "unexpected error: {error}");
}

#[test]
fn packed_section_readers_do_not_share_seek_positions() {
    use snomed_ecl_engine::store::pack;
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let directory = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &directory, &options(&archive)).unwrap();
    let packed = temp.path().join("edition.ecl");
    pack(&directory, &packed).unwrap();
    let store = NumericStore::open(&packed).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let (path, store) = (&packed, &store);
            scope.spawn(move || {
                let display = DisplayStore::open(path).unwrap();
                for _ in 0..20 {
                    assert_eq!(
                        display
                            .get(store.ordinal(LEAF).unwrap())
                            .unwrap()
                            .as_deref(),
                        Some("Synthetic realm label")
                    );
                    assert_eq!(
                        store
                            .identifiers
                            .get()
                            .unwrap()
                            .unwrap()
                            .lookup(ROOT, "A.1"),
                        Some(LEAF)
                    );
                    assert!(!store.descriptions.get().unwrap().unwrap().is_empty());
                    for refset in store.member_tables.refsets() {
                        assert!(store.member_tables.get(refset).unwrap().is_some());
                    }
                }
            });
        }
    });
}

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
        INACTIVE_DESCRIPTION_REFSET,
        DECLARED_DESCRIPTION_REFSET,
        INACTIVE_CONCEPT_REFSET,
        DESCRIPTOR_REFSET,
        DESCRIPTION_TYPE,
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
    // The descriptor declares one reference set as description-based without any member rows.
    add("Snapshot/Refset/der2_cciRefset_RefsetDescriptorSnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tattributeDescription\tattributeType\tattributeOrder\n00000000-0000-4000-8000-000000000101\t20260826\t1\t{ROOT}\t{DESCRIPTOR_REFSET}\t{DECLARED_DESCRIPTION_REFSET}\t{ROOT}\t{DESCRIPTION_TYPE}\t0\n"));
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
    add("Snapshot/Refset/der2_cRefset_LanguageSnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tacceptabilityId\nsynthetic-gb\t20260826\t1\t1000001\t900000000000508004\t6000012\t900000000000548007\nsynthetic-realm\t20260826\t1\t1000001\t999001261000000100\t6000013\t900000000000548007\nsynthetic-retired\t20260826\t0\t1000001\t{INACTIVE_DESCRIPTION_REFSET}\t6000015\t900000000000548007\n"));
    add("Snapshot/Terminology/sct2_Description_Snapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\tconceptId\tlanguageCode\ttypeId\tterm\tcaseSignificanceId\n6000011\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic synonym\t900000000000448009\n6000012\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic GB label\t900000000000448009\n6000013\t20260826\t1\t{ROOT}\t{LEAF}\ten\t900000000000013009\tSynthetic realm label\t900000000000448009\n6000014\t20260826\t1\t{ROOT}\t{ROOT}\ten\t900000000000003001\tSynthetic root (test)\t900000000000448009\n6000015\t20260826\t0\t{ROOT}\t{LEAF}\ten\t900000000000013009\tInactive label\t900000000000448009\n"));
    add("Snapshot/Refset/der2_Refset_SimpleSnapshot.txt", format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\n00000000-0000-4000-8000-000000000002\t20260826\t1\t{ROOT}\t{ROOT}\t{LEFT}\n00000000-0000-4000-8000-000000000003\t20260826\t1\t{ROOT}\t{ROOT}\t{LEFT}\n00000000-0000-4000-8000-000000000004\t20260826\t1\t{ROOT}\t{ROOT}\t{LEAF}\n00000000-0000-4000-8000-000000000005\t20260826\t1\t{ROOT}\t{ROOT}\t{INACTIVE}\n00000000-0000-4000-8000-000000000006\t20260826\t0\t{ROOT}\t{ROOT}\t{RIGHT}\n00000000-0000-4000-8000-000000000007\t20260826\t0\t{ROOT}\t{INACTIVE_CONCEPT_REFSET}\t{RIGHT}\n"));
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
    descriptor_fixture_with(
        path,
        invalid_decimal,
        &["7", "9223372036854775807", "99999999999999999999999"],
    );
}

fn descriptor_fixture_with(path: &Path, invalid_decimal: bool, integers: &[&str]) {
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
                900000000000461009,
                900000000000474003,
                900000000000475002,
                900000000000476001,
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
    let mut descriptors = String::new();
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
    // An Integer descriptor on a string-typed file column declares integer semantics.
    for (position, kind) in [900000000000461009u64, 900000000000476001]
        .into_iter()
        .enumerate()
    {
        descriptors.push_str(&format!("00000000-0000-4000-8000-00000000110{position}\t20260826\t1\t{ROOT}\t900000000000456007\t{KIND}\t{ROOT}\t{kind}\t{position}\n"));
    }
    files
        .iter_mut()
        .find(|(name, _)| name.contains("RefsetDescriptor"))
        .unwrap()
        .1
        .push_str(&descriptors);
    let amount = if invalid_decimal {
        "NaN"
    } else {
        "0.100000000000000001"
    };
    files.push(("Synthetic/Snapshot/Refset/der2_sssRefset_CustomSnapshot.txt".into(), format!("id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tcustom Amount\tReview Date\tlinkUuid\n00000000-0000-4000-8000-000000002001\t20260826\t1\t{ROOT}\t800001\t{LEAF}\t{amount}\t20260801\t00000000-0000-4000-8000-000000009001\n00000000-0000-4000-8000-000000002002\t20260826\t1\t{ROOT}\t800001\t{RIGHT}\t0.1\t\t00000000-0000-4000-8000-000000009002\n")));
    // Undescribed integer fields fall back to the filename type; a declared Integer descriptor
    // applies the same rules to a string-typed file column. Members cycle through LEAF, RIGHT, LEFT.
    let members = [LEAF, RIGHT, LEFT];
    for (file, refset) in [
        ("der2_iRefset_WideSnapshot.txt", LEFT),
        ("der2_sRefset_DeclaredSnapshot.txt", KIND),
    ] {
        let mut body =
            "id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tsequence\n"
                .to_owned();
        for (i, value) in integers.iter().enumerate() {
            body.push_str(&format!("00000000-0000-4000-8000-0000000{}{i:03}\t20260826\t1\t{ROOT}\t{refset}\t{}\t{value}\n", if refset == LEFT { 30 } else { 31 }, members[i % 3]));
        }
        files.push((format!("Synthetic/Snapshot/Refset/{file}"), body));
    }
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
    let MemberColumn::Number(number) = &mut table.columns[4] else {
        panic!()
    };
    number.text.replace_range(..3, "NaN");
    assert!(table.validate().is_err());
    // Integer values beyond i64 keep the whole column exact instead of failing the import,
    // whether the type comes from the filename or from an Integer descriptor on a string column.
    for refset in [LEFT, KIND] {
        let wide = store.member_tables.get(refset).unwrap().unwrap();
        assert!(matches!(wide.columns[4], MemberColumn::Number(_)));
        for (query, expected) in [
            (format!("^{refset} {{{{M sequence=#7}}}}"), vec![LEAF]),
            (
                format!("^{refset} {{{{M sequence>#9223372036854775806}}}}"),
                vec![LEFT, RIGHT],
            ),
            (
                format!("^{refset} {{{{M sequence=#99999999999999999999999.0}}}}"),
                vec![LEFT],
            ),
        ] {
            assert_eq!(
                evaluate(&store, &parse(&query).unwrap())
                    .unwrap()
                    .iter()
                    .map(|&o| store.ids[o as usize])
                    .collect::<Vec<_>>(),
                expected,
                "{query}"
            );
        }
        assert_eq!(
            evaluate_result(&store, &parse(&format!("^[sequence]{refset}")).unwrap()).unwrap(),
            QueryResult::Values(vec![
                MemberValue::Number("7".into()),
                MemberValue::Number("9223372036854775807".into()),
                MemberValue::Number("99999999999999999999999".into()),
            ])
        );
    }
    // Integer lexemes stay integers after promotion; decimals, signs and leading zeros fail.
    for (integers, valid) in [
        (&["7", "99999999999999999999999", "1.5"][..], false),
        (&["7", "99999999999999999999999", "+3"][..], false),
        (&["7", "99999999999999999999999", "007"][..], false),
        (&["7", "-0", "1"][..], false),
        (&["1.5", "7", "9"][..], false),
        (&["-3", "0", "-99999999999999999999999"][..], true),
    ] {
        let archive = temp
            .path()
            .join(format!("integers-{}.zip", integers.join("_")));
        let store_path = temp.path().join(format!("integers-{}", integers.join("_")));
        descriptor_fixture_with(&archive, false, integers);
        let imported = import_snapshot(&archive, &store_path, &options(&archive));
        assert_eq!(imported.is_ok(), valid, "{integers:?}");
        if valid {
            let store = NumericStore::open(&store_path).unwrap();
            assert_eq!(
                evaluate_result(&store, &parse(&format!("^[sequence]{KIND}")).unwrap()).unwrap(),
                // Scalar sets order values by their canonical spelling.
                QueryResult::Values(vec![
                    MemberValue::Number("-3".into()),
                    MemberValue::Number("-99999999999999999999999".into()),
                    MemberValue::Number("0".into()),
                ])
            );
        }
    }
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
    assert_eq!(manifest.active_concept_count, 19);
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
    let display = DisplayStore::open(&destination).unwrap();
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
        search: Default::default(),
        history: Default::default(),
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
    assert_eq!(manifest["manifest"]["active_concept_count"], 19);
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
    assert_eq!(metadata.snapshot_files, 4);
    // The domain comes from descriptors and rows of every status: the declared and the
    // inactive-only description sets are outside it, the inactive-only concept set is inside.
    assert_eq!(
        metadata.non_concept_refsets,
        Some(vec![
            INACTIVE_DESCRIPTION_REFSET,
            DECLARED_DESCRIPTION_REFSET,
            900000000000508004,
            999001261000000100
        ])
    );
    assert_eq!(
        metadata.concept_refsets,
        Some(vec![
            ROOT,
            INACTIVE_CONCEPT_REFSET,
            DESCRIPTOR_REFSET,
            900000000000534007
        ])
    );
    let store = NumericStore::open(&destination).unwrap();
    let codes = |query: &str| {
        evaluate(&store, &parse(query).unwrap()).map(|result| {
            result
                .iter()
                .map(|&i| store.ids[i as usize])
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(codes(&format!("^{ROOT}")).unwrap(), [LEFT, LEAF, INACTIVE]);
    // Section 6.1 excludes description-based reference sets from memberOf; the specified
    // `^ *` query and mixed selections keep returning the concept-based members.
    for query in [
        "^900000000000508004".to_owned(),
        "^(900000000000508004 OR 999001261000000100)".into(),
        format!("^(900000000000508004 OR {LEFT})"),
        "^[referencedComponentId]900000000000508004".into(),
        "^900000000000508004 {{M active=1}}".into(),
        format!("^{INACTIVE_DESCRIPTION_REFSET}"),
        format!("^{INACTIVE_DESCRIPTION_REFSET} {{{{M active=0}}}}"),
        format!("^{DECLARED_DESCRIPTION_REFSET}"),
        format!("^({INACTIVE_DESCRIPTION_REFSET} OR {DECLARED_DESCRIPTION_REFSET}) {{{{M active=\"*\"}}}}"),
    ] {
        assert!(
            matches!(codes(&query), Err(EvalError::Semantic(_))),
            "{query}"
        );
    }
    assert_eq!(
        codes("^*").unwrap(),
        [LEFT, LEAF, INACTIVE, DECLARED_DESCRIPTION_REFSET]
    );
    assert_eq!(
        codes(&format!("^(900000000000508004 OR {ROOT})")).unwrap(),
        [LEFT, LEAF, INACTIVE]
    );
    // A concept-based set whose rows are all inactive stays in the domain, alone and beside
    // a description-based set, so its inactive members remain reachable.
    assert_eq!(
        codes(&format!("^{INACTIVE_CONCEPT_REFSET}")).unwrap(),
        Vec::<u64>::new()
    );
    assert_eq!(
        codes(&format!("^{INACTIVE_CONCEPT_REFSET} {{{{M active=0}}}}")).unwrap(),
        [RIGHT]
    );
    assert_eq!(
        codes(&format!(
            "^({INACTIVE_DESCRIPTION_REFSET} OR {INACTIVE_CONCEPT_REFSET}) {{{{M active=\"*\"}}}}"
        ))
        .unwrap(),
        [RIGHT]
    );
    assert_eq!(
        codes(&format!(
            "^({INACTIVE_DESCRIPTION_REFSET} OR {INACTIVE_CONCEPT_REFSET})"
        ))
        .unwrap(),
        Vec::<u64>::new()
    );
    assert_eq!(
        codes(&format!("^R {RIGHT} {{{{M active=0}}}}")).unwrap(),
        [ROOT, INACTIVE_CONCEPT_REFSET]
    );
    assert_eq!(
        codes(&format!(
            "^(900000000000508004 OR {ROOT}) {{{{M active=0}}}}"
        ))
        .unwrap(),
        [RIGHT]
    );
    assert_eq!(codes("^R 900000000000508004").unwrap(), Vec::<u64>::new());
    assert_eq!(
        codes(&format!("^R {LEFT}")).unwrap(),
        [ROOT, 900000000000534007]
    );
    // A manifest without the classification behaves like the earlier format.
    let mut legacy = Manifest::read(&destination).unwrap();
    let membership = legacy.membership.as_mut().unwrap();
    membership.concept_refsets = None;
    membership.non_concept_refsets = None;
    serde_json::to_writer(
        File::create(destination.join("manifest.json")).unwrap(),
        &legacy,
    )
    .unwrap();
    let legacy_store = NumericStore::open(&destination).unwrap();
    assert!(
        evaluate(&legacy_store, &parse("^900000000000508004").unwrap())
            .unwrap()
            .is_empty()
    );
    serde_json::to_writer(
        File::create(destination.join("manifest.json")).unwrap(),
        &manifest,
    )
    .unwrap();
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

/// Runs the CLI with an isolated selection file, so tests never read or write
/// the developer's own selected index.
fn cli(config: &Path, arguments: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .args(arguments)
        .env("XDG_CONFIG_HOME", config)
        .env("APPDATA", config)
        .env_remove("SNOMED_ECL_STORE")
        .output()
        .unwrap()
}

#[test]
fn cli_remembers_a_selected_index_and_finds_the_indexes_on_disk() {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let archive = temp.path().join("fixture.zip");
    let store = temp.path().join("store");
    let other = temp.path().join("other");
    fixture(&archive, false, false);
    import_snapshot(&archive, &store, &options(&archive)).unwrap();
    import_snapshot(&archive, &other, &options(&archive)).unwrap();
    let store_text = store.to_str().unwrap();
    let other_text = other.to_str().unwrap();

    // Without a selection the store cannot be guessed, and no partial result is
    // written to stdout.
    let unselected = cli(&config, &["expand", "<< 1000001", "--count"]);
    assert!(!unselected.status.success());
    assert!(unselected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unselected.stderr).contains("No index selected"));

    // A path that is not an index is refused before anything is recorded.
    assert!(!cli(&config, &["use", archive.to_str().unwrap()])
        .status
        .success());
    assert!(!cli(&config, &["expand", "<< 1000001", "--count"])
        .status
        .success());

    assert!(cli(&config, &["use", store_text]).status.success());
    let selected = cli(&config, &["expand", "<< 1000001", "--count"]);
    assert!(selected.status.success());
    let total = String::from_utf8_lossy(&selected.stdout).trim().to_owned();
    assert_eq!(
        total,
        String::from_utf8_lossy(
            &cli(&config, &["expand", store_text, "<< 1000001", "--count"]).stdout
        )
        .trim()
    );

    // The environment overrides the selection, and an explicit path overrides
    // both.
    let overridden = std::process::Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .args(["expand", "<< 1000001", "--count"])
        .env("XDG_CONFIG_HOME", &config)
        .env("APPDATA", &config)
        .env("SNOMED_ECL_STORE", temp.path().join("absent"))
        .output()
        .unwrap();
    assert!(!overridden.status.success());
    let explicit = std::process::Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .args(["expand", other_text, "<< 1000001", "--count"])
        .env("XDG_CONFIG_HOME", &config)
        .env("APPDATA", &config)
        .env("SNOMED_ECL_STORE", temp.path().join("absent"))
        .output()
        .unwrap();
    assert!(explicit.status.success());
    assert_eq!(String::from_utf8_lossy(&explicit.stdout).trim(), total);

    // Discovery reports both indexes, marks the selected one and ignores the
    // archive sitting beside them.
    let listed = cli(&config, &["stores", temp.path().to_str().unwrap()]);
    assert!(listed.status.success());
    let rows: Vec<serde_json::Value> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().filter(|row| row["selected"] == true).count(), 1);
    assert!(rows
        .iter()
        .all(|row| row["edition"] == options(&archive).edition && row["packed"] == false));

    assert!(cli(&config, &["use", "--clear"]).status.success());
    assert!(!cli(&config, &["expand", "<< 1000001", "--count"])
        .status
        .success());
}

#[test]
fn opening_checks_bounds_and_verification_checks_meaning() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let store_path = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &store_path, &options(&archive)).unwrap();

    // A sound index passes both.
    let mut store = NumericStore::open(&store_path).unwrap();
    store.validate_bounds().unwrap();
    store.validate().unwrap();

    // An edge pointing outside the concept array would let evaluation index
    // out of range, so opening has to reject it.
    let edge = store.parents.values[0];
    store.parents.values[0] = store.ids.len() as u32;
    assert!(
        store.validate_bounds().is_err(),
        "out-of-range edge must fail bounds"
    );
    assert!(store.validate().is_err());
    store.parents.values[0] = edge;

    // Unordered concept IDs stay in range, so bounds pass. They break binary
    // search, which is a semantic property, so verification has to catch it.
    let mut store = NumericStore::open(&store_path).unwrap();
    store.ids.swap(0, 1);
    store.validate_bounds().unwrap();
    assert!(
        store.validate().is_err(),
        "unordered IDs must fail verification"
    );

    // Same split for the hierarchy: dropping the reverse edge keeps every
    // index in range but makes the two directions disagree.
    let mut store = NumericStore::open(&store_path).unwrap();
    let last = store.children.values.len() - 1;
    store.children.values[last] = store.children.values[0];
    store.validate_bounds().unwrap();
    assert!(
        store.validate().is_err(),
        "disagreeing directions must fail verification"
    );
}

#[test]
fn batch_returns_labels_only_when_they_are_asked_for() {
    use std::process::{Command, Stdio};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();

    let mut process = Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .arg("batch")
        .arg(&destination)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = process.stdin.take().unwrap();
        writeln!(input, "{{\"ecl\":\"{LEAF}\",\"display\":true}}").unwrap();
        writeln!(input, "{{\"ecl\":\"{LEAF}\"}}").unwrap();
        // Counting asks for no concepts, so a label has nothing to attach to.
        writeln!(
            input,
            "{{\"ecl\":\"{LEAF}\",\"display\":true,\"count_only\":true}}"
        )
        .unwrap();
    }
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);

    // Asked for: objects carrying the code and its label.
    let labelled = &lines[0]["concepts"];
    assert_eq!(labelled[0]["code"], LEAF.to_string());
    assert!(
        labelled[0]["display"].is_string(),
        "a label should be resolved"
    );
    assert!(lines[0]["codes"].is_null(), "not both forms at once");

    // Not asked for: bare codes, and no label lookup paid for.
    assert_eq!(lines[1]["codes"][0], LEAF.to_string());
    assert!(lines[1]["concepts"].is_null());

    // Counting returns neither.
    assert_eq!(lines[2]["total"], 1);
    assert!(lines[2]["concepts"].is_null());
    assert!(lines[2]["codes"].is_null());
}

#[test]
fn batch_workers_answer_every_request_under_its_id() {
    use std::process::{Command, Stdio};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let requests: Vec<String> = (0..200)
        .map(|i| match i % 5 {
            0 => format!("{{\"id\":{i},\"ecl\":\"<< {ROOT}\",\"display\":true}}"),
            1 => format!("{{\"id\":\"r{i}\",\"ecl\":\"{LEAF}\",\"count_only\":true}}"),
            2 => format!("{{\"id\":{i},\"concept\":\"{LEFT}\"}}"),
            3 => format!("{{\"id\":{i},\"ecl\":\"<<\"}}"),
            _ => format!("{{\"id\":{i},\"unknown\":true}}"),
        })
        .collect();
    let run = |extra: &[&str]| -> Vec<serde_json::Value> {
        let mut process = Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
            .arg("batch")
            .arg(&destination)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut input = process.stdin.take().unwrap();
            for request in &requests {
                writeln!(input, "{request}").unwrap();
            }
        }
        let output = process.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
                value.as_object_mut().unwrap().retain(|k, _| !k.ends_with("_ms"));
                value
            })
            .collect()
    };
    let sequential = run(&[]);
    let mut concurrent = run(&["--workers", "4"]);
    assert_eq!(sequential.len(), requests.len());
    // Sequential answers come in order, each carrying its id.
    for (i, answer) in sequential.iter().enumerate() {
        let id = if i % 5 == 1 { serde_json::json!(format!("r{i}")) } else { serde_json::json!(i) };
        assert_eq!(answer["id"], id, "{answer}");
    }
    assert!(sequential[3]["error"].is_string(), "a parse error still carries its id");
    assert_eq!(sequential[4]["error"], "InvalidRequest");
    // Workers may answer in any order, but the same answers under the same ids.
    let key = |value: &serde_json::Value| value["id"].to_string();
    concurrent.sort_by_key(key);
    let mut expected = sequential.clone();
    expected.sort_by_key(key);
    assert_eq!(concurrent, expected);
}

#[test]
fn one_concept_reads_the_same_descriptions_as_the_loaded_index() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let packed = temp.path().join("store.ecl");
    snomed_ecl_engine::store::pack(&destination, &packed).unwrap();
    for path in [&destination, &packed] {
        let loaded = NumericStore::open(path).unwrap();
        loaded.descriptions.get().unwrap().unwrap();
        let seeking = NumericStore::open(path).unwrap();
        let mut seen = 0;
        for concept in 0..loaded.ids.len() as u32 {
            let expected = loaded.descriptions.concept_rows(concept).unwrap().unwrap();
            let read = seeking.descriptions.concept_rows(concept).unwrap().unwrap();
            assert_eq!(read, expected, "concept {}", loaded.ids[concept as usize]);
            seen += read.len();
        }
        assert!(seen > 0, "the fixture has descriptions");
        assert!(seeking.descriptions.concept_rows(loaded.ids.len() as u32).is_err());
    }
}

#[test]
fn search_within_an_expression_keeps_only_its_concepts() {
    use std::process::{Command, Stdio};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    // Import builds the word index.
    let mut process = Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .arg("batch")
        .arg(&destination)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = process.stdin.take().unwrap();
        writeln!(input, "{{\"search\":\"synthetic\"}}").unwrap();
        writeln!(input, "{{\"search\":\"synthetic\",\"within\":\"<< {LEFT}\"}}").unwrap();
        writeln!(input, "{{\"search\":\"synthetic\",\"within\":\"{ROOT}\"}}").unwrap();
        writeln!(input, "{{\"search\":\"synthetic\",\"within\":\"{LEFT}\"}}").unwrap();
        writeln!(input, "{{\"search\":\"synthetic\",\"within\":\"<<\"}}").unwrap();
    }
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let codes = |line: &serde_json::Value| -> Vec<String> {
        let mut codes: Vec<String> = line["concepts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["code"].as_str().unwrap().to_owned())
            .collect();
        codes.sort();
        codes
    };
    // RIGHT matches through its text definition.
    assert_eq!(
        codes(&lines[0]),
        [ROOT.to_string(), RIGHT.to_string(), LEAF.to_string()]
    );
    assert_eq!(codes(&lines[1]), [LEAF.to_string()]);
    assert_eq!(lines[1]["total"], 1);
    assert_eq!(codes(&lines[2]), [ROOT.to_string()]);
    assert_eq!(lines[3]["total"], 0);
    assert_eq!(lines[4]["error"], "Syntax");
}

#[test]
fn description_filters_on_a_small_focus_agree_with_the_loaded_index() {
    use snomed_ecl_engine::{ecl::parse, eval::evaluate};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    let destination = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &destination, &options(&archive)).unwrap();
    let packed = temp.path().join("store.ecl");
    snomed_ecl_engine::store::pack(&destination, &packed).unwrap();
    let mut queries = vec![
        format!("* {{{{ D active = 0 }}}}"),
        format!("* {{{{ D active = 1 }}}}"),
        format!("<< {ROOT} {{{{ D type = fsn }}}}"),
        format!("<< {ROOT} {{{{ D type != fsn }}}}"),
        format!("* {{{{ D language = en }}}}"),
        format!("* {{{{ D language != en }}}}"),
        format!("* {{{{ D id = 6000012 }}}}"),
        format!("* {{{{ D moduleId = {ROOT} }}}}"),
        format!("* {{{{ D effectiveTime >= \"20260826\" }}}}"),
        format!("* {{{{ D dialect = en-gb }}}}"),
        format!("* {{{{ D dialect = en-gb (prefer) }}}}"),
        format!("* {{{{ D dialect != en-gb }}}}"),
        format!("* {{{{ D active = *, type = syn }}}}"),
        format!("<< {LEFT} {{{{ D active = 0 }}}} {{{{ D language = en }}}}"),
    ];
    if cfg!(feature = "unicode") {
        queries.push("* {{ D term = \"synthetic\" }}".to_string());
        queries.push("* {{ D term = wild:\"*label\" }}".to_string());
    }
    for path in [&destination, &packed] {
        let loaded = NumericStore::open(path).unwrap();
        loaded.descriptions.get().unwrap().unwrap();
        for query in &queries {
            let expression = parse(query).unwrap_or_else(|e| panic!("{query}: {e}"));
            // A fresh store has not loaded the index, so a small focus reads rows.
            let fresh = NumericStore::open(path).unwrap();
            assert_eq!(
                evaluate(&fresh, &expression),
                evaluate(&loaded, &expression),
                "{query}"
            );
            assert!(!fresh.descriptions.is_loaded(), "{query} loaded the index");
        }
    }
}

#[test]
fn cli_inspect_reports_what_an_archive_declares_before_importing() {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let archive = temp.path().join("fixture.zip");
    fixture(&archive, false, false);
    let archive_text = archive.to_str().unwrap();

    let output = cli(&config, &["inspect", archive_text]);
    assert!(output.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["sha256"], sha256(&archive).unwrap());
    assert_eq!(summary["effective_time"], "20260826");
    assert!(summary["importable"].as_bool().unwrap());
    // The one module carrying the release date is nothing else's dependency, so
    // it is offered as the edition and the offer is unambiguous.
    assert_eq!(summary["root_editions"], 1);
    assert_eq!(
        summary["edition_uris"],
        serde_json::json!([options(&archive).edition])
    );
    // The URI it offers is the one the importer accepts.
    let store = temp.path().join("store");
    import_snapshot(&archive, &store, &options(&archive)).unwrap();

    // An archive without the required Snapshot files is reported, not imported.
    let empty = temp.path().join("empty.zip");
    {
        let mut writer = zip::ZipWriter::new(File::create(&empty).unwrap());
        writer
            .start_file(
                "Synthetic/release_package_information.json",
                SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(br#"{"effectiveTime":"20260826"}"#)
            .unwrap();
        writer.finish().unwrap();
    }
    let output = cli(&config, &["inspect", empty.to_str().unwrap()]);
    assert!(output.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!summary["importable"].as_bool().unwrap());
    assert!(summary["required_files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry[1].is_null()));
}

#[test]
fn cli_diffs_one_expression_between_two_indexes() {
    use snomed_ecl_engine::import::add_refsets_snapshot;
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let archive = temp.path().join("fixture.zip");
    let base = temp.path().join("base");
    fixture(&archive, false, false);
    import_snapshot(&archive, &base, &options(&archive)).unwrap();
    let extra = temp.path().join("extra.zip");
    supplement_fixture(&extra, LEAF, false);
    let combined = temp.path().join("combined");
    let hash = sha256(&extra).unwrap();
    add_refsets_snapshot(&base, &extra, &combined, "20260820", &hash).unwrap();
    let (base_text, combined_text) = (base.to_str().unwrap(), combined.to_str().unwrap());

    let forward = cli(
        &config,
        &["diff", base_text, combined_text, "<< 1000001", "--json"],
    );
    assert!(forward.status.success());
    let report: serde_json::Value = serde_json::from_slice(&forward.stdout).unwrap();
    // The supplement's new concept is a descendant of the root; its module is
    // not, so exactly one concept joins the expansion.
    assert_eq!(report["added"], serde_json::json!(["2000001"]));
    assert_eq!(report["removed"], serde_json::json!([]));
    assert_eq!(
        report["unchanged"].as_u64().unwrap(),
        report["old"]["total"].as_u64().unwrap()
    );
    assert_eq!(
        report["new"]["total"].as_u64().unwrap(),
        report["old"]["total"].as_u64().unwrap() + 1
    );

    // Reversing the indexes reports the same concept as removed.
    let backward = cli(
        &config,
        &["diff", combined_text, base_text, "<< 1000001", "--json"],
    );
    let reversed: serde_json::Value = serde_json::from_slice(&backward.stdout).unwrap();
    assert_eq!(reversed["removed"], serde_json::json!(["2000001"]));
    assert_eq!(reversed["added"], serde_json::json!([]));

    // A projection returning values has nothing to compare as concepts, and
    // says so rather than reporting an empty difference.
    let projection = cli(
        &config,
        &[
            "diff",
            base_text,
            combined_text,
            "^[sourceEffectiveTime]900000000000534007",
        ],
    );
    assert!(!projection.status.success());
    assert!(projection.stdout.is_empty());
    assert!(String::from_utf8_lossy(&projection.stderr).contains("concept results"));
}

#[test]
fn cli_points_at_the_text_a_parse_error_rejected() {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("config");
    let archive = temp.path().join("fixture.zip");
    let store = temp.path().join("store");
    fixture(&archive, false, false);
    import_snapshot(&archive, &store, &options(&archive)).unwrap();
    let failed = cli(
        &config,
        &[
            "expand",
            store.to_str().unwrap(),
            "<< 1000001 : 1000002 = = 1000003",
        ],
    );
    assert!(!failed.status.success());
    let message = String::from_utf8_lossy(&failed.stderr);
    // The caret sits under the offset the parser reported, on its own line.
    let (line, caret) = message
        .lines()
        .zip(message.lines().skip(1))
        .find(|(_, next)| next.trim_start().starts_with('^'))
        .unwrap();
    assert!(line.contains("<< 1000001 : 1000002 = = 1000003"));
    assert_eq!(caret.find('^'), Some(line.find("= =").unwrap() + 2));
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
    let (base_membership, merged) = (
        original.membership.as_ref().unwrap(),
        combined.membership.as_ref().unwrap(),
    );
    assert_eq!(
        merged.non_concept_refsets,
        base_membership.non_concept_refsets
    );
    let mut expected = base_membership.concept_refsets.clone().unwrap();
    expected.push(2000001);
    expected.sort_unstable();
    assert_eq!(merged.concept_refsets, Some(expected));
    assert!(matches!(
        evaluate(&store, &parse("^900000000000508004").unwrap()),
        Err(snomed_ecl_engine::eval::EvalError::Semantic(_))
    ));
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
    let displays = DisplayStore::open(&output).unwrap();
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
        descriptions
            .term(
                descriptions
                    .for_concept(store.ordinal(RIGHT).unwrap())
                    .start
            )
            .unwrap(),
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

#[test]
fn import_reports_as_many_stages_as_it_announces() {
    use snomed_ecl_engine::import::{import_snapshot_with_progress, IMPORT_STAGES};
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("fixture.zip");
    fixture(&archive, false, false);
    let mut stages = 0;
    import_snapshot_with_progress(&archive, &temp.path().join("store"), &options(&archive), |_| {
        stages += 1
    })
    .unwrap();
    assert_eq!(stages, IMPORT_STAGES);
}
