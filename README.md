# SNOMED ECL engine

A compact SNOMED CT Expression Constraint Language engine, written in Rust. Expand ECL locally or embed the library in an application without running a terminology server.

Built for lightweight CLI tools, fast batch expansion and applications that only need compute when queries arrive. The library, native CLI and RF2 index builder share one implementation. Immutable local indexes keep query execution independent of databases, HTTP services and cloud providers.

## Performance so far

Measured against the UK SNOMED CT Monolith, with 1.15 million concepts:

| Workload | Measured result |
|---|---|
| 880 numeric-index expressions, one CPU and 256 MiB | 1.82 seconds per warm batch |
| Original 1,000 expressions, one CPU and 256 MiB | 2.71 seconds per warm batch; 1.65 ms median request |
| Expanded 10,000-expression corpus across 45 categories, one CPU and 256 MiB | 35.05 seconds per warm batch; 1.75 ms median request |
| Measured numeric query-only Linux executable, without Unicode | 0.83 MiB, 0.39 MiB gzipped |
| Numeric and concept-membership indexes | 103.3 MiB |
| Complete current UK index, with descriptions, displays and typed members | 289.5 MiB packed, down from 1,110.8 MiB |
| RF2 import, including descriptions, displays and typed members | 119.8 seconds with two CPUs and 3 GiB |

Batch times are medians of five shuffled runs through one persistent process, with no result cache. Each count request evaluates the full result set. The [1,000-case benchmark](validation/combined-ecl-corpus-results.json) and [10,000-case benchmark](validation/ecl-10000-completion-results.json) pin the release, binary, resource limits and result digests. The larger corpus preserves all 1,000 original expressions and complete results. All 10,000 sets remain unchanged after the latest compatibility fixes. Request p95 was 13.79 ms and container-charged peak memory was 227.15 MiB. An independent RF2 check matches 3,920 of its complete result sets. These figures measure the fixed workloads, not full ECL conformance or hosted cold starts. See [corpus construction and measurement conditions](docs/corpus.md).

The packed store opened in 1.13 seconds; container start through the first response took 1.72 seconds. Checksums and structural validation remain enabled, and description and member data load on demand. Filesystem caches were not dropped. Earlier [startup measurements](validation/startup-results.json) identified and fixed an unbuffered manifest read.

## Compared with Snowstorm

The intended advantage is fast embedded and batch expansion with a small runtime and no separate search service. These observations use the same UK release:

| Metric | This engine | Snowstorm Lite 2.7.0 | Snowstorm 11.0.0 |
|---|---|---|---|
| Query architecture | Rust library or native CLI | Java service with Lucene | Java service plus Elasticsearch |
| Observed import time | 119.8 seconds, including typed members | 1,057 seconds, 17.6 minutes | 4,360 seconds, 72.7 minutes |
| Current index files | 289.5 MiB packed with descriptions, displays and typed members; 103.3 MiB original numeric components | 483.3 MiB | 6.11 GiB Elasticsearch directory |
| Serving allocations used | One CPU; 256 MiB for the current corpus and separate text probes | One CPU, 2 GiB | Each service: four CPUs, 6 GiB |
| Median request, 719-expression Snowstorm comparison | **2.24 ms** | Not measured on this workload | **12.66 ms** |
| Request p95, same 719 expressions | 8.81 ms | Not measured on this workload | 38.92 ms |
| Median 719-request batch | 2.16 seconds | Not measured on this workload | 10.32 seconds |
| Median request, 18-expression Lite comparison | **1.93 ms** | **15.99 ms** | Not measured on this workload |

Request timings include transport. Rust uses a persistent JSONL process; the servers use loopback HTTP. The Lite comparison also includes pagination and display materialisation. Latency statistics include only expressions with matching complete result sets. The Lite and full Snowstorm workloads are different, so their columns do not establish a speed ranking between the two servers.

Index contents and import allocations also differ. Rust now packs its existing components into independently compressed blocks; packing took a further 33.6 seconds on one CPU. The full language still has conformance gaps. These are observed builds, not equal-capability storage ratios or minimum serving allocations. Sources: [container measurements](docs/container.md), [latest RF2 import](validation/combined-ecl-results.json), [Snowstorm comparison](docs/full-snowstorm.md), [Lite comparison](docs/basic-ecl.md) and [derived latency figures](validation/readme-comparison.json).

The comparison servers also have ECL coverage limits. Lite documents an ECL Core subset without attribute groups, concept/description/member filters or member-field selection in its [pinned source](https://github.com/IHTSDO/snowstorm-lite/blob/6942831706b68d23a028e16e92d23ea31d10653c/README.md#ecl-utility-endpoints). The tested full Snowstorm parser rejected all 80 top/bottom expressions in our corpus. We also recorded a concrete-inequality disagreement where Rust matched OneLondon's Ontoserver and the RF2 evidence.

OneLondon's Ontoserver 6.25.4 also rejected our [description metadata probes](validation/ontoserver-description-metadata.json), including `type` filters. These findings apply to the tested versions; full ECL 2.3 remains this project's target, not a claim that it is already complete.

Description metadata now uses compact dictionaries, with term text read from disk as needed. The rewrite reduced the 1,000-expression corpus's peak from 542.6 MiB to 219.4 MiB under a 1 GiB limit. The latest combined build preserved every result at 256 MiB, with a **1.65 ms median request**, a **2.71-second median batch** and a 218.3 MiB charged peak. Ten text probes also passed at 256 MiB. These checks do not establish that every ECL workload fits that limit. The [description measurements](docs/descriptions.md#compact-runtime-measurements) and [combined evaluation](validation/combined-ecl-results.json) record the builds separately.

Typed member data occupies 175.6 MiB packed, compared with 565.6 MiB decoded, and loads refsets lazily. Loaded tables remain cached; the corpus does not touch every table. The [storage plan](docs/performance-plan.md) includes their cost when measuring the full-engine memory target.

## Use it

Build the library and CLI with the pinned Rust toolchain. This example uses Linux:

```sh
git clone https://github.com/EddieDavison92/snomed-ecl-engine.git
cd snomed-ecl-engine
cargo build --locked --release

# Import your RF2 Snapshot ZIP into a new directory.
./target/release/snomed-ecl-engine import RF2_ZIP INDEX_DIRECTORY EDITION_URI TRUSTED_SHA256

# Expand, count or return codes with display labels.
./target/release/snomed-ecl-engine expand INDEX_DIRECTORY '<< 64572001' --count
./target/release/snomed-ecl-engine expand INDEX_DIRECTORY '<< 195967001' --display

# Pack the complete index into one file. All store arguments accept this file.
./target/release/snomed-ecl-engine pack INDEX_DIRECTORY uk.ecl
./target/release/snomed-ecl-engine verify uk.ecl

# Reuse one process for batches of expressions.
printf '%s\n' '{"ecl":"<< 195967001","count_only":true}' |
  ./target/release/snomed-ecl-engine batch INDEX_DIRECTORY
```

Use your own licensed RF2 content. Archives and generated indexes are not included. The importer verifies the supplied checksum and edition, then writes an immutable index. Supplementary simple refsets, including the UK PCD package, can be added with `add-refsets`.

For Rust integration, open a `NumericStore`, parse with `ecl::parse` and evaluate with `eval::evaluate_with_limits`. Keep the store open for repeated queries. Results are concept ordinals that resolve to SNOMED IDs; display labels are a separate lookup. Disable default Cargo features to omit the offline ZIP importer from a query-only application.

Use `eval::evaluate_result_with_limits` when accepting [member-field projections](docs/member-filters.md). It preserves typed scalar sets and rows as well as concept results. The CLI exposes typed results as JSONL.

Description term queries need `--features unicode` and ICU4C development libraries at build time. The [Unicode build guide](docs/descriptions.md#build-with-unicode-term-matching) covers installation and the additional executable size. Numeric queries do not require this feature.

The measured CLI with import, Unicode and block compression is 33.91 MiB, or 13.36 MiB gzipped. Broad term queries currently scan descriptions and are slower than numeric expansions. [Term measurements](docs/descriptions.md#compact-runtime-measurements) record their latency and correctness separately.

The [CLI guide](docs/cli.md) covers commands and output formats. The repository [SKILL.md](SKILL.md) gives agents the build, RF2 loading and querying workflow.

## Development status

Full ECL 2.3 support is the acceptance requirement. Current capabilities include hierarchy and Boolean operations, nested refinements, groups and cardinalities, exact concrete comparisons, top/bottom, concept refsets, concept filters and description filters. The optional Unicode backend evaluates term prefixes, wildcards and term sets.

The 10,000-expression corpus evaluates all cases, with all 1,000 original result sets unchanged. This measures coverage of that workload, not percentage conformance to the language. History profiles, alternate identifiers, typed member projections and configurable dialect aliases are implemented. Remaining semantic details are still in progress. Unsupported expressions fail explicitly. The [conformance checklist](docs/conformance.md) tracks the remaining work.

The engine is intended to power embedded tools, low-resource servers and serverless applications. An HTTP or deployment wrapper belongs in a separate application that consumes the library. Cloud cold starts and the complete engine's final resource footprint still need measurement.

## Further reading

- [Engine design and implementation plan](docs/plan.md)
- [RF2 storage](docs/compact-store.md), [supplementary refsets](docs/refsets.md) and [description data](docs/descriptions.md)
- [Performance experiments](docs/performance-plan.md) and [serverless requirements](docs/serverless.md)
- [Benchmark protocol](docs/benchmarks.md) and [Snowstorm comparison](docs/full-snowstorm.md)
- [Pinned release](docs/release.json) and [reference projects](docs/references.json)
