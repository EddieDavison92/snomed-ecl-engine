//! Classify pinned official syntax examples without copying their contents into this repository.
use anyhow::{ensure, Result};
use snomed_ecl_engine::ecl::{parse, ParseErrorKind};
use std::path::Path;

fn visit(root: &Path, directory: &Path, results: &mut Vec<serde_json::Value>) -> Result<()> {
    let mut entries = std::fs::read_dir(directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            visit(root, &path, results)?;
        } else if path.extension().is_some_and(|e| e == "txt") {
            let query = std::fs::read_to_string(&path)?;
            let result = parse(query.trim_start_matches('\u{feff}'));
            results.push(serde_json::json!({
                "file": path.strip_prefix(root)?.to_string_lossy().replace('\\', "/"),
                "status": match &result { Ok(_) => "parsed", Err(e) if e.kind == ParseErrorKind::Unsupported => "unsupported", Err(e) if e.kind == ParseErrorKind::Semantic => "semantic", Err(_) => "unexpected_error" },
                "error": result.err().map(|e| e.to_string()),
            }));
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 1,
        "Usage: check_ecl_examples EXAMPLES_DIRECTORY"
    );
    let root = Path::new(&args[0]);
    let mut results = Vec::new();
    visit(root, root, &mut results)?;
    ensure!(!results.is_empty(), "No official examples found");
    let unexpected = results
        .iter()
        .filter(|r| r["status"] == "unexpected_error")
        .count();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "reference_commit": "b0e07105ae395821bcc953f3d6084b57dc7bef2c",
            "parsed": results.iter().filter(|r| r["status"] == "parsed").count(),
            "unsupported": results.iter().filter(|r| r["status"] == "unsupported").count(),
            "unexpected_errors": unexpected, "results": results,
            "scope": "Syntax classification only. Unsupported examples remain required; parsing does not establish semantic correctness."
        }))?
    );
    ensure!(
        unexpected == 0,
        "Unexpected errors in official valid examples"
    );
    Ok(())
}
