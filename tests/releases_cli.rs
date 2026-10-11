use std::process::Command;

#[cfg(feature = "download")]
#[test]
fn releases_and_alias_redact_the_environment_key_without_deprecation_noise() {
    let key = "SYNTHETIC-SENTINEL-KEY-123";
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join(format!("{key}.ecl"));
    for output_flags in [vec![], vec!["--json"], vec!["--plain"]] {
        let mut outputs = Vec::new();
        for command in ["releases", "updates"] {
            let output = Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
                .args([command, "--index"])
                .arg(&missing)
                .args(["uk-monolith", "--apply", "--yes", "--exit-code"])
                .args(&output_flags)
                .env("TRUD_API_KEY", key)
                .env("SNOMED_ECL_HOME", directory.path())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1));
            let stdout = String::from_utf8(output.stdout).unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stdout.is_empty());
            assert!(!stderr.contains(key));
            assert!(stderr.contains("Cannot read index"));
            assert!(stderr.contains("***.ecl"));
            assert!(!stderr.contains("updates"));
            assert!(!stderr.contains("deprecated"));
            outputs.push((stdout, stderr));
        }
        assert_eq!(outputs[0], outputs[1]);
    }
}

#[cfg(feature = "download")]
#[test]
fn releases_and_alias_handle_help_and_unknown_flags_before_reading_the_key() {
    let binary = env!("CARGO_BIN_EXE_snomed-ecl-engine");
    for command in ["releases", "updates"] {
        for args in [
            vec![command, "uk-monolith", "--help"],
            vec![command, "uk-monolith", "-h"],
            vec![command, "--index", "one.ecl", "-h"],
            vec![command, "--index", "a.ecl", "uk-monolith", "--help"],
        ] {
            let output = Command::new(binary)
                .args(args)
                .env_remove("TRUD_API_KEY")
                .output()
                .unwrap();
            assert!(output.status.success());
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains("Usage: releases [ITEM ...] [--index PATH]"));
            assert!(stdout.contains("Each --index takes one path; repeat for more paths"));
            assert!(output.stderr.is_empty());
        }
        let output = Command::new(binary)
            .args([command, "--unknown"])
            .env_remove("TRUD_API_KEY")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("Unknown releases option --unknown"));
        assert!(!stderr.contains("Set TRUD_API_KEY"));
    }
}

#[test]
fn help_lists_releases_and_keeps_the_alias_hidden() {
    let binary = env!("CARGO_BIN_EXE_snomed-ecl-engine");
    let output = Command::new(binary).arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("  releases    Check indexed archives"));
    assert!(!stdout.contains("updates"));
    assert!(output.stderr.is_empty());

    for args in [
        ["releases", "--help"],
        ["updates", "--help"],
        ["help", "releases"],
        ["help", "updates"],
    ] {
        let output = Command::new(binary).args(args).output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Usage: releases [ITEM ...] [--index PATH]"));
        assert!(!stdout.contains("updates"));
        assert!(output.stderr.is_empty());
    }
    let output = Command::new(binary)
        .args(["self-update", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("`releases` checks terminology releases"));
    assert!(!stdout.contains("updates"));
    assert!(output.stderr.is_empty());
}

#[cfg(not(feature = "download"))]
#[test]
fn releases_and_alias_report_the_required_feature() {
    for command in ["releases", "updates"] {
        let output = Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
            .arg(command)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("Terminology release checks need the `download` feature"));
        assert!(!stderr.contains("updates"));
    }
}
