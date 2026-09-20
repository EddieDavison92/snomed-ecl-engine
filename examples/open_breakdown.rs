//! Split the cost of opening a store into checksum, decode and validation.
//!
//! `NumericStore::open` does all three. `validate` is public and idempotent, so
//! timing it on an already-open store gives its share, and timing `sha256` over
//! the core section gives the checksum's. Decode is what is left.
use anyhow::{ensure, Result};
use snomed_ecl_engine::store::{sha256, Manifest, NumericStore};
use std::path::Path;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 1, "Usage: open_breakdown STORE");
    let path = Path::new(&args[0]);
    let manifest = Manifest::read(path)?;

    // Three rounds; the filesystem cache is deliberately left warm, so these
    // are process-start costs rather than cold-disk costs.
    for round in 0..3 {
        let start = Instant::now();
        let store = NumericStore::open(path)?;
        let open_ms = start.elapsed().as_secs_f64() * 1000.0;

        let start = Instant::now();
        store.validate()?;
        let validate_ms = start.elapsed().as_secs_f64() * 1000.0;

        let checksum_ms = if path.is_dir() {
            let start = Instant::now();
            let digest = sha256(&path.join("core.bin"))?;
            ensure!(digest == manifest.core_sha256, "Core checksum differs");
            start.elapsed().as_secs_f64() * 1000.0
        } else {
            f64::NAN
        };

        println!(
            "{}",
            serde_json::json!({
                "round": round,
                "layout": if path.is_dir() { "directory" } else { "container" },
                "open_ms": open_ms,
                "validate_ms": validate_ms,
                "core_checksum_ms": checksum_ms,
                "read_and_decode_ms": open_ms - validate_ms - if checksum_ms.is_nan() { 0.0 } else { checksum_ms },
                "concepts": store.ids.len(),
                "hierarchy_edges": store.parents.values.len(),
                "attribute_rows": store.attributes.rows.len(),
                "core_bytes": manifest.core_bytes,
            })
        );
    }
    Ok(())
}
