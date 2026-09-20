# Research notes

Checked on 19 September 2026. Exact reference commits are in [references.json](references.json). Upstream documentation describes intended behaviour; it does not substitute for running our own comparisons.

## Release choice

TRUD item 1799 provides the UK Monolith RF2 Snapshot. The authenticated API returned release 42.5.0, effective 26 August and published 2 September 2026. Item 101, the Clinical Edition with Full/Snapshot/Delta, returned 42.4.0, published 5 August. Use the Monolith for the primary dataset because the goal is the current combined UK content and OneLondon's Ontoserver exposes the matching composition edition. Keep archive publication date distinct from edition effective date. [TRUD API documentation](https://isd.digital.nhs.uk/trud/user/guest/group/0/api)

The Monolith download is 609,629,807 bytes. Its verified checksum is in [release.json](release.json). The raw TRUD API response is not retained because its download URLs contain the API key. RF2 contents and detailed inventory remain local.

## Standards

The official specification identifies ECL 2.3 as current. Its syntax chapter defines active components and active refset members as the default and requires explicit grouping for mixed boolean operators. The grammar repository recommends its ANTLR grammar where the ABNF's overlapping filter productions are ambiguous. Pin both grammar and examples; do not implement a parser from examples alone. [ECL specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language), [syntax rules](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/design/5-syntax-specification), [official grammar](https://github.com/IHTSDO/snomed-expression-constraint-language)

## Reference implementations

| Project | What the inspected source provides | Use here |
|---|---|---|
| [Snowstorm](https://github.com/IHTSDO/snowstorm) | Java and Elasticsearch; extensive ECL implementation and tests; Apache-2.0 | Broad local comparison target and semantic edge cases |
| [Snowstorm Lite](https://github.com/IHTSDO/snowstorm-lite) | Self-contained Lucene service; ECL Core subset; Apache-2.0 | First local baseline for common queries and smaller runtime comparisons |
| [snomed-rust](https://github.com/snomed-rust/snomed-rust) | RF2 parser, in-memory store, ECL, FHIR and classifier; MIT OR Apache-2.0; declared Rust minimum 1.96 | Assess RF2/parser reuse and compare semantic coverage |
| [sct](https://github.com/pacharanero/sct) | Rust/SQLite tooling; indexed hierarchy closure or recursive SQL; `BTreeSet<u64>` query results; AGPL-3.0-or-later | Storage and CLI reference, possible independent local baseline |
| [Hermes](https://github.com/wardle/hermes) | Clojure terminology engine using LMDB and Lucene; EPL-2.0 | Study mature immutable storage and ECL handling |

The reference checkouts are ignored rather than vendored. Licence names above are from their checked-out files, not a decision to adopt their code.

Snowstorm Lite 2.7.0 rejects attribute groups and non-default attribute cardinality in its inspected implementation. It cannot validate the whole proposed core. Its README's memory claims are upstream claims, not measurements on our UK release. Use it for shared capabilities and keep unsupported queries visible in reports. Relevant source is under `service/ecl/` in the local checkout.

The inspected `snomed-rust` version is 0.56.0. Its evaluator returns `HashSet<SctId>` and its snapshot builder holds component hash maps. In `evaluate_wildcard`, the self-inclusive cases enumerate `store.concepts()`, which returns all stored concepts rather than `active_concepts()`. A store containing inactive concepts therefore needs explicit attention before reuse. This is a source-level finding, not a measured release-level conformance verdict. Documented gaps include history supplements, member field selection, alternate identifiers and some newer operators.

The inspected `sct` version is 0.25.0 and declares Rust 1.88. It performs set operations in Rust over IDs read from SQLite, with a transitive closure table when available. This is a useful alternative to a custom binary index, but neither its speed nor another project's published benchmark demonstrates the performance of our proposed engine. We have not built either Rust reference project locally.

## Index candidates

Roaring's Rust implementation provides compressed sets of 32-bit integers and operations including intersection, union and difference. Dense internal concept ordinals let us use those operations without storing sparse 64-bit SCTIDs directly in each set. Benchmark this against sorted vectors before adopting it. [roaring-rs](https://github.com/RoaringBitmap/roaring-rs)

The main risk is index amplification: precomputing every descendant set or every attribute/value combination may cost more memory than it saves in latency. The main semantic risk is flattening relationship groups to accelerate attribute matching. Preserve exact relationships first and optimise candidate selection around them.

## OneLondon's Ontoserver validation

The connected service reported Ontoserver 6.25.4 and FHIR 4.0.1. Discovery returned the required UK composition edition. Eight small queries completed with explicit edition confirmation and complete code lists. The tracked [baseline](../validation/ontoserver-baseline.json) contains counts and digests; the lists stay local.

The existing terminology skill's convenience script does not pin a version or retrieve every page. The project wrapper uses its credential helper but adds version selection, edition checks, pagination and duplicate/count checks. This establishes a comparison dataset, not proof that our unimplemented engine is correct.
