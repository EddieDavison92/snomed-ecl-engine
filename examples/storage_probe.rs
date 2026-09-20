use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use snomed_ecl_engine::store::{Manifest, NumericStore};
use std::fs::File;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 2, "Usage: storage_probe STORE BASELINE_JSON");
    let directory = Path::new(&args[0]);
    let manifest = Manifest::read(directory)?;
    let baseline: serde_json::Value = serde_json::from_reader(File::open(&args[1])?)?;
    ensure!(
        baseline["edition"] == manifest.edition,
        "Baseline edition differs"
    );
    let start = Instant::now();
    let store = NumericStore::open(directory)?;
    let load_seconds = start.elapsed().as_secs_f64();
    let mut results = Vec::new();
    for (id, ancestors, direct, include_self) in [
        ("descendants", false, false, false),
        ("descendants-or-self", false, false, true),
        ("ancestors", true, false, false),
        ("children", false, true, false),
        ("parents", true, true, false),
    ] {
        let expected = baseline["results"]
            .as_array()
            .context("Missing baseline results")?
            .iter()
            .find(|r| r["id"] == id)
            .context("Missing hierarchy probe")?;
        ensure!(expected["complete"] == true, "Incomplete baseline");
        let codes = store.hierarchy(195967001, ancestors, direct, include_self);
        let mut digest = Sha256::new();
        for code in &codes {
            digest.update(format!("{code}\n"));
        }
        let digest = format!("{:x}", digest.finalize());
        ensure!(
            expected["sha256"] == digest && expected["total"] == codes.len(),
            "Result differs for {id}"
        );
        let mut samples = Vec::new();
        for _ in 0..100 {
            let start = Instant::now();
            black_box(store.hierarchy(black_box(195967001), ancestors, direct, include_self));
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        results.push(serde_json::json!({"id": id, "total": codes.len(), "sha256": digest,
            "matches": true, "iterations": samples.len(), "warm_p50_ms": samples[49], "warm_p95_ms": samples[94]}));
    }
    let linux_memory: Vec<_> = std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        .map(str::to_owned)
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "edition": manifest.edition, "archive_sha256": manifest.archive_sha256,
            "core_bytes": manifest.core_bytes, "display_bytes": manifest.display_bytes,
            "load_including_checksum_and_validation_seconds": load_seconds,
            "linux_process_memory": linux_memory, "results": results,
            "scope": "Five hierarchy probes only. Warm enumeration includes sorting and result allocation, excludes loading and display lookup. Process memory excludes container overhead and filesystem page cache."
        }))?
    );
    Ok(())
}
