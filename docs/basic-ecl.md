# Basic ECL milestone

The library now parses and evaluates concept literals, wildcard, eight hierarchy operators, parentheses, conjunction, disjunction and exclusion. It accepts symbolic operators, their long-syntax names, case-insensitive Boolean keywords, comma conjunctions, comments and optional concept terms. Results are distinct concept IDs in numeric order.

Full ECL 2.3 remains mandatory. Refinements, refsets, filters, history, projections, alternate identifiers and top/bottom are still pending. Recognised unsupported constructs produce an explicit error before evaluation. The parser does not validate the entire grammar of an unsupported construct, so that error is not proof that the rest of the query is valid.

## Use the CLI

After the [store build](compact-store.md#build-and-run), run:

```sh
snomed-ecl-engine expand STORE '<< 195967001'
snomed-ecl-engine expand STORE '(<< 195967001) MINUS 195967001' --count
snomed-ecl-engine expand STORE '(<< 195967001) OR (<< 73211009)' --display
```

`--count` currently evaluates the complete ordinal set, then returns its length. It is not a separate count-optimised execution path. Display lookup happens after evaluation and reads only the requested labels. The numeric path works without `display.bin`.

For repeated queries, `batch STORE` opens the store once and accepts one JSON request per line on standard input:

```json
{"ecl":"<< 195967001","count_only":false}
```

Each response contains the edition, total, parse time, evaluation time and codes as decimal strings. Set `count_only` to true to omit codes. Query errors return an `error` field with no results; the process can accept the next request. This local protocol supports validation and benchmarks. It is not an HTTP service.

## Semantics and limits

The implementation follows the [pinned grammar](references.json) and [syntax specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/design/5-syntax-specification). Mixed Boolean operators require parentheses. Repeated exclusion also requires grouping. The evaluator does not apply SQL-style precedence.

Hierarchy operators accept a parenthesised expression as their input. For strict descendants of a set, an input concept can still appear in the result if it descends from another input concept. Traversal therefore tracks visited vertices separately from selected results.

Absent concept literals return empty sets. Inactive literals return themselves; hierarchy traverses active edges. Concept terms are annotations and do not change the identified concept. The parser checks identifier shape, not the Verhoeff checksum or whether the supplied term belongs to that concept.

The evaluator uses sorted `u32` ordinal vectors and linear set merges. A hierarchy traversal visits each reachable vertex once for the whole input set. It does not build every concept's transitive closure or use a query cache.

Default limits are 65,536 query bytes, 64 nested parentheses, 4,096 expression nodes, 100 million work units and eight million simultaneously reserved ordinal slots. A work unit accounts for expression visits, graph work and set scans; it is not a CPU-time unit. The two temporary Boolean arrays used by hierarchy traversal add at most two bytes per stored concept. The loaded store, parser and output buffers are outside the ordinal-slot budget. Batch requests have a 512 KiB encoded-line limit.

The library's `evaluate_with_limits` accepts an optional atomic cancellation flag. Cancellation is cooperative at evaluation checkpoints. Limits and cancellation return errors, never truncated results. Callers should retain a validated store and bound concurrent evaluations independently.

## Reproduce validation

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
python scripts/conformance_inventory.py --check
cargo run --locked --example check_ecl_examples -- references/snomed-expression-constraint-language/examples
python scripts/verify_basic_rf2.py --output data/validation/basic-ecl-rf2.json
```

The conformance inventory includes all 180 productions from each of the official brief and long grammars. `implemented` records development evidence for an individual production, not complete semantic conformance. `partial` and `pending` entries still require work. Lexical edge cases remain tracked separately from the successful official examples.

The synthetic evaluator tests compare against an independent per-seed graph search and `BTreeSet` operations. They cover overlapping inputs, inactive and absent concepts, Boolean expressions, empty results, malformed syntax, unsupported branches, limits and cancellation. CLI tests verify no partial output on a query error and continued batch operation afterwards.

Refresh the small OneLondon probes through the existing credential helper:

```powershell
./scripts/Test-Ontoserver.ps1 -QueryPath validation/basic-ecl-queries.json -Destination data/validation/basic-ecl
```

The tracked [baseline](../validation/ontoserver-basic-ecl.json) records 17 complete result sets at the pinned UK edition. Large queries are excluded from remote probing. Review results before replacing the tracked baseline. Empty sets use the SHA-256 digest of zero bytes.

## Run the local comparison

Build the release binary using the Docker command in the store guide. Start Snowstorm Lite against its existing same-release index, without `--load`:

```powershell
$index = 'type=bind,source=' + (Join-Path (Get-Location).Path 'data/snowstorm-lite') + ',target=/index'
docker run -d --name snomed-ecl-serving --cpus 1 --memory 2g --memory-swap 2g -e JAVA_TOOL_OPTIONS=-Xmx1400m -p 127.0.0.1:18081:8080 --mount $index snomedinternational/snowstorm-lite@sha256:ff167ec28682ac47c5829c528354bbcd15cfd0aafb6d3d118d3945c8a35f1390 --index.path=/index
python scripts/benchmark_ecl.py --samples 3 --output data/validation/basic-ecl-benchmark.json
docker stop snomed-ecl-serving
```

Wait for the server's readiness message before running the script. Choose a new output path for each run. The script uses a separate Rust container capped at one CPU and 256 MiB, with swap disabled, and removes it afterwards. It does not stop the Snowstorm container automatically.

The workload checks complete code sets before timing. It includes all eight hierarchy operators, Boolean operations, empty results, overlapping inputs, clinical findings, wildcard and broad exclusions. A preliminary count difference already proves failure, so those cases skip full enumeration and timing. For equal totals, complete-set comparison remains mandatory. The first complete expansion warms each query; timed iterations alternate engine order. Small cases use three timed samples by default; broad cases use one. Change `--large-samples` for longer runs.

Rust evaluation time excludes parsing and output. The transport measurements include JSON processing through Docker pipes for Rust and paginated HTTP for Snowstorm Lite. Lite also materialises displays. These are different interfaces and output costs, so their timings do not establish an isolated engine speed ratio. Both engines use one CPU, but their memory caps differ. Three samples give preliminary observations only. The [benchmark protocol](benchmarks.md) describes the controlled runs still needed.

The captured container peak covers its entire lifetime and includes charged filesystem cache. Host-side Python memory, shared host cache and the rest of the Docker VM are not included. Startup is a new process against an uncontrolled file cache, not a cold-cache measurement.

## Recorded outcome before the default-substrate correction

These are historical results. The membership milestone corrected our earlier active-only concept default. ECL includes all concepts by default. The old Python check shared that assumption, so agreement with it did not prove the default correct. See [the correction and current evidence](refsets.md#concept-status-defaults).

The [recorded results](basic-ecl-results.json) show 17 complete OneLondon matches and five broad complete-set matches against an independent Python reader of the original RF2 archive. All 22 workload cases therefore have independent expected results. Ten Rust integration tests pass. The pinned official examples classify as 11 supported parses, 110 unsupported features and zero unexpected syntax errors.

Snowstorm Lite matches 18 cases. Four wildcard cases differ:

| ECL | Rust and independent RF2 | Snowstorm Lite |
|---|---:|---:|
| `*` | 838,955 | 1,151,519 |
| `< *` | 838,954 | 1,151,519 |
| `> *` | 209,537 | 1,151,519 |
| `* MINUS (<< 404684003)` | 701,121 | 1,013,685 |

The initial `*` comparison retrieved both full sets. Lite contained every active concept plus 312,564 inactive concepts. A subsequent count preflight avoided repeatedly enumerating known mismatches. The comparison script deliberately exits non-zero when results differ; this baseline does not give an all-green conformance run.

The pinned Lite source explains these results: [`SSubExpressionConstraint.doAddQuery`](https://github.com/IHTSDO/snowstorm-lite/blob/6942831706b68d23a028e16e92d23ea31d10653c/src/main/java/org/snomed/snowstormlite/service/ecl/constraint/SSubExpressionConstraint.java) uses a match-all query for wildcard and bypasses its hierarchy operator. [`ValueSetProvider`](https://github.com/IHTSDO/snowstorm-lite/blob/6942831706b68d23a028e16e92d23ea31d10653c/src/main/java/org/snomed/snowstormlite/fhir/ValueSetProvider.java) accepts an `activeOnly` argument but does not pass it into expansion. The active-only claim in the original analysis was incorrect for concepts. The wildcard and wildcard-exclusion differences came from our default; the hierarchy-wildcard shortcut is a separate issue.

The query container completed the workload under one CPU and a 256 MiB cap. Its charged lifetime peak was 106.6 MiB. That is a basic evaluator measurement, not the full engine's future memory requirement. Small hierarchy queries had median internal evaluation times around 0.6 to 0.7 ms. The 137,834-concept finding expansion took 7.7 ms in its single timed sample. Separate RF2 validation observed the broad wildcard hierarchy queries around 23 ms. These observations need larger, isolated runs before setting performance guarantees.

The stored Snowstorm peak covers both the initial diagnostic run and the revised comparison. The two scripts also ran alongside the independent RF2 validation during part of the revised run. Keep these conditions with the numbers. No isolated engine speedup or final serving-memory ratio is claimed.
