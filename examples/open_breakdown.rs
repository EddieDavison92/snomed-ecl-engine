//! Report what opening a store costs, and what a full verification adds.
//!
//! `NumericStore::open` reads the core, decodes it and checks that every stored
//! index is in range. It deliberately does not hash the section or re-run the
//! semantic checks in `validate`; `verify` does both on demand. All three are
//! timed here so the split is visible.
use anyhow::{ensure, Result};
use snomed_ecl_engine::store::{sha256, Manifest, NumericStore};
use std::path::Path;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 1, "Usage: open_breakdown STORE");
    let path = Path::new(&args[0]);
    let manifest = Manifest::read(path)?;

    // Three rounds with the filesystem cache left warm, so these are
    // process-start costs rather than cold-disk costs.
    for round in 0..3 {
        let start = Instant::now();
        let store = NumericStore::open(path)?;
        let open_ms = start.elapsed().as_secs_f64() * 1000.0;

        let start = Instant::now();
        store.validate()?;
        let validate_ms = start.elapsed().as_secs_f64() * 1000.0;

        // What `verify` pays to hash the core, which opening no longer does.
        // Only a directory store exposes the core as its own file.
        let checksum_ms = if path.is_dir() {
            let start = Instant::now();
            let digest = sha256(&path.join("core.bin"))?;
            ensure!(digest == manifest.core_sha256, "Core checksum differs");
            Some(start.elapsed().as_secs_f64() * 1000.0)
        } else {
            None
        };

        println!(
            "{}",
            serde_json::json!({
                "round": round,
                "layout": if path.is_dir() { "directory" } else { "container" },
                "open_ms": open_ms,
                "core_checksum_ms": checksum_ms,
                "full_validate_ms": validate_ms,
                "concepts": store.ids.len(),
                "hierarchy_edges": store.parents.values.len(),
                "attribute_rows": store.attributes.rows.len(),
                "core_bytes": manifest.core_bytes,
            })
        );
    }
    Ok(())
}
