# SNOMED ECL engine

Evaluate SNOMED CT Expression Constraint Language locally, without running a
terminology server.

A Rust library, a CLI and an RF2 index builder sharing one implementation. Point
it at a release, build an immutable index, and query it — no Elasticsearch, no
JVM, no database, no service to keep alive.

```sh
snomed-ecl-engine use uk.ecl
snomed-ecl-engine expand '<< 195967001 |Asthma|' --display
```

## Why it is small

Most ways to evaluate ECL assume a server: a long-running process, a search
cluster, gigabytes of resident memory. That rules out whole classes of
deployment. This engine is built so the terminology *is* a file and the query
engine is a function.

- **Serverless.** Compute only when a query arrives. The query-only executable is
  2.13 MiB, under a megabyte gzipped, and the index is a single verified file.
- **A small VPS.** One CPU and a few hundred megabytes serves the whole UK
  release, so an ECL API does not need a cluster behind it.
- **Mobile and offline.** No network dependency at query time. An index built
  once is immutable and self-verifying.
- **Agents and tooling.** A persistent JSONL process answers thousands of
  expressions without reopening the index. [SKILL.md](SKILL.md) is the agent
  workflow.

The HTTP wrapper is deliberately **not** here. This repository owns the library,
index format, importer, CLI, conformance tests and benchmarks; a deployment
application depends on it and owns hosting.

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

Bring your own licensed RF2 Snapshot; no release content is in this repository.
`import` verifies the checksum you supply before reading anything.

| Command | |
|---|---|
| `inspect` | Read an archive's release metadata and print its import command |
| `import` · `add-refsets` | Build an index; add simple refsets such as UK PCD |
| `pack` · `verify` | One compressed file; check every section |
| `stores` · `use` · `stats` | Find indexes, select one, inspect it |
| `query` | Evaluate expressions against one open index |
| `expand` · `batch` | One expression; or JSONL on stdin for scripts and agents |
| `diff` | Compare one expression across two indexes |

Full reference: the [CLI guide](docs/cli.md).

## What this makes possible

Terminology servers are built to answer *how many* and *show me a page*. This
engine costs the same to hand you **every code** — 2.20 ms to count, 2.29 ms to
enumerate — because evaluation already produced the whole set. Snowstorm goes
from 13.19 ms to 36.40 ms on the same expressions, because it has to serialise
and page the result over HTTP.

That flat cost is what changes which jobs are reasonable:

- **Expand hundreds of codelists at once.** Converting a directory of static code
  lists into ECL definitions means enumerating every one and diffing it against
  the original. At ~2 ms each that is a loop; against a paged HTTP API it is a
  batch job you schedule.
- **Check a codelist against a new release.** `diff` runs one expression across
  two indexes and reports what a release added and removed.
- **Put it in CI.** A two-megabyte binary and an index file mean a pipeline can
  assert that every definition in a repository still resolves.
- **Work offline.** Mobile, air-gapped or field use: no service, no network at
  query time, and the index verifies itself when opened.
- **Give an agent a terminology.** One persistent JSONL process answers thousands
  of expressions without reopening the index.

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

Over the same 1,000-expression corpus, **879** expressions returned complete code
sets identical to Snowstorm's, up from 719 before membership, descriptions,
history and filters landed. Snowstorm Lite matched 587, declaring 320 of the
expressions to use features it does not implement.

| | |
|---|---:|
| Query-only Linux executable | 2.13 MiB (0.91 MiB gzipped) |
| 10,000-expression corpus, one CPU and 256 MiB | 35.05 s per warm batch |
| Same corpus, four CPUs and four workers | 5.87 s |
| Index open, packed | 1.13 s |
| Container start to first response | 1.72 s |

Method, raw samples, the disagreements and what these numbers are not:
[benchmarks](docs/benchmarks.md).

## What it supports

Every ECL 2.3 feature area is implemented — hierarchy and Boolean sets,
refinements, groups and cardinalities, reverse and dotted attributes, exact
concrete comparisons, top and bottom, membership, concept filters, description
filters, member filters and projections, history supplements and alternate
identifiers.

Three grammar-valid forms have no settled meaning in the specification and are
refused rather than guessed. Unsupported input always fails explicitly; no query
returns a partial answer as a success. [ECL support](docs/ecl-support.md) has the
detail and the grammar inventory; [the roadmap](docs/roadmap.md) has what is left.

Decimals keep their exact spelling and are never compared as binary floating
point. Relationship groups survive import. The engine reads the published
inferred view and does not classify.

## Embed it

```rust
let store = NumericStore::open(Path::new("uk.ecl"))?;
let expression = ecl::parse("<< 64572001 |Disease|")?;
let ordinals = eval::evaluate_with_limits(&store, &expression, limits)?;
let codes = ordinals.iter().map(|&o| store.ids[o as usize]);
```

Keep the store open across queries. Results are concept ordinals that resolve
through `store.ids`; display labels are a separate lookup. Use
`eval::evaluate_result_with_limits` to accept member projections, which return
typed scalars or rows as well as concept sets. `--no-default-features` drops the
ZIP importer for a query-only build.

## Documentation

- [CLI guide](docs/cli.md) — commands, output formats, scripting
- [ECL support](docs/ecl-support.md) — what evaluates, and the open questions
- [Indexes](docs/indexes.md) — building, packing, format, configuration
- [Benchmarks](docs/benchmarks.md) — method, results and comparisons
- [Roadmap](docs/roadmap.md) — what is next
- [Developer setup](docs/setup.md) — building, releases, comparison servers
- [SKILL.md](SKILL.md) — the agent workflow
