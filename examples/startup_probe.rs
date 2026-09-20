//! Separate manifest I/O from checksum, decode and graph validation costs.
use anyhow::{ensure, Result};
use snomed_ecl_engine::store::{sha256, Manifest, NumericStore};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Instant;

struct Counted<R> {
    inner: R,
    reads: usize,
}
impl<R: Read> Read for Counted<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.reads += 1;
        self.inner.read(bytes)
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 1, "Usage: startup_probe STORE");
    let directory = Path::new(&args[0]);
    let path = directory.join("manifest.json");
    let mut samples = Vec::new();
    let mut reference = None;
    for round in 0..3 {
        // Alternate order. Timings include File::open; filesystem cache is not dropped.
        for buffered in if round % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let start = Instant::now();
            let mut input = Counted {
                inner: File::open(&path)?,
                reads: 0,
            };
            let manifest: Manifest = if buffered {
                serde_json::from_reader(BufReader::new(&mut input))?
            } else {
                serde_json::from_reader(&mut input)?
            };
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            let value = serde_json::to_value(&manifest)?;
            if let Some(expected) = &reference {
                ensure!(expected == &value, "Manifest differs");
            }
            reference = Some(value);
            let sample = serde_json::json!({"round": round, "buffered": buffered,
                "read_calls": input.reads, "elapsed_ms": elapsed_ms});
            eprintln!("{sample}");
            samples.push(sample);
        }
    }
    let start = Instant::now();
    let store = NumericStore::open(directory)?;
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "manifest_bytes": path.metadata()?.len(), "manifest_sha256": sha256(&path)?,
            "manifest_samples": samples, "store_open_ms": open_ms,
            "concept_count": store.ids.len(), "manifests_equal": true,
            "scope": "Manifest parse and store open, including existing checksums and structural validation. Filesystem caches are not dropped. Excludes Docker startup and lazy semantic indexes."
        }))?
    );
    Ok(())
}
