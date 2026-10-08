#![cfg(feature = "import")]

use snomed_ecl_engine::{
    ecl::parse,
    eval::evaluate,
    import::{import_snapshot, inspect_archive, ImportOptions, INTERNATIONAL_DISPLAY_REFSETS},
    store::{pack, sha256, verify, DisplayStore, Manifest, NumericStore},
};
use std::{fs::File, io::Write, path::Path};
use tempfile::TempDir;
use zip::{write::SimpleFileOptions, ZipWriter};

const INTERNATIONAL: u64 = 900000000000207008;
const CORE: u64 = 900000000000012004;
const MAP: u64 = 2000002;
const ROOT: u64 = 1000001;
const US: u64 = 900000000000509007;
const GB: u64 = 900000000000508004;
const FSN: u64 = 900000000000003001;
const SYNONYM: u64 = 900000000000013009;
const ISA: u64 = 116680003;
const FOLDER: &str = "SnomedCT_InternationalRF2_PRODUCTION_20260801T120000Z";
const EDITION: &str = "http://snomed.info/sct/900000000000207008/version/20260801";
const EXTENSION: &str = "http://snomed.info/sct/2000001/version/20260801";

// Every RF2 row is synthetic. Full files deliberately have invalid content:
// only the Snapshot files may be read, even though the ZIP contains both.
fn package(zip: &mut ZipWriter<File>, folder: &str, edition: u64, extension_only: bool) {
    let mut add = |name: &str, body: String| {
        zip.start_file(format!("{folder}/{name}"), SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    if folder != "SyntheticUK" {
        add(
            "release_package_information.json",
            r#"{"effectiveTime":"20260801"}"#.into(),
        );
    }
    let mut concepts = "id\teffectiveTime\tactive\tmoduleId\tdefinitionStatusId\n".to_owned();
    for code in [
        edition,
        CORE,
        MAP,
        ROOT,
        1000002,
        1000003,
        1000004,
        ISA,
        US,
        GB,
        FSN,
        SYNONYM,
        900000000000548007,
        900000000000534007,
    ] {
        if extension_only && code == CORE {
            continue;
        }
        concepts.push_str(&format!(
            "{code}\t20260801\t1\t{CORE}\t900000000000074008\n"
        ));
    }
    add("Snapshot/Terminology/sct2_Concept_Snapshot.txt", concepts);
    let mut relationships = "id\teffectiveTime\tactive\tmoduleId\tsourceId\tdestinationId\trelationshipGroup\ttypeId\tcharacteristicTypeId\tmodifierId\n".to_owned();
    for (row, child) in [1000002, 1000003, 1000004].into_iter().enumerate() {
        relationships.push_str(&format!("{}\t20260801\t1\t{edition}\t{child}\t{ROOT}\t0\t{ISA}\t900000000000011006\t900000000000451002\n", 3000001 + row));
    }
    add(
        "Snapshot/Terminology/sct2_Relationship_Snapshot.txt",
        relationships,
    );
    add("Snapshot/Terminology/sct2_RelationshipConcreteValues_Snapshot.txt", "id\teffectiveTime\tactive\tmoduleId\tsourceId\tvalue\trelationshipGroup\ttypeId\tcharacteristicTypeId\tmodifierId\n".into());
    let mut dependencies = "id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tsourceEffectiveTime\ttargetEffectiveTime\n".to_owned();
    for (row, module, target) in [(1, edition, CORE), (2, MAP, edition)] {
        dependencies.push_str(&format!("00000000-0000-4000-8000-{row:012}\t20260801\t1\t{module}\t900000000000534007\t{target}\t20260801\t20260801\n"));
    }
    if extension_only {
        dependencies.push_str(&format!("00000000-0000-4000-8000-000000000003\t20260801\t1\t{edition}\t900000000000534007\t2000003\t20260801\t20260801\n"));
    }
    add(
        "Snapshot/Refset/der2_ssRefset_ModuleDependencySnapshot.txt",
        dependencies,
    );
    let mut descriptions = "id\teffectiveTime\tactive\tmoduleId\tconceptId\tlanguageCode\ttypeId\tterm\tcaseSignificanceId\n".to_owned();
    for (id, concept, kind, term) in [
        (6000011, ROOT, FSN, "Synthetic root (test)"),
        (6000012, 1000002, SYNONYM, "Synthetic GB label"),
        (6000013, 1000002, SYNONYM, "Synthetic US label"),
        (6000014, 1000002, FSN, "Synthetic child (test)"),
        (6000015, 1000003, FSN, "Synthetic fallback (test)"),
        (6000016, 1000003, SYNONYM, "Synthetic unpreferred synonym"),
        (6000017, 1000004, SYNONYM, "Synthetic GB fallback"),
        (6000018, 1000004, FSN, "Synthetic other child (test)"),
    ] {
        descriptions.push_str(&format!(
            "{id}\t20260801\t1\t{edition}\t{concept}\ten\t{kind}\t{term}\t900000000000448009\n"
        ));
    }
    add(
        "Snapshot/Terminology/sct2_Description_Snapshot.txt",
        descriptions,
    );
    let mut language =
        "id\teffectiveTime\tactive\tmoduleId\trefsetId\treferencedComponentId\tacceptabilityId\n"
            .to_owned();
    for (row, refset, description) in [(4, US, 6000013), (5, GB, 6000012), (6, GB, 6000017)] {
        language.push_str(&format!("00000000-0000-4000-8000-{row:012}\t20260801\t1\t{edition}\t{refset}\t{description}\t900000000000548007\n"));
    }
    add(
        "Snapshot/Refset/der2_cRefset_LanguageSnapshot.txt",
        language,
    );
    for name in [
        "Terminology/sct2_Concept_Full.txt",
        "Terminology/sct2_Relationship_Full.txt",
        "Terminology/sct2_RelationshipConcreteValues_Full.txt",
        "Terminology/sct2_Description_Full.txt",
        "Refset/der2_ssRefset_ModuleDependencyFull.txt",
        "Refset/der2_cRefset_LanguageFull.txt",
    ] {
        add(
            &format!("Full/{name}"),
            "Full decoy: must not be read\n".into(),
        );
    }
}

fn fixture(path: &Path, extension_only: bool, merged: bool) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    if extension_only {
        package(&mut zip, "SyntheticExtension", 2000001, true);
    } else {
        package(&mut zip, FOLDER, INTERNATIONAL, false);
    }
    if merged {
        package(&mut zip, "SyntheticUK", 83821000000107, false);
    }
    zip.finish().unwrap();
}

#[test]
fn international_imports_packs_verifies_and_uses_us_then_gb_then_fsn() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("international.zip");
    fixture(&archive, false, false);
    let summary = inspect_archive(&archive).unwrap();
    assert!(summary.importable);
    assert_eq!(summary.root_editions, 1);
    assert_eq!(
        summary.edition_uris[0],
        "http://snomed.info/sct/2000002/version/20260801"
    );
    assert!(summary.edition_uris.iter().any(|uri| uri == EDITION));
    assert_eq!(
        summary.choose_edition(Some(INTERNATIONAL)).unwrap(),
        EDITION
    );
    assert_ne!(summary.choose_edition(None).unwrap(), EDITION);
    let directory = temp.path().join("index");
    let manifest = import_snapshot(
        &archive,
        &directory,
        &ImportOptions::new(
            summary.choose_edition(Some(INTERNATIONAL)).unwrap(),
            &summary.sha256,
        ),
    )
    .unwrap();
    assert_eq!(manifest.display_refsets, INTERNATIONAL_DISPLAY_REFSETS);
    let packed = temp.path().join("int-20260801.ecl");
    pack(&directory, &packed).unwrap();
    for source in [&directory, &packed] {
        verify(source).unwrap();
        let store = NumericStore::open(source).unwrap();
        let codes: Vec<_> = evaluate(&store, &parse("<< 1000001").unwrap())
            .unwrap()
            .into_iter()
            .map(|ordinal| store.ids[ordinal as usize])
            .collect();
        assert_eq!(codes, [ROOT, 1000002, 1000003, 1000004]);
        let displays = DisplayStore::open(source).unwrap();
        for (code, label) in [
            (1000002, "Synthetic US label"),
            (1000003, "Synthetic fallback (test)"),
            (1000004, "Synthetic GB fallback"),
        ] {
            assert_eq!(
                displays
                    .get(store.ordinal(code).unwrap())
                    .unwrap()
                    .as_deref(),
                Some(label)
            );
        }
    }
    // Exercise both CLI paths, including an explicit display override.
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
            .args(args)
            .env("SNOMED_ECL_HOME", temp.path().join("library"))
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("APPDATA", temp.path().join("config"))
            .env_remove("SNOMED_ECL_STORE")
            .output()
            .unwrap()
    };
    let checksum = sha256(&archive).unwrap();
    let added = run(&[
        "add",
        archive.to_str().unwrap(),
        "--edition",
        EDITION,
        "--sha256",
        &checksum,
    ]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let selected = temp.path().join("library/int-20260801.ecl");
    assert_eq!(
        Manifest::read(&selected).unwrap().display_refsets,
        INTERNATIONAL_DISPLAY_REFSETS
    );
    let expanded = run(&["expand", "int", "<< 1000001", "--count"]);
    assert!(
        expanded.status.success(),
        "{}",
        String::from_utf8_lossy(&expanded.stderr)
    );
    assert_eq!(String::from_utf8(expanded.stdout).unwrap().trim(), "4");
    for (name, explicit) in [("default", false), ("override", true)] {
        let destination = temp.path().join(name);
        let mut args = vec![
            "import",
            archive.to_str().unwrap(),
            destination.to_str().unwrap(),
            EDITION,
            &checksum,
        ];
        if explicit {
            args.push("900000000000508004");
        }
        let imported = run(&args);
        assert!(
            imported.status.success(),
            "{}",
            String::from_utf8_lossy(&imported.stderr)
        );
        let expected = if explicit {
            &[GB][..]
        } else {
            INTERNATIONAL_DISPLAY_REFSETS
        };
        assert_eq!(
            Manifest::read(&destination).unwrap().display_refsets,
            expected
        );
    }
}

#[test]
fn add_without_an_edition_selects_international_even_when_it_is_not_a_root() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("international.zip");
    fixture(&archive, false, false);
    let summary = inspect_archive(&archive).unwrap();
    assert_eq!(summary.root_editions, 1);
    assert_ne!(summary.edition_uris[0], EDITION);
    assert!(summary.edition_uris[summary.root_editions..]
        .iter()
        .any(|uri| uri == EDITION));
    let library = temp.path().join("library");
    let added = std::process::Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .arg("add")
        .arg(&archive)
        .args(["--sha256", &summary.sha256, "--json"])
        .env("SNOMED_ECL_HOME", &library)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("APPDATA", temp.path().join("config"))
        .env_remove("SNOMED_ECL_STORE")
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&added.stdout).unwrap();
    assert_eq!(result["edition"], EDITION);
    assert_eq!(result["name"], "int-20260801");
    let manifest = Manifest::read(&library.join("int-20260801.ecl")).unwrap();
    assert_eq!(manifest.edition, EDITION);
    assert_eq!(manifest.display_refsets, INTERNATIONAL_DISPLAY_REFSETS);
    assert!(manifest.source.is_none());
}

#[test]
fn side_by_side_packages_are_reported_by_inspect_and_rejected_by_import() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("merged.zip");
    fixture(&archive, false, true);
    let inspected = inspect_archive(&archive).unwrap();
    assert!(!inspected.importable);
    assert_eq!(inspected.effective_time, "20260801");
    assert_eq!(inspected.duplicate_files.len(), 6);
    assert!(inspected
        .required_files
        .iter()
        .all(|(_, found)| found.is_none()));
    for (_, names) in &inspected.duplicate_files {
        assert_eq!(names.len(), 2);
        assert!(names.iter().any(|name| name.starts_with(FOLDER)));
        assert!(names.iter().any(|name| name.starts_with("SyntheticUK/")));
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .arg("inspect")
        .arg(&archive)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["importable"], false);
    assert_eq!(
        json["duplicate_files"],
        serde_json::to_value(&inspected.duplicate_files).unwrap()
    );
    let imported = import_snapshot(
        &archive,
        &temp.path().join("index"),
        &ImportOptions::new(EDITION, sha256(&archive).unwrap()),
    )
    .unwrap_err()
    .to_string();
    assert!(
        imported.contains("Expected one Snapshot file for sct2_Concept_"),
        "{imported}"
    );
    assert!(
        imported.contains("merged packages are not supported"),
        "{imported}"
    );
}

#[test]
fn inspect_reports_duplicate_package_metadata_without_choosing_a_release() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("merged.zip");
    fixture(&archive, false, true);
    let file = File::options()
        .read(true)
        .write(true)
        .open(&archive)
        .unwrap();
    let mut zip = ZipWriter::new_append(file).unwrap();
    zip.start_file(
        "SyntheticUK/release_package_information.json",
        SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(br#"{"effectiveTime":"20260801"}"#).unwrap();
    zip.finish().unwrap();
    let summary = inspect_archive(&archive).unwrap();
    assert!(!summary.importable);
    assert!(summary.effective_time.is_empty());
    assert!(summary.edition_uris.is_empty());
    let (_, names) = summary
        .duplicate_files
        .iter()
        .find(|(kind, _)| kind == "package metadata")
        .unwrap();
    assert_eq!(
        names,
        &[
            format!("{FOLDER}/release_package_information.json"),
            "SyntheticUK/release_package_information.json".into(),
        ]
    );
}

#[test]
fn extension_only_package_names_all_missing_dependency_modules() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("extension.zip");
    fixture(&archive, true, false);
    let destination = temp.path().join("index");
    let error = import_snapshot(
        &archive,
        &destination,
        &ImportOptions::new(EXTENSION, sha256(&archive).unwrap()),
    )
    .unwrap_err()
    .to_string();
    for text in [
        CORE.to_string(),
        "2000003".into(),
        "not self-contained".into(),
        "UK Drug Extension needs the editions it depends on".into(),
    ] {
        assert!(error.contains(&text), "{error}");
    }
    assert!(!destination.exists());
}
