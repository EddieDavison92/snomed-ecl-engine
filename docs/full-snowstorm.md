# Full Snowstorm comparison

On 20 September 2026, all 1,000 corpus expressions were processed against the saved UK Monolith index. The [raw report](full-snowstorm-results.json) retains code-set digests, all timing samples, resource snapshots and failures. This is a diagnostic comparison on a shared Windows/Docker host, not an equal-resource or equal-transport benchmark.

## Correctness outcome

| Outcome | Expressions |
|---|---:|
| Complete Rust/Snowstorm code sets match | 719 |
| Concrete inequality disagreement, checked against OneLondon | 1 |
| Top/bottom rejected by Snowstorm's parser | 80 |
| ECL features still unsupported by Rust | 200 |

Rust evaluated 800 expressions. The runner timed 799, excluding the one mismatch. Only the 719 exact matches receive paired Snowstorm timings. Membership, concept filters, description filters, history and member projections remain required engine work. This corpus is not full ECL conformance evidence.

Snowstorm completed the original import in 4,360 seconds. This run used a serving-only restart with the saved Elasticsearch volume. The runner checked the prior completed-import report, unchanged MAIN branch, advertised edition, active concept count and clinical-finding descendant count. The edition is `http://snomed.info/sct/83821000000107/version/20260826`; archive and binary SHA-256 values are in the raw report.

## Timings

Five seeded, shuffled, sequential batches followed complete-set enumeration. These figures are for the same 719 matching expressions:

| Measure | Rust | Snowstorm |
|---|---:|---:|
| Median batch, including request transport | 2.163 s, Docker JSONL | 10.318 s, loopback HTTP |
| Median batch, engine evaluation only | 1.267 s | Not measured |
| Median individual request | 2.237 ms | 12.661 ms |
| Individual request p95 | 8.809 ms | 38.918 ms |

Snowstorm's request batch took about 4.8 times as long in this setup. This is an observed CLI-versus-HTTP result, not an isolated evaluator speedup. Rust uses one persistent process and pipe; Snowstorm requests use Python urllib without an explicit persistent connection pool. Rust has no result cache. Snowstorm uses its default caches and returns a total plus at most one ID. Rust count requests still evaluate and materialise the full ordinal set, returning only the count. Individual-request percentiles pool all five batches and use the nearest-rank method.

The five Snowstorm batch totals were 32.568, 9.974, 10.200, 11.477 and 10.318 seconds. The slower first batch remains in the report and median calculation. Result enumeration does not guarantee identical cache state for subsequent count requests. A local RF2 inspection also ran near the end of the measurement session. Treat these timings as diagnostic evidence; controlled transport, cache and concurrency comparisons remain open.

Rust's query-only executable is 756,080 bytes, or 356,103 bytes gzipped. Opening the cached-volume core took 0.457 seconds; container start plus the first request took 1.003 seconds. This is not a cold operating-system cache or a hosted serverless cold start.

## Resource context

Rust had one CPU and a 256 MiB memory limit. Snowstorm and Elasticsearch each had four CPUs and a 6 GiB limit, with swap disabled for all three. Snowstorm used a 1 GiB initial/4 GiB maximum Java heap; Elasticsearch used a 3 GiB heap. Images are pinned in the raw report. This run did not find minimum Snowstorm serving allocations.

Container-charged peaks were 193.7 MiB for Rust, 1.688 GiB for Snowstorm and 4.429 GiB for Elasticsearch. These include charged file cache and wrapper processes, not the whole Docker VM or Windows host. The service peaks were measured separately and should not be presented as a simultaneous combined peak. The serving processes were restarted after import.

The Elasticsearch data directory occupied 6,565,868,541 bytes at a sampled point during the run. Its broader descriptions, refsets and semantic indexes make it incomparable with the current incomplete Rust core's 90,263,179 bytes as a full-ECL storage claim.

## Concrete inequality discrepancy

The mismatching expression was:

```ecl
(<< 377442002) : 1142138002 != #10
```

Rust returned `377442002`; Snowstorm returned no concepts. The RF2 Snapshot has two active inferred values of attribute `1142138002` on that concept: `#20` in group 1 and `#10` in group 2. An inequality can match the `#20` relationship even when a different relationship equals `#10`. The ECL specification distinguishes this from prohibiting any equal value, which uses a zero cardinality. See [Exclusion and Not Equals](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/examples/6.5-exclusion-and-not-equals).

Version-pinned OneLondon Ontoserver 6.25.4 returned the same complete singleton set as Rust. The related equality and greater-than probes also returned that singleton. Their [validation record](../validation/ontoserver-concrete-inequality.json) contains the edition and digests. This evidence supports a Snowstorm concrete-inequality defect; it does not justify changing Rust to reproduce the omission. A synthetic two-value regression test covers existential inequality, equality, grouping and cardinality without copying release data into the fixture.

The raw report deliberately retains `mismatch`, and the benchmark runner exited with status 1. Its `exit_code` field records the Rust child process, which exited normally with status 0. All five timing batches completed. Both services were stopped afterwards and their indexes retained.
