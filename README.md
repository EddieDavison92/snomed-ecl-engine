# SNOMED ECL engine

Evaluate SNOMED CT Expression Constraint Language without running a terminology
server.

Most ways to run ECL assume a server: a JVM that stays up, a search cluster
beside it, gigabytes of memory. This is a Rust library and a CLI instead. You
point it at a SNOMED CT release, it builds an index once, and you query that
index inside your own process.

```sh
snomed-ecl-engine use data/uk.ecl
snomed-ecl-engine expand '<< 195967001 |Asthma|' --display
```

## Status

Version 0.1.0. The engine evaluates every ECL 2.3 feature area; [ECL
support](docs/ecl-support.md) lists the three grammar forms it refuses and why.
Releases publish Linux x86-64 executables. The crate is not on crates.io.

The index format may change between releases without a migration: rebuild the
index from your RF2 archive when you upgrade.

## Install

Download an executable from [Releases](https://github.com/EddieDavison92/snomed-ecl-engine/releases).
Each release has three builds:

| Build | Size | For |
|---|---:|---|
| `query` | 2.5 MiB | Querying an existing index, packing and verifying |
| `default` | 3.3 MiB | The above, plus importing RF2 |
| `unicode` | 34.5 MiB | The above, plus term matching in description filters |

Or build from source with Rust 1.93 or later:

```sh
cargo install --locked --git https://github.com/EddieDavison92/snomed-ecl-engine
```

Add `--features unicode` for term matching, which needs ICU 72 or later
(`libicu-dev` and `pkg-config` on Debian or Ubuntu). [Developer
setup](docs/setup.md) covers other platforms and Docker.

## Quick start

You need a SNOMED CT RF2 Snapshot that you are licensed to use; see
[licence](#licence). UK Monolith is the tested edition: download it from NHS
England's [TRUD](https://isd.digital.nhs.uk/trud/) and note the SHA-256 shown on
its download page.

```sh
# Read the archive's release metadata; prints the import command for it.
snomed-ecl-engine inspect uk_sct2mo_42.5.0_20260826000001Z.zip

# Build the index, then keep it in one compressed file.
snomed-ecl-engine import uk_sct2mo_42.5.0_20260826000001Z.zip data/index EDITION_URI SHA256
snomed-ecl-engine pack data/index data/uk.ecl

# Choose it once; later commands need no path.
snomed-ecl-engine use data/uk.ecl
snomed-ecl-engine expand '<< 195967001 |Asthma|' --count
snomed-ecl-engine query
```

`import` checks the SHA-256 you supply before reading anything. Compare it with
the value the distributor published: a checksum of your own download shows it is
intact, not where it came from.

| Command | |
|---|---|
| `inspect` | Read an archive's release metadata and print its import command |
| `import` · `add-refsets` | Build an index; add simple refsets such as UK PCD |
| `pack` · `verify` | Write one compressed file; check every section |
| `stores` · `use` · `stats` | Find indexes, select one, inspect it |
| `query` | Evaluate expressions interactively against one open index |
| `expand` · `batch` | Evaluate one expression; or JSONL on stdin for scripts and agents |
| `hierarchy` | List a concept's parents, children, ancestors or descendants |
| `diff` | Compare one expression across two indexes |

The [CLI guide](docs/cli.md) is the full reference.

## Why use it

Evaluating ECL is set algebra over a graph that is already on your disk. Here
the terminology is one file and the query engine is a function call, so ECL can
run wherever your code runs:

- **Serverless functions.** The query-only executable is 2.5 MiB, 1.1 MiB
  gzipped, and the index is one file. Starting the process, opening the index
  and answering a query takes 163 ms on one CPU.
- **A small server.** One CPU and 256 MiB serve the whole UK release for
  typical workloads; one that loads every index at once needs 320 MiB.
- **Offline.** The index sits beside your application and needs no network at
  query time. It never changes after it is built.
- **Agents and tooling.** One process reads expressions as JSONL and answers
  thousands of them without reopening the index. [SKILL.md](SKILL.md) takes an
  agent from a clone to a first query.

Counting the concepts an expression selects takes 0.86 ms, and returning every
one of them takes 0.94 ms. They are nearly the same because evaluating the
expression already built the set; an HTTP API has to serialise the concepts and
page them back, and that cost grows with the answer.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/expansion-scaling-dark.svg">
  <img alt="Cost of a complete expansion against the number of concepts returned, both axes logarithmic. This engine runs from about 0.8 ms at one concept to 0.33 s at 839,000. Snowstorm asked for the first time runs from about 35 ms to 30 s; once cached, from about 30 ms to 2 s." src="docs/images/expansion-scaling-light.svg">
</picture>

So expanding a definition in full is cheap enough to do routinely:

- **Convert code lists.** Turning static code lists into ECL definitions means
  expanding each candidate in full and diffing it against the original list. At
  about a millisecond each, hundreds of lists take under a second.
- **Check a code list against a new release.** `diff` runs one expression
  across two indexes and reports what the release added and removed.
- **Test definitions in CI.** An executable of under 3 MiB and an index file let
  a pipeline assert that every definition still resolves.

This is not a replacement for a terminology server. Snowstorm does a great deal
this engine does not; expanding ECL is the one job both do.
[Benchmarks](docs/benchmarks.md#is-this-a-fair-comparison) sets out where the
comparison is and is not fair.

## Measurements

Against the UK Monolith release of 26 August 2026: 1.15 million concepts.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/footprint-dark.svg">
  <img alt="Index on disk: this engine 152 MiB, Snowstorm Lite 483 MiB, Snowstorm 6.11 GiB. Reading the release and building indexes: 2.3, 17.6 and 72.7 minutes. Memory allocated: 256 MiB, 2 GiB and 12 GiB." src="docs/images/footprint-light.svg">
</picture>

Speed only counts if the answers match, so the corpus compares whole code sets
rather than totals. This engine returned a set for all 1,000 expressions.
Snowstorm agreed on 879, disagreed on 1, where the RF2 rows support this
engine's answer, and could not answer 120. Snowstorm Lite agreed on 587, returned
13 wrong answers, and could not answer 400.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/latency-dark.svg">
  <img alt="Warm count median: this engine 0.86 ms on 1 CPU and 256 MiB, Snowstorm Lite 4.56 ms on 1 CPU and 2 GiB, Snowstorm 13.19 ms on 8 CPUs and 12 GiB. Complete enumeration median: 0.94 ms, 7.15 ms and 36.40 ms on the same allocations." src="docs/images/latency-light.svg">
</picture>

| Measurement | This engine |
|---|---:|
| Packed index for the UK release | 152 MiB |
| Open a packed index, one CPU | 156 ms |
| 1,000-expression corpus, median count / enumeration | 0.86 ms / 0.94 ms |
| 10,000-expression corpus through the CLI, one CPU | 11.5 s per batch |
| Same corpus through the library, one CPU | 2.0 s per batch |

[Benchmarks](docs/benchmarks.md) has the method, every figure's evidence file,
the disagreements and where this engine is slower.

### How the index is so small

The UK release is 903 MiB as a directory of sections and 152 MiB packed:

- Concepts are four-byte ordinals from the first read on, never 18-digit codes.
- Sorted lists, such as the concepts sharing a word, store the gaps between
  values, mostly one byte each.
- Reference set rows are sorted by the component they reference, so
  neighbouring rows compress into each other. The member UUID, which ECL never
  uses, is not stored.
- Each display label is its own zstd frame against a dictionary trained on the
  labels, so reading one label is still one read.
- Sections are packed as independent zstd blocks, small for descriptions so
  describing a concept decodes only what it reads.

None of this slows queries: sections read whole decode once into plain arrays,
and labels and single-concept lookups still read only what they need. [How the
index is built](docs/index-format.md) explains each technique and what it saves.

## Finding concepts by name

ECL answers which concepts, never what a concept is. A browser needs both, so
the index also holds a word index over active description terms, and a lookup
that returns a concept's descriptions, hierarchy neighbours, relationship groups
and reference set membership.

A search of the whole UK edition takes under a millisecond once the word index
is loaded, and `within` limits it to an expression's answer. The same question
asked as an ECL term filter scans every description in scope. Words are
extracted when the index is built, so a search is a binary search and a list
intersection, and it needs no collation library at query time.

```sh
echo '{"search":"chronic kidney","limit":5}' | snomed-ecl-engine batch data/uk.ecl
echo '{"concept":"709044004"}'               | snomed-ecl-engine batch data/uk.ecl
```

## What it supports

Every ECL 2.3 feature area: hierarchy and Boolean sets, refinements, groups and
cardinalities, reverse and dotted attributes, exact concrete comparisons, top
and bottom, membership, concept filters, description filters, member filters and
projections, history supplements and alternate identifiers.

Three forms are valid under the grammar but have no clear meaning in the
specification, so the parser refuses them rather than guess. Two have open
questions with SNOMED International. Unsupported input fails with an explicit
error; no query returns a partial answer as a success.

Decimals keep their exact spelling and are never compared as binary floating
point. Relationship groups survive import. The engine reads the published
inferred view and does not classify.

## Embed it

```rust
use snomed_ecl_engine::{ecl, eval, store::NumericStore};
use std::path::Path;

let store = NumericStore::open(Path::new("data/uk.ecl"))?;
let expression = ecl::parse("<< 64572001 |Disease|")?;
let ordinals = eval::evaluate(&store, &expression)?;
let codes: Vec<u64> = ordinals.iter().map(|&o| store.ids[o as usize]).collect();
```

Open the store once and keep it for every query. Results are concept ordinals
that resolve through `store.ids`; display labels are a separate lookup through
`DisplayStore`. `eval::evaluate_result` also returns member projections, which
can be typed values or rows rather than concepts, and the `_with_limits`
variants bound the work a query may do. Build with `default-features = false`
to leave out the RF2 importer.

## Documentation

- [CLI guide](docs/cli.md): commands, output formats and the batch protocol.
- [ECL support](docs/ecl-support.md): what evaluates, and the open questions.
- [Indexes](docs/indexes.md): building, supplements, packing, configuration.
- [How the index is built](docs/index-format.md): encodings, compression and reads.
- [Benchmarks](docs/benchmarks.md): method, results and comparisons.
- [Roadmap](docs/roadmap.md): open work.
- [Developer setup](docs/setup.md): building from source and reproducing the benchmarks.
- [Contributing](CONTRIBUTING.md) and [security](SECURITY.md).

## Licence

The code is licensed under either the [Open Government Licence v3.0](LICENSE-OGL)
or the [MIT licence](LICENSE-MIT), at your option.

SNOMED CT is not included and is licensed separately. It is owned by SNOMED
International, and you need a licence to use it: in the UK, through NHS England's
[TRUD](https://isd.digital.nhs.uk/trud/); in other member countries, through the
national release centre or [MLDS](https://mlds.ihtsdotools.org/). An index built
from a release contains SNOMED CT content, so share it only as your licence
allows.
