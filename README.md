# SNOMED Rust ECL engine

A private personal project to build a fast, lightweight ECL engine over a published SNOMED CT RF2 snapshot.

The first deliverable is a Rust library and CLI. Hosting, FHIR, authoring and classification are outside the current scope. No engine has been implemented yet.

- [Implementation plan](docs/plan.md)
- [Research and reference code](docs/research.md)
- [Snowstorm indexing analysis](docs/indexing-research.md)
- [Benchmark protocol](docs/benchmarks.md)
- [Local setup](docs/setup.md)
- [Preparation results and local import status](docs/baseline-status.md)
- [Release manifest](docs/release.json)
- [OneLondon baseline](validation/ontoserver-baseline.json)

The local dataset is the UK Monolith Snapshot, release 42.5.0, effective 26 August 2026 and published 2 September 2026. Its SHA-256 was checked against TRUD. The Monolith combines UK clinical and drug content with the International dependency; package contents are inventoried locally before implementation.

RF2 data and generated indexes stay in ignored `data/`. Reference checkouts stay in ignored `references/`, with their commits recorded in [references.json](docs/references.json). The repository contains no TRUD key or raw credential-bearing download URLs.
