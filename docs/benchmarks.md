# Benchmarks

All figures are for the UK Monolith release of 26 August 2026, pinned in
[release.json](release.json), on x86-64 Linux in Docker. Each section names the
evidence file its figures come from; the files are in [`validation/`](../validation).

These pages measure two workloads separately, because their costs differ.

**Counting** asks for the size of a result. Snowstorm answers with the total and
at most one concept identifier.

**Enumerating** asks for every concept in the result, which is what building a
code list, an export or a value set needs. Snowstorm returns those concepts in
pages of up to 10,000, so a 65,000-concept result costs seven HTTP round trips
and 65,000 serialised entries. This engine returns the set from memory in one
response.

The two cost this engine 0.86 ms and 0.94 ms. They cost Snowstorm 13.19 ms and
36.40 ms. Quoting a count latency for an enumeration workload would understate
Snowstorm's cost by a factor of three, so the tables below keep them apart.

## Is this a fair comparison?

This engine is not trying to be Snowstorm, and nothing here is a verdict on
Snowstorm. The comparison answers one narrower question: if you were going to
batch-expand ECL through a terminology server's API, what does that cost, and
what does doing the same work in your own process cost instead?

**What is fair.** Both engines answer the same expressions against the same
release, gated by four checks before any timing runs: the index manifest and the
recorded import agree on edition and archive checksum, the server advertises
that edition, its branch head is unchanged since the import finished, and two
sentinel queries return exact expected counts. Only expressions where both
returned identical code sets receive paired timings. Snowstorm is asked for its
cheapest form, `returnIdOnly=true`, at its own maximum page size of 10,000, using
cursor pagination rather than deep offsets. It had eight CPUs and 12 GiB against
this engine's one CPU and 256 MiB.

**What is not.**

*Transport is included and is not symmetric.* This engine hands a result to a
parent process over a pipe. Snowstorm serialises JSON and returns it in pages
over HTTP. Much of the difference on large results is serialisation and round
trips, not evaluation. That measures what it costs to get a code set out of each
system, which is the question here, not whose set algebra is faster. HTTP is
Snowstorm's only interface, so there is no version of this comparison without
it.

*The corpus is ours.* The ratios below are a property of this workload. The
1,000-expression and 10,000-expression corpora have the same median result size,
one concept, and differ almost entirely in the tail: two results over 50,000
concepts against 107. That tail moves the ratio. On a workload of mostly small
expansions the gap is closer to the count figure.

*The products are not equivalent.* Snowstorm is a terminology server with
search, FHIR endpoints, branch management, authoring and several versions loaded
at once, answering over a network interface other clients can share. This engine
evaluates ECL against one frozen release, in your process, for you alone.

**What is untested.** Every comparison here is a single sequential client.
Snowstorm's extra CPUs would matter under concurrent load.

## Setting up and serving one release

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/footprint-dark.svg">
  <img alt="Index on disk: this engine 152 MiB, Snowstorm Lite 483 MiB, Snowstorm 6.11 GiB. Reading the release and building indexes: 2.3, 17.6 and 72.7 minutes. Memory allocated: 256 MiB, 2 GiB and 12 GiB." src="images/footprint-light.svg">
</picture>

| | This engine | Snowstorm Lite 2.7.0 | Snowstorm 11.0.0 |
|---|---:|---:|---:|
| Index on disk | 152 MiB packed | 483 MiB | 6.11 GiB Elasticsearch |
| Read the release and build the indexes | 138 s | 1,057 s | 4,360 s |
| Memory allocated to answer queries | 256 MiB | 2 GiB | 12 GiB (two services) |
| Architecture | Rust library or CLI | Java service with Lucene | Java service plus Elasticsearch |

Reading the release happens once, before any query, and produces an index that
never changes. The engine's import ran on two CPUs and 3 GiB and peaked at
2.3 GB; packing took a further 98 s on two CPUs. Allocations are what each run
was given, not measured minimums. Snowstorm's figure counts its own service and
Elasticsearch together. Index contents differ: this engine's holds
descriptions, labels, typed member tables, a word index and a history section;
the servers hold their own search structures.

Evidence: [`release-measurements.json`](../validation/release-measurements.json)
for the engine, [`snowstorm-import-evidence.json`](../validation/snowstorm-import-evidence.json)
for Snowstorm.

## Counting and enumerating

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/latency-dark.svg">
  <img alt="Warm count median: this engine 0.86 ms on 1 CPU and 256 MiB, Snowstorm Lite 4.56 ms on 1 CPU and 2 GiB, Snowstorm 13.19 ms on 8 CPUs and 12 GiB. Complete enumeration median: 0.94 ms, 7.15 ms and 36.40 ms on the same allocations." src="images/latency-light.svg">
</picture>

The 1,000-expression corpus, each server compared on the expressions where both
returned the same code set. The engine ran on one CPU and 256 MiB and peaked at
141 MiB; Snowstorm Lite on one CPU and 2 GiB; Snowstorm on eight CPUs and 12 GiB
across its service and Elasticsearch.

| | Engine median | Engine p95 | Server median | Server p95 |
|---|---:|---:|---:|---:|
| Snowstorm, warm count | 0.86 ms | 1.92 ms | 13.19 ms | 41.63 ms |
| Snowstorm, enumeration | 0.94 ms | 2.11 ms | 36.40 ms | 98.18 ms |
| Lite, warm count | 0.85 ms | 1.40 ms | 4.56 ms | 35.40 ms |
| Lite, enumeration | 0.93 ms | 1.67 ms | 7.15 ms | 58.24 ms |

As ratios of medians:

| | Count | Enumeration |
|---|---:|---:|
| Faster than Snowstorm by | 15.3x | 38.8x |
| Faster than Snowstorm Lite by | 5.3x | 7.7x |

Summed over each matched set of expressions rather than per expression,
enumeration takes 1.07 s against Snowstorm's 101.6 s (95x) and 0.73 s against
Lite's 22.8 s (31x). The totals differ from the medians because the largest
results dominate them.

Timings include transport: JSONL to a child process for the engine, loopback
HTTP with paging for the servers. Five seeded shuffled batches, no result cache,
file cache not dropped. p95 describes this corpus, not a general workload.
Counts are pooled over every sample; enumeration is one timing per expression.

Evidence: [`engine-1000-results.json`](../validation/engine-1000-results.json),
[`snowstorm-1000-results.json`](../validation/snowstorm-1000-results.json) and
[`lite-1000-results.json`](../validation/lite-1000-results.json).

### How the cost grows with the answer

One ratio depends on which expressions you picked, so this walks a ladder of 15
expressions log-spaced by result size, from one concept to every active concept
in the release. Every rung returned identical code sets from both engines.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/expansion-scaling-dark.svg">
  <img alt="Cost of a complete expansion against concepts returned, both axes logarithmic. This engine runs from about 0.8 ms at one concept to 0.33 s at 839,000. Snowstorm asked for the first time runs from about 35 ms to 30 s; once cached, from about 30 ms to 2 s." src="images/expansion-scaling-light.svg">
</picture>

| Concepts returned | This engine | Snowstorm, first ask | Snowstorm, cached | First ask vs engine |
|---:|---:|---:|---:|---:|
| 1 | 0.76 ms | 36.0 ms | 31.8 ms | 47x |
| 3 | 0.77 ms | 31.4 ms | 28.0 ms | 41x |
| 10 | 0.76 ms | 35.2 ms | 28.9 ms | 46x |
| 30 | 0.71 ms | 69.1 ms | 23.1 ms | 97x |
| 104 | 0.81 ms | 89.4 ms | 30.3 ms | 110x |
| 305 | 0.83 ms | 114 ms | 30.1 ms | 137x |
| 1,020 | 1.12 ms | 147 ms | 36.6 ms | 131x |
| 2,803 | 2.52 ms | 298 ms | 28.1 ms | 118x |
| 9,301 | 4.46 ms | 821 ms | 37.6 ms | 184x |
| 29,978 | 10.4 ms | 995 ms | 86.3 ms | 95x |
| 43,647 | 14.4 ms | 1.6 s | 133 ms | 112x |
| 71,560 | 22.8 ms | 2.5 s | 205 ms | 108x |
| 92,718 | 34.7 ms | 3.3 s | 230 ms | 95x |
| 232,007 | 83.6 ms | 8.2 s | 555 ms | 98x |
| 838,955 | 334 ms | 30.2 s | 2.0 s | 90x |

Snowstorm caches an expansion once asked for it, and its second answer is about
15 times quicker. The run restarts Snowstorm first so every rung's first sample
is cold, and leaves out `<< 404684003` because the provenance check queries it
as a sentinel and would warm that rung. This engine has no result cache, so its
column is what every query costs. Past about 30,000 concepts, its time goes on
writing and reading the codes rather than finding them.

Which Snowstorm column applies depends on your workload. Expanding many
different definitions once each, as a code list conversion does, pays the
first-ask cost every time. Re-expanding the same definitions pays the cached
cost.

Evidence: [`expansion-size-results.json`](../validation/expansion-size-results.json),
from the ladder in [`expansion-ladder.json`](../validation/expansion-ladder.json).

### The 10,000-expression corpus

A comparison with Snowstorm over the [10,000-expression
corpus](../validation/ecl-10000.json) was stopped after 3,301 expressions: it was
on course for six hours, and the ladder above answers the same question better.
The partial results are in
[`snowstorm-10000-results.json`](../validation/snowstorm-10000-results.json):
2,612 matching code sets, one disagreement, 433 expressions Snowstorm could not
parse or return, and 255 it refused because they name a concept inactive on its
branch.

That corpus holds 107 expressions returning more than 50,000 concepts, against
two in the 1,000. Treat it as a stress test of enumeration rather than a
representative workload.

## How much of the language each engine ran

Same 1,000 expressions:

| Outcome | vs Snowstorm | vs Lite |
|---|---:|---:|
| Code sets match | **879** | **587** |
| Server declares the feature unsupported | 0 | 320 |
| Server's parser rejects the expression, or it cannot return the result | 120 | 80 |
| Code sets disagree | 1 | 13 |

Snowstorm Lite answers HTTP 501 `not-supported` and names the feature: attribute
group (80), concept filter (40), description filter (40), member filter (40),
attribute cardinality (40), reverse flag (40) and concrete value comparison
operators (40). Of Snowstorm's 120, its parser rejects 80 at the first `!` of
ECL 2.3 top and bottom, and its concept endpoint cannot return the 40
member-field projections.

### Where this engine is slower

Snowstorm enumerated faster on none of the 879 matching expressions, and Lite on
2 of its 587. Each was the first query of its kind in the process: 33 ms to load
the history section and 38 ms to build the attribute index that refinements use,
against about 4 ms for Lite. Later queries of both kinds take about 1 ms.

The first description filter over a focus of more than 1,000 concepts is the
slowest such load. It loads the whole description index, about 0.74 s on one
core, and later filters take about 2 ms. A smaller focus reads only its own
concepts' descriptions.
That matters most in a serverless function, where every invocation is a new
process. It is in the [roadmap](roadmap.md).

### The disagreements

Against Snowstorm, one: `(<< 377442002) : 1142138002 != #10`. The RF2 concrete
relationship rows settle it. The concept has two active values for attribute
1142138002 in this release:

```text
active=1  group=1  value=#20
active=1  group=2  value=#10
```

One of those is not 10, so the concept satisfies `!= #10`. This engine returns
it, and so does Ontoserver, with the same result for `= #10` and `> #10` because
a different row satisfies each. Snowstorm returns an empty set, reading the test
as "has no value equal to 10" rather than "has a value that is not 10". The probe
is in
[`ontoserver-concrete-inequality.json`](../validation/ontoserver-concrete-inequality.json).

Against Lite, thirteen, all attribute inequality refinements of the form
`X : attribute != value`. Lite returns an empty set for each. Snowstorm agrees
with this engine on all thirteen, including cases with 477 and 123 concepts, so
these are answers Lite gets wrong rather than a semantic disagreement. Lite
returns them as successes rather than the 501 it uses for unsupported features.
This applies to the version tested.

## Engine-only measurements

| Workload | Result |
|---|---:|
| 10,000-expression corpus through the CLI, one CPU and 320 MiB | 11.5 s per batch, 0.89 ms median request, 266 MiB peak |
| Same corpus through the library, one CPU and 512 MiB | 1.98 s per batch, 276 MiB peak |
| Same, two CPUs and two workers | 1.47 s per batch, 286 MiB peak |
| Same, four CPUs and four workers | 2.27 s per batch, 299 MiB peak |
| Query-only executable, `--no-default-features` | 2,644,864 B (2.52 MiB), 1,138,545 B gzipped |
| Default executable, with the RF2 importer | 3,445,560 B (3.29 MiB), 1,505,048 B gzipped |
| With `--features unicode` for term matching | 36,202,864 B (34.5 MiB), 14,299,445 B gzipped |
| Process start, open a packed index and answer one query | 163 ms |

Every run returned the recorded code set for every expression, including term
matching, which uses the ICU build.

The CLI figure includes writing every code as JSON and the harness reading it
back; the library figure does not. The 10,000-expression corpus loads every
semantic index, including all description metadata, so it needs 320 MiB where
the 1,000-expression corpus peaks at 141 MiB.

More workers do not help this corpus: two workers are 26% faster than one, and
four are slower than two. That is in the [roadmap](roadmap.md).

Statically linking ICU costs about 31 MiB of executable. Only description term
predicates need it; metadata-only description filters and every other feature
work in the smaller builds.

Evidence: [`engine-10000-results.json`](../validation/engine-10000-results.json),
[`ecl-10000-scaling-results.json`](../validation/ecl-10000-scaling-results.json)
and [`release-measurements.json`](../validation/release-measurements.json).

## Starting cold

Opening an index is the cost a serverless invocation pays before it can answer
anything. `examples/open_breakdown.rs` measures it on one CPU with the index on
the container's own filesystem and in the file cache, taking the median of
three rounds:

| | Uncompressed | Packed |
|---|---:|---:|
| Open, which a query pays | **78 ms** | **156 ms** |
| Full semantic validation, which only `verify` pays | +74 ms | +88 ms |

Opening reads the core, decodes it, and checks that every stored offset and
reference is inside its array. It does not hash the section or re-derive the
semantic invariants: that IDs are sorted, that the two hierarchy directions
agree, that the graph is acyclic. Import proves those before publishing an
index. `verify` hashes every section and re-runs the semantic checks on demand.

The packed layout costs about 78 ms more to open, because zstd decodes 14.6 MiB
into the 86 MiB core. It is 152 MiB on disk against 903 MiB uncompressed. Which
way that trades depends on whether the file is already local or fetched at each
cold start. When it is fetched, packed wins: at 136 MiB/s, reading the whole file
takes 1.1 s packed against 6.6 s uncompressed.

Measure with the index on a local filesystem, not a network or virtual machine
share, which can dominate every figure above.

## Reproducing

```sh
# Correctness and latency across a fixed corpus, engine only.
python scripts/benchmark_corpus.py --output OUT.json \
  --corpus validation/ecl-1000.json --store-directory data/index

# Against Snowstorm, or Snowstorm Lite; one server per run.
python scripts/benchmark_corpus.py --output OUT.json \
  --corpus validation/ecl-1000.json --store-directory data/index \
  --snowstorm http://127.0.0.1:18082 --import-report validation/snowstorm-import-evidence.json
python scripts/benchmark_corpus.py --output OUT.json \
  --corpus validation/ecl-1000.json --store-directory data/index \
  --lite http://127.0.0.1:18081/fhir

# How the cost grows with the size of the answer.
python scripts/benchmark_expansion_size.py --output OUT.json \
  --store-directory data/index \
  --snowstorm http://127.0.0.1:18082 \
  --import-report validation/snowstorm-import-evidence.json

# The library across worker counts, and packing time.
python scripts/benchmark_scaling.py --output OUT.json --store data/uk.ecl
python scripts/benchmark_pack.py --store data/index --destination OUT.ecl --output OUT.json
```

The engine side runs in the pinned `rust:1.93.1-bookworm` image, with Linux
executables built into `target/linux-core` and `target/linux-unicode`; see
[developer setup](setup.md#build-in-docker). Restart Snowstorm before a ladder
run so its ECL cache is empty. Running both servers at once distorts the
timings, and the corpus harness refuses it. Setting up the servers is in
[developer setup](setup.md#comparison-servers).

The harness pins the release, executable, resource limits and code set digests,
and will not compare unless the server advertises the same edition and passes
two release sentinels. A query only receives paired timings when both engines
returned the same code set. Unsupported features, parser rejections and
disagreements are recorded separately and never counted as speed.

`scripts/verify_corpus.py` is the quick correctness check: it runs a corpus
through `batch` and compares every code set with a recorded report.
`scripts/generate_corpus.py` built the corpora from the pinned release, and the
`scripts/check_*_rf2.py` scripts check result sets against the RF2 files
directly. Charts are drawn by `scripts/make_charts.py` from the values on this
page.

## What these numbers are not

They are fixed workloads on one host, not a conformance score and not a
general-purpose ranking. Comparison servers were given their own containers and
their default caches. Index contents differ. Nothing here measures a cloud cold
start or an object-storage download.
