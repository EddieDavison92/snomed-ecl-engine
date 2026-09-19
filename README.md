# SNOMED Rust ECL engine

A private personal project to build a fast, lightweight ECL engine over a published SNOMED CT RF2 snapshot.

Full ECL 2.3 support is a core requirement. The project is complete only when the full language has implementation and conformance evidence; subsets are intermediate milestones.

The prototype is a Rust library and CLI that imports an immutable numeric store and parses basic ECL: literals, wildcard, hierarchy operators and Boolean sets. It preserves relationship groups and concrete values for the next evaluator stage. Displays are optional lookups in a separate file. Refinements and the rest of the language remain required work. Hosting, FHIR, authoring and classification are outside the current scope.

- [Implementation plan](docs/plan.md)
- [Compact store prototype and commands](docs/compact-store.md)
- [Basic ECL commands and validation](docs/basic-ecl.md)
- [Full ECL acceptance checklist](docs/conformance.md)
- [Research and reference code](docs/research.md)
- [Snowstorm indexing analysis](docs/indexing-research.md)
- [Benchmark protocol](docs/benchmarks.md)
- [Local setup](docs/setup.md)
- [Preparation results and local import status](docs/baseline-status.md)
- [Release manifest](docs/release.json)
- [OneLondon baseline](validation/ontoserver-baseline.json)

The local dataset is the UK Monolith Snapshot, release 42.5.0, effective 26 August 2026 and published 2 September 2026. Its SHA-256 was checked against TRUD. The Monolith combines UK clinical and drug content with the International dependency; package contents are inventoried locally before implementation.

RF2 data and generated indexes stay in ignored `data/`. Reference checkouts stay in ignored `references/`, with their commits recorded in [references.json](docs/references.json). The repository contains no TRUD key or raw credential-bearing download URLs.
