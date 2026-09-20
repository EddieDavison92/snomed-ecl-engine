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
serverless function, a shared VPS or a portable device. This engine keeps the
terminology in one file and evaluates queries inside the calling process.

### What it is designed for

- **Serverless functions.** Compute only when a query arrives. The query-only
  executable is 2.13 MiB, under a megabyte gzipped, and the index is a single
  verified file.
- **A small VPS.** One CPU and a few hundred megabytes serve the whole UK
  release, so an ECL API does not need a cluster behind it.
- **Portable devices.** The index sits beside the application and needs no
  network at query time. Built once it never changes, and it checks its own
  checksums when opened.
- **Agents and tooling.** One persistent process reads expressions as JSONL and
  answers thousands of them without reopening the index.

Everything measured here ran on x86-64 Linux.

No HTTP server lives here, by design. This repository owns the library, index
format, importer, CLI, conformance tests and benchmarks. A deployment
application depends on it and owns hosting.

## What it is good for

Asking this engine how many concepts an expression selects typically takes
2.20 ms. Asking it for every one of those concepts takes 2.29 ms. Evaluating the
expression already built the whole set, so returning it costs almost nothing
more. Snowstorm answers the same two requests in 13.19 ms and 36.40 ms, because
it serialises the concepts and returns them in pages over HTTP.

All four are medians over the same 1,000 expressions. The slowest 5% take
9.97 ms here and 41.63 ms through Snowstorm.

Expanding the 879 expressions that both engines could answer took 9.0 seconds
here and 101.6 seconds through Snowstorm.

- **Expand hundreds of codelists at once.** Turning a directory of static code
  lists into ECL definitions means expanding every one in full and diffing it
  against the original. At about 2 ms each, 274 lists take under a second.
- **Check a codelist against a new release.** `diff` runs one expression across
  two indexes and reports what the release added and removed.
- **Put it in CI.** A two-megabyte binary and an index file let a pipeline assert
  that every definition in a repository still resolves.

### Authoring with an assistant

Giving someone ECL usually means provisioning a terminology server account or an
API key. Here they install one binary and point it at a release they are already
licensed for. Nothing to host, no key to issue, no rate limit and no per-seat
provisioning. An agent can do the setup unaided: [SKILL.md](SKILL.md) takes it
from a clone to a working index and a query.

That makes interactive terminology work practical. Replacing a static code list
with an ECL definition means walking up the hierarchy from every code in the
list, sizing each ancestor that could subsume them, and comparing what each
candidate returns against the list you started with. A list of five codes takes
about thirty expansions; a list of a hundred takes about five hundred. At
roughly 2 ms each that is a tenth of a second at one end and just over a second
at the other, so an assistant can propose a definition, show exactly which
concepts it adds and which it drops, then try a different one.

The same loop through a hosted terminology server is those same hundreds of
requests per suggestion, on a shared service, under someone else's rate limit.

## Measurements

Against the UK Monolith release, 1.15 million concepts.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/footprint-dark.svg">
  <img alt="Index on disk: this engine 290 MiB, Snowstorm Lite 483 MiB, Snowstorm 6.11 GiB. Reading the release and building indexes: 2.0, 17.6 and 72.7 minutes. Memory allocated: 256 MiB, 2 GiB and 12 GiB." src="docs/images/footprint-light.svg">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/latency-dark.svg">
  <img alt="Warm count median: this engine 2.20 ms, Snowstorm Lite 4.56 ms, Snowstorm 13.19 ms. Complete enumeration median: 2.29 ms, 7.15 ms and 36.40 ms." src="docs/images/latency-light.svg">
</picture>

This engine evaluated all 1,000 expressions in the test corpus and returned a
complete code set for every one. Snowstorm could answer 879 of them, and agreed
with us on all 879. Its parser rejected 80, its concept endpoint could not
return 40, and it answered 1 differently.

That one is `(<< 377442002) : 1142138002 != #10`. In this release the concept has
two active values for that attribute, 20 in one relationship group and 10 in
another, so `!= #10` selects it: one of its values is not 10. We return it and
Ontoserver returns it. Snowstorm returns nothing, which reads the test as "has no
value equal to 10".

Snowstorm Lite could answer 587. It reported 320 as using ECL features it does
not implement, rejected 80 at the parser, and answered 13 differently. All 13
are attribute inequalities where Lite returns an empty set, and Snowstorm agrees
with us on every one.

| | |
|---|---:|
| Query-only Linux executable | 2.13 MiB (0.91 MiB gzipped) |
| 10,000-expression corpus, one CPU and 256 MiB | 35.05 s per warm batch |
| Same corpus, four CPUs and four workers | 5.87 s |
| Open a packed index | 525 ms |
| Open an uncompressed index | 285 ms |

[Benchmarks](docs/benchmarks.md) has the method, the raw samples, the
disagreements and the limits of these numbers.

## What it supports

The engine implements every ECL 2.3 feature area: hierarchy and Boolean sets,
refinements, groups and cardinalities, reverse and dotted attributes, exact
concrete comparisons, top and bottom, membership, concept filters, description
filters, member filters and projections, history supplements and alternate
identifiers.

Three forms are valid under the grammar but have no clear meaning in the
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
