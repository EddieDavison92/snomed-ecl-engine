#![cfg(feature = "download")]

use std::process::Command;

#[test]
fn updates_failure_redacts_the_environment_key_at_the_output_boundary() {
    let key = "SYNTHETIC-SENTINEL-KEY-123";
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join(format!("{key}.ecl"));
    let output = Command::new(env!("CARGO_BIN_EXE_snomed-ecl-engine"))
        .args(["updates", "--index"])
        .arg(&missing)
        .env("TRUD_API_KEY", key)
        .env("SNOMED_ECL_HOME", directory.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.contains(key));
    assert!(!stderr.contains(key));
    assert!(stderr.contains("Cannot read index"));
    assert!(stderr.contains("***.ecl"));
}

#[test]
fn updates_handles_help_and_unknown_flags_before_reading_the_key() {
    let binary = env!("CARGO_BIN_EXE_snomed-ecl-engine");
    for args in [
        vec!["updates", "uk-monolith", "--help"],
        vec!["updates", "uk-monolith", "-h"],
        vec!["updates", "--index", "one.ecl", "-h"],
        vec!["updates", "--index", "a.ecl", "uk-monolith", "--help"],
    ] {
        let output = Command::new(binary)
            .args(args)
            .env_remove("TRUD_API_KEY")
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Usage: updates [ITEM ...] [--index PATH]"));
        assert!(stdout.contains("Each --index takes one path; repeat for more paths"));
        assert!(output.stderr.is_empty());
    }
    let output = Command::new(binary)
        .args(["updates", "--unknown"])
        .env_remove("TRUD_API_KEY")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Unknown updates option --unknown"));
    assert!(!stderr.contains("Set TRUD_API_KEY"));
}
