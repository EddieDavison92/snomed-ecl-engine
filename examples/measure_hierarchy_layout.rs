//! Measure exact descendant interval counts before changing the stored ordinal order.
use anyhow::{ensure, Result};
use serde_json::json;
use snomed_ecl_engine::store::{Manifest, NumericStore};
use std::{path::Path, time::Instant};

fn runs(values: &[u32]) -> usize {
    usize::from(!values.is_empty()) + values.windows(2).filter(|w| w[1] != w[0] + 1).count()
}

fn permutation(store: &NumericStore) -> (Vec<u32>, usize) {
    let count = store.ids.len();
    let mut visited = vec![false; count];
    let mut order = Vec::with_capacity(count);
    let mut stack = Vec::new();
    let root = store.ordinal(138875005);
    let mut root_reached = 0;
    for seed in root.into_iter().chain(0..count as u32) {
        if visited[seed as usize] {
            continue;
        }
        stack.push(seed);
        while let Some(node) = stack.pop() {
            if std::mem::replace(&mut visited[node as usize], true) {
                continue;
            }
            order.push(node);
            stack.extend(store.children.get(node).iter().rev().copied());
        }
        if Some(seed) == root {
            root_reached = order.len();
        }
    }
    // Keep inactive concepts without assuming that root reachability equals active status.
    let mut permutation = vec![0; count];
    let mut assigned = 0;
    for active in [true, false] {
        for &node in &order {
            if (store.flags[node as usize] & 1 != 0) == active {
                permutation[node as usize] = assigned;
                assigned += 1;
            }
        }
    }
    (permutation, root_reached)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() == 1, "Usage: measure_hierarchy_layout INDEX");
    let path = Path::new(&args[0]);
    let manifest = Manifest::read(path)?;
    let store = NumericStore::open(path)?;
    let started = Instant::now();
    let count = store.ids.len();
    ensure!(
        count > 0 && count < u32::MAX as usize,
        "Unsupported concept count"
    );
    let (permutation, root_reached) = permutation(&store);
    let mut sorted = permutation.clone();
    sorted.sort_unstable();
    ensure!(
        sorted.iter().copied().eq(0..count as u32),
        "Invalid permutation"
    );
    drop(sorted);
    let mut marks = vec![0u32; count];
    let mut stack = Vec::new();
    let mut found = Vec::new();
    let mut distribution = Vec::with_capacity(count);
    let mut pairs = 0u64;
    let mut original_runs = 0u64;
    let mut reordered_runs = 0u64;
    let mut worst = (0u32, 0usize);
    let mut hybrid = [(8, 0u64, 0usize), (32, 0, 0), (64, 0, 0), (256, 0, 0)];
    let mut focus = Vec::new();
    for seed in 0..count {
        let epoch = seed as u32 + 1;
        found.clear();
        stack.push(seed as u32);
        marks[seed] = epoch;
        while let Some(node) = stack.pop() {
            found.push(node);
            for &child in store.children.get(node) {
                if marks[child as usize] != epoch {
                    marks[child as usize] = epoch;
                    stack.push(child);
                }
            }
        }
        pairs += found.len() as u64;
        found.sort_unstable();
        let before = runs(&found);
        original_runs += before as u64;
        for node in &mut found {
            *node = permutation[*node as usize];
        }
        found.sort_unstable();
        let after = runs(&found);
        reordered_runs += after as u64;
        distribution.push(after as u32);
        if after > worst.1 {
            worst = (seed as u32, after);
        }
        for (limit, total, covered) in &mut hybrid {
            if after <= *limit {
                *total += after as u64;
                *covered += 1;
            }
        }
        if [
            138875005, 404684003, 64572001, 195967001, 373873005, 123037004, 71388002, 73211009,
            763158003,
        ]
        .contains(&store.ids[seed])
        {
            focus.push(json!({"concept":store.ids[seed].to_string(),
                "descendants_or_self":found.len(), "sctid_order_runs":before, "dfs_order_runs":after}));
        }
    }
    distribution.sort_unstable();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "edition":manifest.edition, "archive_sha256":manifest.archive_sha256,
            "core_sha256":manifest.core_sha256, "concepts":count,
            "active_concepts":manifest.active_concept_count, "root_reached":root_reached,
            "hierarchy_edges":store.children.values.len(), "descendant_or_self_pairs":pairs,
            "sctid_order_runs":original_runs, "dfs_order_runs":reordered_runs,
            "median_runs":distribution[count/2], "p95_runs":distribution[count*95/100],
            "p99_runs":distribution[count*99/100],
            "worst":{"concept":store.ids[worst.0 as usize].to_string(), "runs":worst.1},
            "interval_pairs_bytes":reordered_runs*8, "offsets_bytes":(count+1)*4,
            "permutation_bytes":count*4, "focus":focus,
            "hybrid":hybrid.map(|(limit,total,covered)|json!({"max_stored_runs":limit,
                "covered_concepts":covered,"interval_pairs_bytes":total*8,"fallback_concepts":count-covered})),
            "layout_scan_seconds":started.elapsed().as_secs_f64(),
            "scope":"Exact counts for all descendant-or-self sets, including inactive concepts. DFS forest with root first, ascending SCTID children, stable active/inactive partition. Byte counts describe uncompressed u32 interval pairs, offsets and one permutation, not a complete index. No runtime evaluator or stored file changes. Timing excludes opening the validated numeric store."
        }))?
    );
    Ok(())
}
