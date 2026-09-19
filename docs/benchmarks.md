# Benchmark protocol

## Comparisons

Run this engine and Snowstorm on exactly the same RF2 archive. Snowstorm Lite is the first baseline because it has fewer infrastructure requirements. Full Snowstorm is the comparison for groups, cardinality and other features Lite does not support. Keep `sct` as an optional third baseline.

Freeze the archive SHA-256, edition URI, module versions, inferred view, active defaults, language configuration, engine commits and container image digests. Verify full result equality before including a query in a speed comparison. Record unsupported queries, wrong results, timeouts and import failures separately.

## Workload

Start with [queries.json](../validation/queries.json). Expand it before making performance claims: all hierarchy operators, small and broad expansions, unions, intersections, exclusions, actual concept refsets present in this release, selective/unselective attributes, same-group constraints, zero/multiple cardinality, concrete values and filters. Include empty results and pathological nesting. Choose release-specific cases after inventory and preserve their definitions.

The language-refset expression in the seed suite deliberately probes non-concept referenced components. It is not a substitute for a representative concept-refset benchmark. The eight `probe` cases are a small connection and correctness sample, not a representative workload.

Measure count-only, first-page and fully enumerated results independently. Use the same requested output in each end-to-end comparison. A bitmap count is not comparable to a server serialising every concept and label.

## Measurements

1. Measure import wall time, peak resident memory, total written bytes and final index size. Report the release's component counts.
2. Measure startup to readiness with a prebuilt index. Separate a cold process from a cold operating-system file cache.
3. Measure parse, plan, evaluate and enumerate within Rust. Do not compare these timings directly with HTTP round trips.
4. Measure local HTTP end to end once a minimal benchmark adapter exists. Keep adapter code outside the core. Use persistent connections and identical pagination and label settings.
5. Warm each query, randomise query order, and run at least five independent batches. Start with 1,000 observations per class for p95/p99 reporting, then check stability. Store raw samples, timeouts and errors.
6. Run concurrency 1, 4 and 16 separately. Record throughput alongside latency, CPU and memory. Do not use an unbounded cache to win repeated-query tests.

Use the same Linux host/runtime, CPU allocation and memory accounting for every implementation. Record CPU model, OS, toolchains, Docker/WSL versions, JVM options and resource limits. Include Elasticsearch when measuring full Snowstorm. Record process RSS, container memory and file cache separately; mmap pages still consume memory.

Use two resource comparisons: equal-resource performance and the minimum allocation at which each implementation completes the workload. Keep offline import allocation separate from serving allocation. If Snowstorm needs a larger serving allocation, state that rather than quietly changing limits.

Do not benchmark OneLondon's shared remote server. Its network latency and shared load are unsuitable, and repeated load adds no useful correctness evidence.

## Initial baseline attempt

The preparation run uses Snowstorm Lite 2.7.0, pinned to image digest `sha256:ff167ec28682ac47c5829c528354bbcd15cfd0aafb6d3d118d3945c8a35f1390`. It imports the verified UK Monolith with two CPUs, a 6 GiB container cap and a 4 GiB Java maximum heap. It listens only on `127.0.0.1:18080`.

Any initial HTTP timings are smoke measurements. They precede a Rust implementation and cannot establish a speedup. Keep their status and results in `baseline-status.md` after the import attempt finishes.
