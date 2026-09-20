# Description data and filters

The importer retains all Snapshot descriptions and text definitions, including inactive rows. It stores description IDs, concept ordinals, module, type, effective time, active status, language, complete UTF-8 terms and active language memberships. Preferred displays remain a separate lookup.

`descriptions.bin` uses format `SNDES001`, either as a standalone file or inside
the compressed container. Its component bytes and checksums remain compatible.
Opening the numeric store does not read description data. The first description
query verifies the section, compacts metadata and retains a reader for the text.
Callers can preload metadata with `store.descriptions.get()`.

Modules, types, dates and language/status flags use adaptive dictionaries in
memory. Repeated dialect-membership combinations share one stored list. Columns
with too many distinct values retain wider representations, so the UK release's
small dictionaries do not become limits on other editions.

Term text remains on disk. A shared reader fetches a 64 KiB window, extended when
a term crosses its boundary or exceeds that size. Memory follows the largest
requested term instead of the whole text column. The reader checks UTF-8 and
description boundaries in a streaming pass when the index opens. Compressed
blocks retain their checksum checks. Missing or corrupt data returns an error.

`DescriptionIndex::term(row)` now returns `Result<String>` because text access
can fail. `with_term(row, callback)` avoids allocating a separate string and is
used by the evaluator. Its callback runs while the text-reader lock is held and
must not re-enter that reader. Term work limits are checked before fetching the
text. Metadata predicates do not fetch term windows after initial validation.

Writing the loaded index expands these dictionaries back to the existing format
without changing bytes. This step reduces query memory; a persistent encoding
with compact columns and a term-posting index remains future work.

## Compact runtime measurements

The [runtime record](../validation/description-stream-results.json) uses the same
303,566,848-byte complete packed UK index as the preceding container build.
Neither component bytes nor result sets changed. The executable includes import,
Unicode matching and block compression.

| Workload | Memory limit | Charged peak | Median request | Median batch |
|---|---:|---:|---:|---:|
| Previous 1,000-query corpus | 1 GiB | 542.6 MiB | 1.87 ms | 3.05 s |
| Compact runtime, same corpus | 1 GiB | 219.4 MiB | 1.96 ms | 3.10 s |
| Compact runtime, same corpus | 256 MiB | 217.8 MiB | 2.09 ms | 3.22 s |

All 1,000 complete result sets match the previous build in both new runs. The
256 MiB run's per-request p95 is 13.77 ms. Timings include local JSONL transport;
five seeded shuffled batches reuse one process on one CPU. Filesystem caches
were retained and another build could run on the shared host. These observations
establish the memory reduction, not a controlled throughput improvement.

Ten separate English term probes also matched their independent RF2 sets at one
CPU and 256 MiB, with a 217.2 MiB charged peak. The broad mixed `gas`/`*itis` query
took 1,149 ms median warm evaluation, or 4.02 seconds for its first evaluation
including description loading. It still scans descriptions. The known scope and
negation differences with OneLondon's Ontoserver remain unchanged. Eighteen
metadata probes matched the independent RF2 scan, and `verify` checked all 587
sections, including 7,177,107 typed member rows.

These workloads do not exercise every large member table or arbitrary combinations
of text and member queries. Typed member tables still load in full when selected.
The text window grows for long terms, and its lock serialises text callbacks on
one shared reader. Whole-section checksums and streaming UTF-8 validation still
run at first description access. Persistent compact columns, term postings and
bounded member-table loading remain required work.

The following sections retain the earlier build measurements for comparison.

## Supported predicates

Description filters support `active`, `moduleId`, `effectiveTime`, `language`, `id`, `type`, `typeId`, `dialect` and `dialectId`, including equality, inequality and applicable value sets. Effective time supports ordered comparisons. Type and module IDs can use nested concept expressions. Dialects can constrain acceptability per refset or through a shared set. The built-in aliases are `en-gb` and `en-us`; use `dialectId` or [configured aliases](aliases.md) for other refsets.

```text
<<195967001 {{D type=fsn}}
<<195967001 {{D dialect=en-gb (prefer)}}
* {{D active=0, language=en}}
```

Predicates within one block must match the same description. Separate blocks may match different descriptions of one concept. Active descriptions are the default for each block unless it contains an explicit active predicate. Language membership predicates use active language-member rows.

Term predicates support word prefixes in any order, whole-term wildcards, escaped literal asterisks, sets of search terms and `!=`. Enable the optional `unicode` Cargo feature to evaluate them. Without it, term predicates return an explicit unsupported error, including when the candidate set is empty.

## Build with Unicode term matching

On Debian or Ubuntu, install `build-essential`, `pkg-config` and `libicu-dev`, then build:

```sh
cargo build --locked --release --features unicode
./target/release/snomed-ecl-engine expand INDEX_DIRECTORY '< 64572001 {{ term = (match:"gas" wild:"*itis")}}' --count

# Embed the query engine without the offline importer.
cargo build --locked --release --no-default-features --features unicode
```

The backend requires ICU4C development libraries version 72 or later, including static libraries discoverable through `pkg-config`, and a C compiler. Linux with ICU4C 72.1 is tested. Native Windows and macOS Unicode builds still need validation. The pinned Rust Bookworm Docker image includes the Linux prerequisites.

Matching uses the description's language, ICU word boundaries and asymmetric secondary-strength collation with canonical normalisation. This follows the [ECL collation guidance](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/design/5-syntax-specification#character-collation-for-term-filters). An unaccented English query can match an accented term; accents supplied in the query remain significant. Swedish and Danish tailoring have synthetic checks. The ICU version is available as `text::COLLATION_VERSION`; record it with benchmark results.

The safe Rust wrapper owns each search pattern. Its small C shim resets borrowed target references before returning. Search objects are local to a query, compiled when needed and reused across candidate descriptions. Other Rust modules continue to reject unsafe code. The static ICU data adds about 31 MiB to the executable; reducing that package cost remains work.

`term != "gas"` selects a concept if an eligible description fails to match, even if another description does match. To exclude every concept with a matching description, use `MINUS`. Definitions participate alongside synonyms and FSNs; add `type=(syn fsn)` when the intended scope is names only.

## Term comparison evidence

The [term checks](../validation/term-results.json) compare nine fixed English probes with OneLondon's Ontoserver 6.25.4 and an independent RF2 scan. Seven match Ontoserver's complete sets. Two differences are retained:

- The mixed `gas`/`*itis` query returns 5,609 concepts in Rust and the independent scan. Ontoserver returns 5,493. Restricting Rust to synonyms and FSNs reproduces Ontoserver's complete set; all 116 extra concepts match through text definitions.
- `<<195967001 {{term!="allergic"}}` returns 91 concepts in Rust and the scan, versus 82 in Ontoserver. The nine extra concepts have active descriptions without the prefix. A synthetic regression checks the per-description negation required by the [ECL behaviour specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.8-description-filters#filters-with-negation).

The additional type-scoped query also matches the independent scan. These ten checks cover fixed English patterns, not general Unicode conformance. Locale and accent behaviour have separate synthetic tests. Broader collation and lexical cases remain part of full-language acceptance.

```sh
python scripts/check_terms_rf2.py --archive RF2_ZIP --store STORE_DIRECTORY --reference data/validation/ontoserver-terms/ontoserver.json --output data/validation/term-check.json
```

Warm timings in the record measure engine evaluation after descriptions have loaded. The first broad request includes lazy loading. This initial implementation scans candidate descriptions; a text search index is still needed for fast broad expansions.

| Term-build measurement | Result |
|---|---:|
| Linux CLI with import and static ICU4C 72.1 | 32.64 MiB; 12.86 MiB gzipped |
| Mixed `gas`/`*itis` query, median warm evaluation | 901 ms |
| Same query restricted to synonyms and FSNs | 895 ms |
| First mixed-query evaluation, including lazy description loading | 5.79 seconds |

The [Unicode-build corpus check](../validation/unicode-corpus-results.json) preserves all 920 previously evaluated result sets and 80 unsupported cases. Median warm batch time is 1.892 seconds and median request time is 1.75 ms, including local JSONL transport. Container-charged peak memory is 543.6 MiB under a 1 GiB cap. This corpus contains description metadata predicates but no term predicates; the separate term checks above exercise the matcher. These figures do not establish full-ECL or serverless cold-start costs.

## UK release validation

The [recorded checks](../validation/description-metadata-results.json) use UK Monolith 42.5.0, effective 26 August 2026.

| Measurement | Result |
|---|---:|
| Descriptions and text definitions | 3,562,700 |
| Active descriptions and definitions | 2,699,081 |
| Active language memberships | 5,606,864 |
| Description file | 394,241,234 bytes, 375.98 MiB |
| Complete import, including numeric indexes and displays | 83.56 seconds |
| Import allocation | 2 CPUs, 2 GiB |
| Independent complete-set RF2 checks | 18 matches |

The import time is one local Windows/Docker run. Core, display and concept-membership checksums are unchanged from the preceding import. The RF2 checker independently traverses active inferred relationships and scans descriptions, definitions and language rows. Rust query validation used one CPU and 1 GiB; its first query includes loading the description file. This is not evidence that description queries fit in 256 MiB.

OneLondon's Ontoserver rejected all 18 metadata probes. The first diagnostic confirmed `TypeFilter not supported`. The [rejection record](../validation/ontoserver-description-metadata.json) claims no external matches. Synthetic tests cover shared-description semantics, inactive rows, value sets, nested expressions, dialect acceptability, limits, lazy loading, corruption and supplementary remapping.

Adding PCD 63.0.0 to this base took 20.82 seconds in a separate local run. The combined description file is 394,688,869 bytes, with 3,564,824 descriptions and 5,608,984 language memberships. The loader remaps base ordinals and validates new descriptions before publishing the combined store.

```sh
python scripts/check_descriptions_rf2.py --archive RF2_ZIP --store STORE_DIRECTORY --output data/validation/description-check.json
```

The [expanded corpus](../validation/description-corpus-results.json) evaluates 920 of 1,000 expressions at one CPU and 1 GiB. Its median warm batch took 1.858 seconds; container-charged peak memory was 536.84 MiB. All 880 preceding result digests are unchanged. The same executable evaluated the 880 numeric-store cases at one CPU and 256 MiB. A [recorded description query](../validation/descriptions-256m-attempt.json) exceeded 256 MiB and was killed. These results make description compression and bounded loading explicit optimisation targets.
