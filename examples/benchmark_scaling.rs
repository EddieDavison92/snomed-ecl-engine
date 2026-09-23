//! Measure concurrent requests against one shared index, without a transport layer.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snomed_ecl_engine::{
    ecl, eval,
    store::{sha256, Manifest, NumericStore},
};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Deserialize)]
struct Case {
    id: String,
    category: String,
    ecl: String,
}
#[derive(Deserialize)]
struct Corpus {
    archive_sha256: String,
    cases: Vec<Case>,
}
struct Expected {
    status: String,
    total: usize,
    sha256: String,
}
/// A baseline row. Unsupported or failed rows in a report carry no digest.
#[derive(Deserialize)]
struct Row {
    id: String,
    status: String,
    total: Option<usize>,
    sha256: Option<String>,
}
#[derive(Deserialize)]
struct Baseline {
    corpus_sha256: String,
    container_sha256: String,
    edition: String,
    archive_sha256: String,
    /// A corpus report names these `results`; each row carries the same fields.
    #[serde(alias = "results")]
    result_digests: Vec<Row>,
}
#[derive(Serialize)]
struct Measurement {
    case: usize,
    request_ms: f64,
    eval_ms: f64,
}
#[derive(Serialize)]
struct Round {
    wall_ms: f64,
    measurements: Vec<Measurement>,
}

fn run(
    store: &NumericStore,
    cases: &[Case],
    expected: &BTreeMap<String, Expected>,
    order: &[usize],
    workers: usize,
    verify: bool,
) -> Result<Vec<Measurement>> {
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let next = &next;
                scope.spawn(move || -> Result<Vec<Measurement>> {
                    let mut measurements = Vec::new();
                    while let Some(&index) = order.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let case = &cases[index];
                        let start = Instant::now();
                        let expression =
                            ecl::parse(&case.ecl).with_context(|| format!("Parse {}", case.id))?;
                        let eval_start = Instant::now();
                        let result = eval::evaluate_result(store, &expression)
                            .with_context(|| format!("Evaluate {}", case.id))?;
                        let eval_ms = eval_start.elapsed().as_secs_f64() * 1000.0;
                        let request_ms = start.elapsed().as_secs_f64() * 1000.0;
                        let reference = &expected[&case.id];
                        ensure!(
                            result.len() == reference.total,
                            "Changed count: {}",
                            case.id
                        );
                        if verify {
                            let eval::QueryResult::Concepts(ordinals) = result else {
                                anyhow::bail!("Expected concept result: {}", case.id);
                            };
                            let mut codes: Vec<_> = ordinals
                                .iter()
                                .map(|&ordinal| store.ids[ordinal as usize])
                                .collect();
                            codes.sort_unstable();
                            ensure!(codes.windows(2).all(|w| w[0] != w[1]), "Duplicate codes");
                            let mut hash = Sha256::new();
                            for code in codes {
                                writeln!(hash, "{code}")?;
                            }
                            ensure!(
                                format!("{:x}", hash.finalize()) == reference.sha256,
                                "Changed complete set: {}",
                                case.id
                            );
                        }
                        measurements.push(Measurement {
                            case: index,
                            request_ms,
                            eval_ms,
                        });
                    }
                    Ok(measurements)
                })
            })
            .collect();
        let mut measurements = Vec::with_capacity(cases.len());
        for handle in handles {
            measurements.extend(handle.join().expect("Benchmark worker panicked")?);
        }
        measurements.sort_unstable_by_key(|m| m.case);
        ensure!(
            measurements.len() == cases.len()
                && measurements.iter().enumerate().all(|(i, m)| i == m.case),
            "Missing or repeated cases"
        );
        Ok(measurements)
    })
}

fn shuffled_order(count: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<_> = (0..count).collect();
    let mut state = seed;
    for index in (1..count).rev() {
        // Fixed xorshift schedule, independent of library versions and worker count.
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        order.swap(index, (state % (index as u64 + 1)) as usize);
    }
    order
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 5,
        "Usage: benchmark_scaling STORE CORPUS BASELINE WORKERS SAMPLES"
    );
    let workers: usize = args[3].parse()?;
    let samples: usize = args[4].parse()?;
    ensure!(
        workers > 0 && samples > 0,
        "Workers and samples must be positive"
    );
    let baseline: Baseline = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    ensure!(
        sha256(Path::new(&args[1]))? == baseline.corpus_sha256,
        "Changed corpus"
    );
    ensure!(
        sha256(Path::new(&args[0]))? == baseline.container_sha256,
        "Changed index"
    );
    let corpus: Corpus = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let manifest = Manifest::read(Path::new(&args[0]))?;
    ensure!(manifest.edition == baseline.edition, "Changed edition");
    ensure!(manifest.supplements.is_empty(), "Unexpected supplement");
    ensure!(
        corpus.archive_sha256 == baseline.archive_sha256
            && manifest.archive_sha256 == baseline.archive_sha256,
        "Changed RF2 archive"
    );
    let expected_count = baseline.result_digests.len();
    // Every case must have a whole recorded set, or its answers cannot be checked.
    let expected: BTreeMap<_, _> = baseline
        .result_digests
        .into_iter()
        .map(|row| match (row.total, row.sha256) {
            (Some(total), Some(sha256)) => Ok((
                row.id,
                Expected {
                    status: row.status,
                    total,
                    sha256,
                },
            )),
            _ => anyhow::bail!("Baseline has no complete result for {}", row.id),
        })
        .collect::<Result<_>>()?;
    ensure!(
        expected.len() == expected_count && expected.len() == corpus.cases.len(),
        "Case count differs"
    );
    ensure!(!corpus.cases.is_empty(), "Empty corpus");
    let unique: HashSet<_> = corpus.cases.iter().map(|case| &case.id).collect();
    ensure!(unique.len() == corpus.cases.len(), "Duplicate case IDs");
    for case in &corpus.cases {
        ensure!(
            expected
                .get(&case.id)
                .is_some_and(|r| r.status == "evaluated"),
            "Missing successful baseline: {}",
            case.id
        );
    }
    let start = Instant::now();
    let store = NumericStore::open(Path::new(&args[0]))?;
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    eprintln!("Store opened in {open_ms:.3} ms");
    run(
        &store,
        &corpus.cases,
        &expected,
        &shuffled_order(corpus.cases.len(), 20260825),
        workers,
        true,
    )?;
    eprintln!(
        "Verified {} complete sets with {workers} workers",
        corpus.cases.len()
    );
    let mut rounds = Vec::new();
    for iteration in 0..samples {
        let order = shuffled_order(corpus.cases.len(), 20260826 + iteration as u64);
        let start = Instant::now();
        let measurements = run(&store, &corpus.cases, &expected, &order, workers, false)?;
        let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
        eprintln!("Batch {}: {wall_ms:.3} ms", iteration + 1);
        rounds.push(Round {
            wall_ms,
            measurements,
        });
    }
    let read_cgroup = |name: &str, fallback: &str| {
        std::fs::read_to_string(format!("/sys/fs/cgroup/{name}"))
            .or_else(|_| std::fs::read_to_string(format!("/sys/fs/cgroup/{fallback}")))
            .ok()
    };
    let memory_peak = read_cgroup("memory.peak", "memory/memory.max_usage_in_bytes")
        .and_then(|s| s.trim().parse::<u64>().ok());
    let cpu_max = std::fs::read_to_string("/sys/fs/cgroup/cpu.max")
        .ok()
        .or_else(|| {
            let quota = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us").ok()?;
            let period = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us").ok()?;
            Some(format!("{} {}", quota.trim(), period.trim()))
        });
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "workers": workers, "samples": samples, "store_open_ms": open_ms,
            "verified_complete_sets": corpus.cases.len(), "rounds": rounds,
            "cases": corpus.cases.iter().map(|c| serde_json::json!({"id": c.id, "category": c.category})).collect::<Vec<_>>(),
            "memory_peak_bytes": memory_peak,
            "cpu_stat": read_cgroup("cpu.stat", "cpu/cpu.stat"),
            "memory_events": read_cgroup("memory.events", "memory/memory.failcnt"),
            "cpu_max": cpu_max, "memory_max": read_cgroup("memory.max", "memory/memory.limit_in_bytes"),
            "scope": "One shared NumericStore. Dynamic assignment, one evaluation per worker at a time. No result cache. Each timed request parses ECL and materialises its complete result. Request latency excludes queueing, transport, code serialisation and result destruction. Batch time includes dispatch, thread start/join and destruction. Complete sets checked before timings. Filesystem caches retained; index hash read before open. Peak includes verification, OS cache and measurement storage."
        }))?
    );
    Ok(())
}
