# Refinements and the 1,000-expression corpus

The evaluator now supports nested attribute names and concept values, attribute and group cardinalities, grouped conjunction/disjunction, ungrouped reverse attributes, concept-valued dotted chains, top/bottom and exact concrete decimal comparisons. Literal concrete strings, string sets and Booleans have synthetic tests. The core store format remains unchanged for the imported UK release.

Group zero stays outside relationship groups. An inequality requires a matching attribute with a value outside the constraint; absence alone does not satisfy it. Reverse cardinality counts distinct source concepts. Decimal comparison preserves precision beyond machine integer and floating-point ranges.

The evaluator consumes the published inferred distribution form. It does not remove redundant relationships or perform classification. Grouped reverse queries and concrete dotted projections currently fail explicitly. Membership, filters, history, alternate identifiers, typed member projections and remaining syntax/semantic checks are still mandatory.

## Validation

`tests/refinements.rs` covers cross-group false matches, group zero, zero and bounded cardinality, reverse source identity, absence versus inequality, nested expressions, exact decimals, strings, Booleans and malformed syntax. Existing independent hierarchy/set tests still pass with and without the importer feature.

All 20 small probes against OneLondon's Ontoserver matched complete code sets for the pinned UK edition. See [probe results](../validation/ontoserver-refinements.json). This is targeted correctness evidence, not proof of all refinement semantics.

The official example classification parses 63 of 121 files. The other 58 return unsupported errors. No valid example fails with an unexpected syntax error.

## Corpus and timings

[The corpus](../validation/ecl-1000.json) contains exactly 1,000 unique expressions, with 40 in each of 25 categories. `scripts/generate_corpus.py` samples active inferred rows from the checksum-pinned archive using seed `20260826`, with distinct source concepts. It was generated with Python 3.11.9. Query expressions are tracked; RF2 rows and indexes remain ignored.

The current engine evaluates 800 cases. The 200 cases for membership, concept filters, description filters, history and member projections remain explicit coverage gaps. Generated expressions are not a clinical workload distribution. Among the 800 evaluated cases, 117 return no concepts, 368 return one, and the largest returns 65,043.

`scripts/benchmark_corpus.py` enumerates each supported result once and records a canonical full-set digest. It then runs five seeded shuffled batches in one loaded process, with no result cache. Count requests still evaluate and materialise the complete ordinal set. The local full Snowstorm option checks all returned IDs before timing Rust cases; mismatches are excluded from successful timings and remain in the report.

```sh
python scripts/benchmark_corpus.py --output data/validation/ecl-1000.json
python scripts/benchmark_corpus.py --store-volume snomed-ecl-runtime-index --output data/validation/ecl-1000-linux.json
```

The optional volume must contain `core.bin` and `manifest.json` for the pinned edition. The [recorded diagnostic run](refinement-results.json) includes raw samples, digests, unsupported cases and resource context. Other runs stay in ignored `data/validation/` until reviewed.

The first five-batch Linux-volume run evaluated all 800 supported cases in a median 1.64 seconds of engine time, or 2.57 seconds including sequential Docker JSONL requests. The host was also importing full Snowstorm. These are diagnostic timings, with no full Snowstorm speed comparison yet. Category percentiles describe this corpus and sample count, not a production latency guarantee.

See [serverless measurements](serverless.md) for binary size, startup and memory. The small core measurements must be repeated after full ECL is implemented.

## Local full Snowstorm comparison

The [completed run](full-snowstorm.md) records 719 full-set matches, the independently checked inequality discrepancy, unsupported categories and five timing batches.

The current local import uses Snowstorm 11.0.0 and Elasticsearch 8.19.8, each capped at four CPUs and 6 GiB. Snowstorm imports the checksum-pinned archive into an initially empty `MAIN`, with the UK Monolith module URI. Its image is `snomedinternational/snowstorm@sha256:fa9cce11ce3f25bdc98c67d8f93ccd6c96665fbeaab7cb42f273f0118fbc222e`. Elasticsearch uses `docker.elastic.co/elasticsearch/elasticsearch@sha256:1b6a877f18352510860ee065f01472bd37d33ac5eb1d943e0b9ed366b149638c`.

`scripts/Watch-FullBaseline.ps1` waits for the import, runs the comparison, then stops both containers. It writes status to `data/full-baseline-status.json` and comparison evidence to `data/validation/ecl-1000-full-snowstorm.json`. The import wait is bounded to 90 minutes; the comparison has a one-hour budget and stops after five unexpected request errors. A successful import is required, along with an advertised matching FHIR edition and release-count checks. A pending run is not validation evidence.

The comparison requests IDs only, pages with `searchAfter`, and checks every code. Warm Snowstorm count requests return a total plus at most one ID and include HTTP overhead and its default caches. Rust has no result cache. Report these differences with any timing comparison. Resource snapshots cover the running container's lifetime; separate the original import from a serving-only restart when interpreting memory.

For a serving-only restart, use a prior report as completed-import evidence because Snowstorm's in-memory import job has disappeared. The runner checks the edition, archive checksum, complete MAIN branch response and live release sentinels before comparison:

```sh
python scripts/benchmark_corpus.py --store-volume snomed-ecl-runtime-index --snowstorm http://127.0.0.1:18082 --import-report data/validation/ecl-1000-full-snowstorm.json --output data/validation/ecl-1000-full-snowstorm-rerun.json
```

The output path must be new. Restart the saved Elasticsearch volume and start Snowstorm without `--import`; retain the original container for its import logs. Stop both services after the run. Do not use a report for a different edition or a branch changed since that report.

The pinned Snowstorm parser rejects the first `!` in ECL 2.3 top/bottom expressions. The runner recognises only that specific HTTP 400 response as `snowstorm-unsupported`, retains the response body and continues. These cases receive Rust-only timings. Other request errors and result mismatches remain failures. Paired timings include only expressions whose complete code sets match. Rust request timings include Docker JSONL transport as a separate measure from engine evaluation.
