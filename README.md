# SNOMED ECL engine

A compact SNOMED CT Expression Constraint Language engine, written in Rust. Expand ECL locally or embed the library in an application without running a terminology server.

Built for lightweight CLI tools, fast batch expansion and applications that only need compute when queries arrive. The library, native CLI and RF2 index builder share one implementation. Immutable local indexes keep query execution independent of databases, HTTP services and cloud providers.

## Performance so far

Measured against the UK SNOMED CT Monolith, with 1.15 million concepts:

| Workload | Measured result |
|---|---|
| 880 numeric-index expressions, one CPU and 256 MiB | 1.82 seconds per warm batch |
| 920 expressions including description metadata, one CPU and 1 GiB | 1.86 seconds per warm batch |
| Query-only Linux executable | 0.83 MiB, 0.39 MiB gzipped |
| Numeric and concept-membership indexes | 103.3 MiB |
| RF2 import, including descriptions and displays | 83.6 seconds with two CPUs and 2 GiB |

Batch times are medians of five shuffled runs through one persistent process, with no result cache. Each count request evaluates the full result set. The [benchmark record](validation/description-corpus-results.json) pins the release, binary, resource limits and result digests.

In a separate same-release comparison, 719 expressions produced identical complete results in Rust and Snowstorm. Median batch request time was **2.16 seconds for Rust versus 10.32 seconds for Snowstorm**. Rust used a persistent CLI process; Snowstorm used loopback HTTP with larger resource allocations. See the [comparison and methodology](docs/full-snowstorm.md) for transport, cache and resource details.

Description data currently adds 376 MiB and loads on demand. The description-inclusive run peaked at 537 MiB of container-charged memory; it does not fit in 256 MiB yet. Compression and bounded loading are the next storage targets. The [description measurements](docs/descriptions.md) include the failed 256 MiB run as well as successful checks.

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

# Reuse one process for batches of expressions.
printf '%s\n' '{"ecl":"<< 195967001","count_only":true}' |
  ./target/release/snomed-ecl-engine batch INDEX_DIRECTORY
```

Use your own licensed RF2 content. Archives and generated indexes are not included. The importer verifies the supplied checksum and edition, then writes an immutable index. Supplementary simple refsets, including the UK PCD package, can be added with `add-refsets`.

For Rust integration, open a `NumericStore`, parse with `ecl::parse` and evaluate with `eval::evaluate_with_limits`. Keep the store open for repeated queries. Results are concept ordinals that resolve to SNOMED IDs; display labels are a separate lookup. Disable default Cargo features to omit the offline ZIP importer from a query-only application.

The [CLI guide](docs/cli.md) covers commands and output formats. The repository [SKILL.md](SKILL.md) gives agents the build, RF2 loading and querying workflow.

## Development status

Full ECL 2.3 support is the acceptance requirement. Current capabilities include hierarchy and Boolean operations, nested refinements, groups and cardinalities, exact concrete comparisons, top/bottom, concept refsets, concept filters and description metadata filters.

The 1,000-expression corpus currently evaluates 920 cases. This measures coverage of that workload, not percentage conformance to the language. Term matching, typed member filters and projections, history, alternate identifiers and remaining semantic details are still in progress. Unsupported expressions fail explicitly. The [conformance checklist](docs/conformance.md) tracks the remaining work.

The engine is intended to power embedded tools, low-resource servers and serverless applications. An HTTP or deployment wrapper belongs in a separate application that consumes the library. Cloud cold starts and the complete engine's final resource footprint still need measurement.

## Further reading

- [Engine design and implementation plan](docs/plan.md)
- [RF2 storage](docs/compact-store.md), [supplementary refsets](docs/refsets.md) and [description data](docs/descriptions.md)
- [Performance experiments](docs/performance-plan.md) and [serverless requirements](docs/serverless.md)
- [Benchmark protocol](docs/benchmarks.md) and [Snowstorm comparison](docs/full-snowstorm.md)
- [Pinned release](docs/release.json) and [reference projects](docs/references.json)
