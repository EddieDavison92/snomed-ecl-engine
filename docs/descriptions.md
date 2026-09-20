# Description data and filters

The importer retains all Snapshot descriptions and text definitions, including inactive rows. It stores description IDs, concept ordinals, module, type, effective time, active status, language, complete UTF-8 terms and active language memberships. Preferred displays remain a separate lookup.

`descriptions.bin` uses format `SNDES001`. Columns hold fixed-width IDs and ordinals, per-concept row offsets, term offsets, a UTF-8 term buffer and language-member pairs. The manifest records its checksum, byte size and counts. Opening the numeric store does not open this file. The first description query loads, verifies and retains it; callers can preload it with `store.descriptions.get()`.

The initial reader loads the complete description file. It is not yet a compressed search index. Missing or corrupt declared data returns an index error. Older stores without description metadata return an unsupported-feature error until reimported.

## Supported predicates

Description filters support `active`, `moduleId`, `effectiveTime`, `language`, `id`, `type`, `typeId`, `dialect` and `dialectId`, including equality, inequality and applicable value sets. Effective time supports ordered comparisons. Type and module IDs can use nested concept expressions. Dialects can constrain acceptability per refset or through a shared set. The built-in aliases are `en-gb` and `en-us`; use `dialectId` for other refsets. Configurable aliases remain required work.

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
