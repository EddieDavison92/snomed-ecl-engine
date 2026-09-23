# Benchmarks

These pages measure two workloads separately, because their costs differ.

**Counting** asks for the size of a result. Snowstorm answers with the total and
at most one concept identifier.

**Enumerating** asks for every concept in the result, which is what building a
codelist, an export or a value set needs. Snowstorm returns those concepts in
pages of up to 10,000, so a 65,000-concept result costs seven HTTP round trips
and 65,000 serialised entries. This engine returns the set from memory in one
response.

The two cost this engine 0.78 ms and 0.93 ms. They cost Snowstorm 13.19 ms and
36.40 ms. Quoting a count latency for an enumeration workload would understate
Snowstorm's by a factor of three, so the tables below keep them apart.

## Is this a fair comparison?

This engine is not trying to be Snowstorm, and nothing here is a verdict on
Snowstorm. The comparison answers one narrower question: if you were going to
batch-expand ECL through a terminology server's API, what does that cost, and
what does doing the same work in your own process cost instead? That is a real
choice somebody has to make, so it is worth measuring.

Within that question, here is what holds and what does not.

**What is fair.** Both engines answer the same expressions against the same
release, gated by four checks before any timing runs: the store manifest and the
recorded import agree on edition and archive checksum, the server advertises
that edition, its branch head is unchanged since the import completed, and two
sentinel queries return exact expected counts. Only expressions where both
returned identical complete code sets receive paired timings. Snowstorm is asked
for its cheapest form, `returnIdOnly=true`, at its own maximum page size of
10,000, using cursor pagination rather than deep offsets. It also had eight CPUs
and 12 GiB against this engine's one CPU and 256 MiB.

**What is not.** Three things.

*Transport is included and is not symmetric.* This engine hands a result to a
parent process over a pipe. Snowstorm serialises JSON and returns it in pages
over HTTP. Much of the difference on large results is serialisation and round
trips, not evaluation. That is a fair measure of what it costs to get a complete
code set out of each system, which is the question here, but it is not a measure
of whose set algebra is faster. HTTP is Snowstorm's only interface, so there is
no version of this comparison without it.

*The corpus is ours.* The multiples above are a property of this workload. The
1,000-expression corpus and the 10,000-expression corpus have the same median
result size, one concept, and differ almost entirely in the tail: two results
over 50,000 concepts against 107. That tail is what moves the ratio. On a
workload of mostly small expansions the gap is closer to the count figure.

*The products are not equivalent.* Snowstorm is a terminology server with
search, FHIR endpoints, branch management, authoring and multiple versions
loaded at once, and it is answering over a network interface that other clients
can share. This engine evaluates ECL against one frozen release, in your
process, for you alone. Being quicker at that one operation says nothing about
the rest.

**What is untested.** Every measurement here is a single sequential client.
Snowstorm's extra CPUs would matter under concurrent load. `batch --workers N`
now answers concurrent requests from one index, but neither side has been
benchmarked under concurrency.

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
never changes. Allocations are what each run was given, not measured minimums. Snowstorm's
figure counts its own service and Elasticsearch together. Index contents are not
identical: ours holds descriptions, displays, typed member tables, a word index
and a history section; the servers hold their own search structures.

The engine's figures were re-measured on 23 September 2026 with the current
importer, which also builds the word index and history section, at the same
allocation as before: two CPUs and 3 GiB for the import, which peaked at 2.3 GB.
The index shrank from 389 MiB to 152 MiB that day without changing query
latency; [how the index is built](index-format.md#where-the-size-went) lists each
step. The servers' figures are from their original runs and were not repeated.

## Counting and enumerating

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/latency-dark.svg">
  <img alt="Warm count median: this engine 0.78 ms on 1 CPU and 256 MiB, Snowstorm Lite 4.56 ms on 1 CPU and 2 GiB, Snowstorm 13.19 ms on 8 CPUs and 12 GiB. Complete enumeration median: 0.93 ms, 7.15 ms and 36.40 ms on the same allocations." src="images/latency-light.svg">
</picture>

The 1,000-expression corpus, each server compared on its own matched cohort.
The engine ran on one CPU and 256 MiB, peaking at 144 MiB, Snowstorm Lite on one
CPU and 2 GiB, and Snowstorm on eight CPUs and 12 GiB across its service and
Elasticsearch. The engine was re-run on 23 September 2026 and returned, for every
expression in each cohort, the code set that server had matched; the servers'
timings are from their original runs:

| | Engine median | Engine p95 | Server median | Server p95 |
|---|---:|---:|---:|---:|
| Snowstorm, warm count | 0.78 ms | 1.65 ms | 13.19 ms | 41.63 ms |
| Snowstorm, complete enumeration | 0.93 ms | 2.22 ms | 36.40 ms | 98.18 ms |
| Lite, warm count | 0.78 ms | 1.25 ms | 4.56 ms | 35.40 ms |
| Lite, complete enumeration | 0.94 ms | 1.62 ms | 7.15 ms | 58.24 ms |

As ratios, comparing medians on each matched cohort:

| | Count | Complete enumeration |
|---|---:|---:|
| Faster than Snowstorm by | 16.9x | 39.3x |
| Faster than Snowstorm Lite by | 5.9x | 7.6x |

Summed over a whole cohort rather than per expression, enumeration is 96x
against Snowstorm and 31x against Lite. The per-expression median and the
cohort total differ because the total is dominated by the largest results.

The engine's cost moves from 0.78 ms to 0.93 ms between counting and
enumerating, because evaluating the expression already built the whole set.
Both servers roughly triple. Over the matched cohort that is 1.06 s
against 101.6 s for Snowstorm, and 0.73 s against 22.8 s for Lite.

The engine's figures were 2.20 ms and 2.29 ms before operators were made to cost
what they read rather than the size of the edition; see
[costing what a query touches](#costing-what-a-query-touches).

Timings include transport: JSONL to a child process for the engine, loopback
HTTP with paging for the servers. Five seeded shuffled batches, no result cache,
caches not dropped. p95 describes this corpus, not a general workload.

### How the cost grows with the answer

A single multiple depends on which expressions you picked, so this walks a
ladder of 15 expressions chosen to be log-spaced by result size, from one
concept to every active concept in the release. Every rung returned identical
code sets from both engines.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/expansion-scaling-dark.svg">
  <img alt="Cost of a complete expansion against concepts returned, both axes logarithmic. This engine runs from 0.8 ms at one concept to 0.4 s at 839,000. Snowstorm asked for the first time runs from 31 ms to 30 s; once cached, from 28 ms to 2 s." src="images/expansion-scaling-light.svg">
</picture>

| Concepts returned | This engine | Snowstorm, first ask | Snowstorm, cached | First ask vs engine |
|---:|---:|---:|---:|---:|
| 1 | 0.75 ms | 36 ms | 32 ms | 48x |
| 3 | 0.91 ms | 31 ms | 28 ms | 35x |
| 10 | 0.80 ms | 35 ms | 29 ms | 44x |
| 30 | 0.82 ms | 69 ms | 23 ms | 84x |
| 104 | 0.83 ms | 89 ms | 30 ms | 108x |
| 305 | 0.92 ms | 114 ms | 30 ms | 125x |
| 1,020 | 1.2 ms | 147 ms | 37 ms | 118x |
| 2,803 | 1.9 ms | 298 ms | 28 ms | 160x |
| 9,301 | 4.3 ms | 821 ms | 38 ms | 192x |
| 29,978 | 12.5 ms | 995 ms | 86 ms | 79x |
| 43,647 | 19.8 ms | 1.6 s | 133 ms | 81x |
| 71,560 | 26.0 ms | 2.5 s | 205 ms | 94x |
| 92,718 | 36.4 ms | 3.3 s | 230 ms | 90x |
| 232,007 | 97.8 ms | 8.2 s | 555 ms | 84x |
| 838,955 | 378 ms | 30.2 s | 2.0 s | 80x |

Snowstorm caches an expansion once it has been asked for, and the cache is
effective: about 15 times quicker on the second ask. Measuring without
separating the two conflates them, which is what an earlier version of this
ladder did. The run behind this table restarts Snowstorm first so that every
rung's first sample is genuinely cold, and it excludes `<< 404684003` because
the provenance gate queries it as a sentinel and would warm that rung.

This engine has no result cache, so the column above is simply what a query
costs. Its figures were re-measured on 23 September 2026 against the same
ladder, each rung returning the code set Snowstorm had matched; Snowstorm's are
from the original run. Small answers fell from about 2 ms to under 1 ms. Past
about 30,000 concepts the time barely moved, because it is spent writing and
reading the codes rather than finding them.

Which column applies depends on your workload. Expanding many different
definitions once each, as a codelist conversion does, pays the first-ask cost
every time. Re-expanding the same definitions pays the cached cost.

### The 10,000-expression corpus

A full comparison run over the [10,000-expression corpus](../validation/ecl-10000.json)
was started and abandoned after 3,301 expressions. It was on course for roughly
six hours, and the ladder above answers the same question better, because it
shows the shape rather than one ratio. The partial results are kept in
[`snowstorm-10000-results.json`](../validation/snowstorm-10000-results.json) and
are labelled as partial: 2,612 agreeing complete code sets, one disagreement,
433 expressions Snowstorm could not parse or return, and 255 it refused because
they name a concept inactive on its branch.

That corpus holds 107 expressions returning more than 50,000 concepts, against
two in the 1,000. Both have the same median result size of one concept. Treat it
as a torture test for the enumeration path rather than a representative
workload. If your expansions are small, the count figures above are the ones to
read.

## How much of the language each engine ran

Same 1,000 expressions:

| Outcome | vs Snowstorm | vs Lite |
|---|---:|---:|
| Complete code sets match | **879** | **587** |
| Server declares the feature unsupported | 0 | 320 |
| Server's parser rejects the expression | 120 | 80 |
| Complete sets disagree | 1 | 13 |

Snowstorm Lite reports unsupported features honestly, answering HTTP 501
`not-supported` and saying which feature: attribute group (80), concept filter
(40), description filter (40), member filter (40), attribute cardinality (40),
reverse flag (40), and concrete value comparison operators (40). Snowstorm's 120
are its parser rejecting ECL 2.3 top and bottom at the first `!`, plus
member-field projections its concept endpoint cannot return.

### Where this engine is slower

Snowstorm enumerated faster on 2 of the 879, and Lite on 37 of its 587. Two
paths account for nearly all of it.

**The first description-filter query in a process.** It loads the description
index, and that cost lands on whichever query arrives first: 5,940 ms for the
slowest case here. Across all 40 description-filter expressions the median
first run was 1.24 ms and the median warm request 1.13 ms, so this is one load
rather than a per-query cost. It matters most in a serverless function, where
every invocation is a new process.

**History supplements.** These ran at a 15.65 ms median cold and 16.10 ms warm,
so loading is not the cause. Evaluating one scans reference set member rows.
Lite answered the same expressions in about 4 ms.

Since this run, a history section indexes associations from both ends: the 320
history expressions in the 10,000-expression corpus now take a 0.016 ms median.
The description-filter load remains and is recorded in the [roadmap](roadmap.md).

### The disagreements

Against Snowstorm, one: `(<< 377442002) : 1142138002 != #10`. The RF2 concrete
relationship rows settle it. The concept has two active values for attribute
1142138002 in this release:

```text
active=1  group=1  value=#20
active=1  group=2  value=#10
```

One of those is not 10, so the concept satisfies `!= #10`. We return it, and
OneLondon's Ontoserver returns it, with the same result for `= #10` and `> #10`
because a different row satisfies each. Snowstorm returns an empty set, which
reads the test as "has no value equal to 10" rather than "has a value that is
not 10". The probe is recorded in
[`ontoserver-concrete-inequality.json`](../validation/ontoserver-concrete-inequality.json).

Against Lite, thirteen, all attribute inequality refinements of the form
`X : attribute != value`. Lite returns an empty set for each. Snowstorm agrees
with this engine on all thirteen, including cases with 477 and 123 concepts, so
these are answers Lite gets wrong rather than a semantic disagreement. It also
returns them as success rather than the 501 it uses for features it does not
implement. This applies to the pinned version tested.

## Engine-only measurements

No comparison server involved.

| Workload | Result |
|---|---:|
| 10,000-expression corpus, one CPU and 320 MiB | 10.27 s per warm batch, 0.82 ms median request, 268 MiB peak |
| Same corpus, four CPUs and four library workers | 2.15 s per batch, 298 MiB peak |
| Query-only executable, `--no-default-features` | 2,669,936 B (2.55 MiB), 1,152,950 B gzipped |
| Default build, with the RF2 importer | 3,388,216 B (3.23 MiB), 1,476,075 B gzipped |
| With `--features unicode` for term matching | 36,150,560 B (34.48 MiB), 14,289,683 B gzipped |
| Open a packed index | 177 ms |
| Open an uncompressed index | 94 ms |
| Process start, open and answer one query | 177 ms |

### Costing what a query touches

Operators used to pay for the whole edition: subsumption allocated and scanned
edition-sized markers, reverse attributes and dotted projections scanned every
concept, refinements tested every focus concept, and member filters scanned
every row of a reference set. Each now costs what it reads. Evaluation time as
reported by `batch`, median of repeated runs, before and after, on WSL2 on the
development machine; not comparable with the container figures above.

| Workload | Before | After |
|---|---:|---:|
| 1,000-expression corpus, total | 1,470 ms | 28 ms |
| 10,000-expression corpus, total | 18.2 s | 0.62 s |
| `* : 363698007 = << 39057004` | 38.4 ms | 0.02 ms |
| `<< 404684003 : 363698007 = << 39057004` | 11.9 ms | 0.22 ms |
| `<< 373873005 : 127489000 = << 387517004` | 13.7 ms | 0.18 ms |
| `* : R 363698007 = << 195967001` | 11.9 ms | 0.02 ms |
| `<< 195967001 MINUS << 426979002` | 1.25 ms | 0.01 ms |
| `>> 195967001` | 0.64 ms | 0.003 ms |
| 400 `<<` operators joined by `OR` | exceeded the work limit | 1.35 ms |
| Search, warm | 21 ms | 0.5 ms |
| Describe a concept, first in a process | 650 ms | 1.7 ms |

Both corpora return the recorded answers, all 10,000 including term matching
checked with the ICU build.

## Starting cold

Opening an index is the cost a serverless invocation pays before it can answer
anything. `examples/open_breakdown.rs` measures it, on one CPU with the index on
a local filesystem:

| | Uncompressed | Packed |
|---|---:|---:|
| Open, which a query pays | **94 ms** | **177 ms** |
| Full semantic validation, which only `verify` pays | +105 ms | +100 ms |

Opening reads the core, decodes it, and checks that every stored offset and
reference is inside its array. It does not hash the section, and it does not
re-derive the semantic invariants: that IDs are sorted, that the two hierarchy
directions agree, that the graph is acyclic. Import proves those before
publishing an index. `verify` hashes every section and re-runs the semantic
pass on demand, and the split is covered by a test.

Opening used to checksum each section before reading it, so the core was read
twice and, when packed, decompressed twice. Removing that took the packed open
from 347 ms to 177 ms and the uncompressed open from 162 ms to 94 ms, measured
the same way.

The packed layout costs about 83 ms more to open, because zstd decodes 14.6 MiB
into the 86 MiB core. It is 152 MiB on disk against 903 MiB. Which way that
trades depends on whether the file is already local or fetched per cold start.
When it is fetched, packed wins: reading the whole file at 136 MiB/s takes 1.1 s
packed against 6.6 s uncompressed.

Measure with the index on a local filesystem. A Windows bind mount reads at
181 MB/s against 5.6 GB/s for the container's own filesystem, which dominates
every figure above.

[Bringing this down further](roadmap.md#now) is open work.

## Executable size

Statically linking ICU4C costs about 31 MiB of executable. Only description term
predicates need it. Metadata-only description filters and every other feature
work in the 3.2 MiB build.

The concurrency figure shares one index across four workers under a 1 GiB limit
with a 298 MiB charged peak; every complete result set matched. Those are direct
library timings without transport. The CLI answers concurrently with
`batch --workers N`.

The 10,000-expression rows were re-measured on 23 September 2026 with the ICU
build against the current packed index, and every result set matched the
recorded one. They were 35.05 s at one CPU and 256 MiB, and 5.87 s on four
workers. The single-CPU run no longer fits 256 MiB: that corpus loads every
semantic index, including all description metadata, and the peak rose from
227 MiB to 268 MiB. About 23 MiB of that is new in the engine, mostly the
attribute inverse that refinements use, and the rest is the history and word
index sections. The 1,000-expression corpus peaks at 144 MiB.

The table's 10,000-expression row is from that run. A re-run after the index
shrank peaked at 266 MiB and still needs 320 MiB: the peak is decoded sections,
not file cache. Its batches took 11.4 s and its median request 0.89 ms, which
moved with the host rather than the build: interleaved 1,000-expression runs of
the previous and current builds could not be told apart.

## Reproducing

```sh
# How the cost grows with the size of the answer.
python scripts/benchmark_expansion_size.py --output OUT.json \
  --snowstorm http://127.0.0.1:18082 \
  --import-report validation/snowstorm-import-evidence.json

# Correctness and latency across a fixed corpus.
python scripts/benchmark_corpus.py --output OUT.json \
  --corpus validation/ecl-1000.json \
  --store-directory data/compact-store/v1-ecl-completion \
  --snowstorm http://127.0.0.1:18082 --import-report validation/snowstorm-import-evidence.json
python scripts/benchmark_corpus.py --output OUT.json \
  --corpus validation/ecl-1000.json \
  --store-directory data/compact-store/v1-ecl-completion \
  --lite http://127.0.0.1:18081/fhir
```

Restart Snowstorm before a scaling run so its ECL cache is empty, or every rung
after the first measures the cache rather than the work. One server per run:
running both at once distorts the timings, and the corpus harness refuses it.
Starting the containers is in [developer setup](setup.md#comparison-servers).

The harness pins the release, executable, resource limits and complete result
digests, and will not compare unless the server advertises the same edition and
passes two release sentinels. A query only receives paired timings when both
engines returned the same complete code set. Unsupported features, parser
rejections and mismatches are recorded separately and never counted as speed.

Charts are generated by `scripts/make_charts.py` from the values in this page.

## What these numbers are not

They are fixed workloads on one host, not a conformance score and not a
general-purpose ranking. Comparison servers were given their own containers and
their default caches. Index contents differ. Nothing here measures a cloud cold
start, object-storage download, or the memory a full-language workload needs
with every semantic index resident. That last one is open work in the
[roadmap](roadmap.md).
