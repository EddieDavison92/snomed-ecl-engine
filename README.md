# SNOMED ECL engine

Evaluate SNOMED CT Expression Constraint Language locally, without running a
terminology server.

A Rust library, a CLI and an RF2 index builder sharing one implementation. Point
it at a release, build an immutable index, and query it. No Elasticsearch, no
JVM, no database, no service to keep alive.

```sh
snomed-ecl-engine use uk.ecl
snomed-ecl-engine expand '<< 195967001 |Asthma|' --display
```

## Why it is small

Most ways to evaluate ECL need a server: a process that stays up, a search
cluster beside it, gigabytes of resident memory. That puts ECL out of reach of a
serverless function, a shared VPS or a phone. This engine keeps the terminology
in one file and evaluates queries inside the calling process.

- **Serverless.** Compute only when a query arrives. The query-only executable is
  2.13 MiB, under a megabyte gzipped, and the index is a single verified file.
- **A small VPS.** One CPU and a few hundred megabytes serve the whole UK
  release, so an ECL API does not need a cluster behind it.
- **Mobile and offline.** No network dependency at query time. An index built
  once never changes, and checks its own checksums when opened.
- **Agents and tooling.** A persistent JSONL process answers thousands of
  expressions without reopening the index. [SKILL.md](SKILL.md) is the agent
  workflow.

No HTTP server lives here, by design. This repository owns the library, index
format, importer, CLI, conformance tests and benchmarks. A deployment
application depends on it and owns hosting.

## What it is good for

Asking this engine how many concepts an expression selects takes 2.20 ms.
Asking it for every one of those concepts takes 2.29 ms. Evaluating the
expression already built the whole set, so returning it costs almost nothing
more. Snowstorm answers the same two requests in 13.19 ms and 36.40 ms, because
it serialises the concepts and returns them in pages over HTTP.

Expanding 879 definitions in full took 9.0 seconds here. The same 879 through
Snowstorm took 101.6 seconds.

- **Expand hundreds of codelists at once.** Turning a directory of static code
  lists into ECL definitions means expanding every one in full and diffing it
  against the original. At about 2 ms each, 274 lists take under a second.
- **Check a codelist against a new release.** `diff` runs one expression across
  two indexes and reports what the release added and removed.
- **Put it in CI.** A two-megabyte binary and an index file let a pipeline assert
  that every definition in a repository still resolves.
- **Work offline.** Mobile, air-gapped or field use. No service, no network at
  query time, and the index checks its own checksums when opened.
- **Give an agent a terminology.** One persistent JSONL process answers thousands
  of expressions without reopening the index.

## Get started

```sh
cargo build --locked --release --bin snomed-ecl-engine

# What does this archive declare? Prints the import command for it.
snomed-ecl-engine inspect uk_release.zip

# Build an immutable index, then keep it in one compressed file.
snomed-ecl-engine import uk_release.zip index/ EDITION_URI SHA256
snomed-ecl-engine pack index/ uk.ecl

# Choose it once; later commands need no path.
snomed-ecl-engine use uk.ecl
snomed-ecl-engine query
```

Bring your own licensed RF2 Snapshot. This repository contains no release
content, and `import` verifies the checksum you supply before reading anything.

| Command | |
|---|---|
| `inspect` | Read an archive's release metadata and print its import command |
| `import` · `add-refsets` | Build an index; add simple refsets such as UK PCD |
| `pack` · `verify` | One compressed file; check every section |
| `stores` · `use` · `stats` | Find indexes, select one, inspect it |
| `query` | Evaluate expressions against one open index |
| `expand` · `batch` | One expression; or JSONL on stdin for scripts and agents |
| `diff` | Compare one expression across two indexes |

The [CLI guide](docs/cli.md) is the full reference.

## Measurements

Against the UK Monolith release, 1.15 million concepts.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/footprint-dark.svg">
  <img alt="Index on disk: this engine 290 MiB, Snowstorm Lite 483 MiB, Snowstorm 6.11 GiB. Import time: 2.0, 17.6 and 72.7 minutes. Memory allocated: 256 MiB, 2 GiB and 12 GiB." src="docs/images/footprint-light.svg">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/latency-dark.svg">
  <img alt="Warm count median: this engine 2.20 ms, Snowstorm Lite 4.56 ms, Snowstorm 13.19 ms. Complete enumeration median: 2.29 ms, 7.15 ms and 36.40 ms." src="docs/images/latency-light.svg">
</picture>

Over the same 1,000-expression corpus, 879 expressions returned complete code
sets identical to Snowstorm's, up from 719 before membership, descriptions,
history and filters landed. Snowstorm Lite matched 587. It declared 320 of the
expressions to use features it does not implement.

| | |
|---|---:|
| Query-only Linux executable | 2.13 MiB (0.91 MiB gzipped) |
| 10,000-expression corpus, one CPU and 256 MiB | 35.05 s per warm batch |
| Same corpus, four CPUs and four workers | 5.87 s |
| Index open, packed | 1.13 s |
| Container start to first response | 1.72 s |

[Benchmarks](docs/benchmarks.md) has the method, the raw samples, the
disagreements and the limits of these numbers.

## What it supports

The engine implements every ECL 2.3 feature area: hierarchy and Boolean sets,
refinements, groups and cardinalities, reverse and dotted attributes, exact
concrete comparisons, top and bottom, membership, concept filters, description
filters, member filters and projections, history supplements and alternate
identifiers.

Three forms are valid under the grammar but have no settled meaning in the
specification, so the parser refuses them rather than guess:

- a reverse flag inside an attribute group, `* : { R 363698007 = X }`
- a member filter with no refset operator, `X {{ M active = true }}`
- a reverse flag applied to a concrete value

Two of those have questions open with SNOMED International, still unanswered.
Everything else in ECL 2.3 evaluates. Unsupported input fails with an explicit
error, and no query returns a partial answer as a success.

Decimals keep their exact spelling, and the evaluator never compares them as
binary floating point. Relationship groups survive import. The engine reads the
published inferred view and does not classify.

## Embed it

```rust
let store = NumericStore::open(Path::new("uk.ecl"))?;
let expression = ecl::parse("<< 64572001 |Disease|")?;
let ordinals = eval::evaluate_with_limits(&store, &expression, limits)?;
let codes = ordinals.iter().map(|&o| store.ids[o as usize]);
```

Keep the store open across queries. Results are concept ordinals that resolve
through `store.ids`. Display labels are a separate lookup. Use
`eval::evaluate_result_with_limits` to accept member projections, which return
typed scalars or rows as well as concept sets. `--no-default-features` drops the
ZIP importer for a query-only build.

## Documentation

- [CLI guide](docs/cli.md) covers commands, output formats and scripting.
- [ECL support](docs/ecl-support.md) lists what evaluates and the open questions.
- [Indexes](docs/indexes.md) covers building, packing, the format and configuration.
- [Benchmarks](docs/benchmarks.md) has the method, results and comparisons.
- [Roadmap](docs/roadmap.md) lists the open work.
- [Developer setup](docs/setup.md) covers building, releases and comparison servers.
- [SKILL.md](SKILL.md) is the agent workflow.
