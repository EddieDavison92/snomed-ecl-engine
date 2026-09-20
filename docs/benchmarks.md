# Benchmarks

Two workloads, measured separately, because they behave differently.

**Counting** — "how many concepts does this expression select?" A terminology
server is built for this, and returns a total plus at most one identifier.

**Enumerating** — "give me every code." This is what building a codelist, an
export or a value set actually needs. A server has to serialise and page the
whole set over HTTP; an embedded engine materialises it in memory.

Keeping them apart matters, because the gap between the two is the difference
between a faster endpoint and a different kind of tool.

## Cost of serving one release

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/footprint-dark.svg">
  <img alt="Index on disk: this engine 290 MiB, Snowstorm Lite 483 MiB, Snowstorm 6.11 GiB. Import time: 2.0, 17.6 and 72.7 minutes. Memory allocated: 256 MiB, 2 GiB and 12 GiB." src="images/footprint-light.svg">
</picture>

| | This engine | Snowstorm Lite 2.7.0 | Snowstorm 11.0.0 |
|---|---:|---:|---:|
| Index on disk | 290 MiB packed | 483 MiB | 6.11 GiB Elasticsearch |
| Import time | 120 s | 1,057 s | 4,360 s |
| Memory allocated to serve | 256 MiB | 2 GiB | 12 GiB (two services) |
| Architecture | Rust library or CLI | Java service with Lucene | Java service plus Elasticsearch |

Allocations are what each run was given, not measured minimums. Snowstorm's
figure counts its own service and Elasticsearch together. Index contents are not
identical: ours holds descriptions, displays and typed member tables; the servers
hold their own search structures.

## Counting and enumerating

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/latency-dark.svg">
  <img alt="Warm count median: this engine 2.20 ms, Snowstorm Lite 4.56 ms, Snowstorm 13.19 ms. Complete enumeration median: 2.29 ms, 7.15 ms and 36.40 ms." src="images/latency-light.svg">
</picture>

The 1,000-expression corpus, one CPU and 256 MiB for the engine, each server
compared on its own matched cohort:

| | Engine median | Engine p95 | Server median | Server p95 |
|---|---:|---:|---:|---:|
| **vs Snowstorm** — warm count | 2.20 ms | 9.97 ms | 13.19 ms | 41.63 ms |
| **vs Snowstorm** — complete enumeration | 2.29 ms | 11.45 ms | 36.40 ms | 98.18 ms |
| **vs Lite** — warm count | 2.22 ms | 13.94 ms | 4.56 ms | 35.40 ms |
| **vs Lite** — complete enumeration | 2.34 ms | 15.95 ms | 7.15 ms | 58.24 ms |

The engine's cost barely moves between counting and enumerating — 2.20 ms to
2.29 ms — because evaluation already produces the whole set and returning it is
a write. Both servers roughly triple. Over the matched cohort that is 9.0 s
against 101.6 s for Snowstorm, and 2.1 s against 22.8 s for Lite.

Timings include transport: JSONL to a child process for the engine, loopback
HTTP with paging for the servers. Five seeded shuffled batches, no result cache,
caches not dropped. p95 describes this corpus, not a general workload.

### Where enumeration stops being practical

The broader [10,000-expression corpus](../validation/ecl-10000.json) includes
expansions large enough that paging them out of Snowstorm takes minutes each, and
a full comparison run did not finish. The engine evaluates that corpus in 35.05 s
per warm batch on one CPU and 256 MiB. This is not a claim that Snowstorm is
slow — it is that repeatedly materialising complete code sets is a workload an
HTTP terminology API is not shaped for.

## How much of the language each engine ran

Same 1,000 expressions:

| Outcome | vs Snowstorm | vs Lite |
|---|---:|---:|
| Complete code sets match | **879** | **587** |
| Server declares the feature unsupported | 0 | 320 |
| Server's parser rejects the expression | 120 | 80 |
| Complete sets disagree | 1 | 13 |

The previous run of this comparison matched 719 of 1,000. The increase is engine
work since — membership, descriptions, history, concept filters, member filters
and projections — not a change to the corpus or the release.

Snowstorm Lite reports unsupported features honestly, answering HTTP 501
`not-supported` and saying which feature: attribute group (80), concept filter
(40), description filter (40), member filter (40), attribute cardinality (40),
reverse flag (40), and concrete value comparison operators (40). Snowstorm's 120
are its parser rejecting ECL 2.3 top and bottom at the first `!`, plus
member-field projections its concept endpoint cannot return.

### The disagreements

Against Snowstorm, one: a concrete inequality, `(<< 377442002) : 1142138002 != #10`.
Our result matches OneLondon's Ontoserver and the RF2 rows.

Against Lite, thirteen, all attribute inequality refinements of the form
`X : attribute != value`. Lite returns an empty set for each. Snowstorm agrees
with this engine on all thirteen, including cases with 477 and 123 concepts, so
these are answers Lite gets wrong rather than a semantic disagreement — and it
returns them as success rather than as the 501 it uses elsewhere. This applies to
the pinned version tested.

## Engine-only measurements

No comparison server involved.

| Workload | Result |
|---|---:|
| 10,000-expression corpus, one CPU and 256 MiB | 35.05 s per warm batch, 1.75 ms median request |
| Same corpus, four CPUs and four library workers | 5.87 s, 0.95 ms median query |
| Query-only executable, `--no-default-features` | 2,231,176 B (2.13 MiB), 955,586 B gzipped |
| Default build, with the RF2 importer | 2,973,128 B (2.84 MiB), 1,290,251 B gzipped |
| With `--features unicode` for term matching | 35,731,536 B (34.08 MiB), 14,106,938 B gzipped |
| Packed index open | 1.13 s |
| Container start to first response | 1.72 s |

Statically linking ICU4C for term matching costs about 31 MiB of executable.
That is the price of description term predicates; metadata-only description
filters, and everything else, need only the 2.8 MiB build.

The concurrency figure shares one index across four workers under a 1 GiB limit
with a 297 MiB charged peak; every complete result set matched. Those are direct
library timings without transport. The CLI is sequential.

## Reproducing

```sh
python scripts/benchmark_corpus.py --output OUT.json \
  --corpus validation/ecl-1000.json \
  --store-directory data/compact-store/v1-ecl-completion \
  --snowstorm http://127.0.0.1:18082 --import-report validation/snowstorm-import-evidence.json
python scripts/benchmark_corpus.py --output OUT.json \
  --corpus validation/ecl-1000.json \
  --store-directory data/compact-store/v1-ecl-completion \
  --lite http://127.0.0.1:18081/fhir
```

One server per run: running both at once distorts the timings, and the harness
refuses it. Starting the containers is in [developer setup](setup.md#comparison-servers).

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
start, object-storage download, or the memory a full-language workload needs with
every semantic index resident — that last one is open work in the
[roadmap](roadmap.md).
