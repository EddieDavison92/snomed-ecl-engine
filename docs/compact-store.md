# Compact store prototype

For the current compressed single-file layout and `pack`/`verify` commands, read
[Single-file index](container.md). The measurements below describe the original
directory format; its component bytes remain readable inside the new container.

The prototype keeps expansion data in `core.bin` and selected English displays in `display.bin`. Numeric queries open the core, manifest and membership file when declared. With `--display`, the CLI resolves the code set first, then fetches labels by concept ordinal.

This document records the first storage experiment. The later [basic ECL milestone](basic-ecl.md) adds a parser and set evaluator. Later milestones add [refinements](refinements.md) and [membership](refsets.md). The original `hierarchy` command still exposes the eight hierarchy variants directly for validation.

The finished engine must implement full ECL 2.3. Every pending language feature below is required. The current file sizes exclude semantic indexes still needed for those features and are not a size estimate for the complete engine.

## Stored data

Every concept, including inactive concepts, has a dense `u32` ordinal. The core holds the sorted `u64` SCTID dictionary, module ordinals, effective dates and active/definition flags. The ECL default includes all concepts; hierarchy uses active inferred edges.

The UK dictionary uses 9,212,152 bytes for SCTIDs. Relationships reference four-byte ordinals, so repeated 18-digit codes are not stored in every row. This binary format has no SQL `NUMBER` columns. The current population needs 21 bits per ordinal, making three-byte or bit-packed encodings possible experiments. Their space savings must be measured against decoding cost. Smaller dictionaries for attribute types and compact group numbers are further candidates.

Parent and child adjacency arrays contain the active inferred is-a graph. Attribute arrays are ordered by source, relationship group, type and value. A row uses three `u32` fields; per-concept offsets identify its source. Group numbers survive import, including group zero and groups shared across ordinary and concrete relationships.

Concrete values have a separate dictionary. Numbers retain their original decimal spelling, so import loses no precision. The evaluator compares decimals exactly. Concrete strings remain semantic values in the core even though display text is separate.

| Capability | Status |
|---|---|
| Verified UK Monolith Snapshot import | Implemented |
| Concept metadata and inferred hierarchy | Implemented |
| Eight hierarchy variants via library and CLI | Implemented |
| Grouped attributes and exact concrete spelling | Stored and evaluated; see the refinement milestone |
| Optional English display lookup | Implemented |
| Refset membership and member fields | Active concept membership and lazy typed member tables |
| Basic ECL grammar and Boolean sets | Implemented in the later basic ECL milestone |
| Attribute refinements | Implemented; conformance gaps remain |
| Description, concept and member filters | Implemented; see the full conformance checklist for remaining details |
| History supplements and field projections | Implemented, including typed scalar sets and terminal tuples |
| Full, Delta, multiple packages or incremental updates | Not supported |

Description filters use a separate description index. A preferred display lookup cannot answer queries about synonyms, language, dialect or description metadata. Prefix postings and compressed text remain planned storage experiments.

## Display selection

The default policy chooses an active English synonym marked preferred in the first matching refset:

1. NHS clinical realm: `999001261000000100`.
2. NHS pharmacy realm: `999000691000001104`.
3. GB English: `900000000000508004`.

The fallback is an active English fully specified name, then an active English synonym. The smallest description ID breaks ties. Missing text returns null. A caller can supply a different ordered refset list at import time; the manifest records it. This policy selects one label per concept, not a complete language service.

The display file has `u32` offsets followed by UTF-8 text. Opening it verifies its checksum and loads the offsets. Individual lookups read only the selected text into memory.

## Format and validation

Format 1 uses explicit little-endian integers, length-prefixed sections and separate file magic values. The JSON manifest records the edition, source checksum, module dependency rows, capabilities, counts and file checksums. Readers check lengths before allocating, then validate offsets, references and graph structure. Opening the core verifies its checksum and graph on every call; a future service should retain the opened store.

Import requires exactly one file for each supported Snapshot component and a package date matching the edition URI. It validates unique concepts, active relationship IDs, required concept references, dates, supported modifiers, active graph endpoints and acyclicity. It records dependency versions and requires the edition composition dependency. It does not yet prove every module's dependency version against every component.

The importer writes into a new sibling directory and publishes it by rename after writing checksums. Existing destinations are rejected. A failed write can leave a `.store-building-*` directory for inspection. The format is experimental and may change; rebuild from the pinned RF2 archive when it does.

The reader loads numeric vectors into memory. It does not use memory mapping, precomputed transitive closure, Roaring bitmaps, Redis or compressed integer encodings yet. These measurements establish a baseline before those choices.

## UK snapshot measurements

The verified 26 August 2026 UK Monolith imported successfully. The [recorded results](compact-store-results.json) contain counts, checksums, settings and all five matched hierarchy digests.

| Measurement | Result |
|---|---:|
| Numeric core | 90,263,179 bytes, 86.1 MiB |
| Separate display file | 68,953,229 bytes, 65.8 MiB |
| Combined data files | 159,216,408 bytes, 151.8 MiB |
| Import elapsed time | 32.9 seconds |
| Import process peak RSS | 314.3 MiB |
| Numeric probe process peak RSS | 107.9 MiB |
| Numeric probe container charged peak | 114.7 MiB |

The core contains 1,151,519 concepts, 1,607,583 hierarchy edges, 2,960,422 ordinary attributes and 320,982 concrete attributes. There are 1,944 distinct concrete spellings. All concepts received a display under the recorded policy.

The import used two CPUs and a 2 GiB container limit. The numeric probe used one CPU and a 256 MiB limit with swap disabled. Container charged memory includes the wrapper and charged filesystem cache; it excludes the rest of the Docker VM and host. The initial import measurement did not capture a container peak.

Five complete hierarchy results matched version-pinned digests from OneLondon's Ontoserver. Four synthetic integration tests passed, including all eight hierarchy variants against a slow graph evaluator, group and decimal preservation, separate displays, invalid imports and corrupt stores. A real-release display lookup also succeeded within the 256 MiB container limit.

Two smoke runs took 2.6 and 3.5 seconds to load, checksum and validate the core through the Windows bind mount. Warm p95 timings for the five small queries ranged from 0.05 to 0.15 ms across those runs. These are preliminary observations, not controlled cold-start or throughput benchmarks.

The same-release Snowstorm Lite index occupies 506,801,604 bytes, but it implements capabilities and indexes missing from this prototype. The size difference does not yet establish an advantage for a complete ECL engine. Future comparisons must include all required semantic data and equivalent query work.

## Build and run

Use the pinned Rust 1.93.1 toolchain:

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
cargo build --locked --release --bins --examples
```

On Windows, native MSVC builds require the C++ build tools and Windows SDK. The measured run uses Linux in Docker instead. From PowerShell at the repository root:

```powershell
$rustImage = 'rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31'
$mount = 'type=bind,source=' + (Get-Location).Path + ',target=/work'
docker run --rm --mount $mount -e CARGO_TARGET_DIR=/work/target/linux -w /work $rustImage cargo build --locked --release --bins --examples
$release = Get-Content docs/release.json -Raw | ConvertFrom-Json
$edition = 'http://snomed.info/sct/83821000000107/version/20260826'
New-Item -ItemType Directory -Force data/compact-store | Out-Null
docker run --rm --cpus 2 --memory 2g --memory-swap 2g --mount $mount -w /work $rustImage python3 scripts/measure_process.py --report data/compact-store/import-resources.json --stdout data/compact-store/import.json target/linux/release/snomed-ecl-engine import ('data/rf2/' + $release.archiveFileName) data/compact-store/v1 $edition $release.sha256
```

Choose new store and report paths for subsequent imports. Run the five release-matched hierarchy probes in a fresh container:

```powershell
docker run --rm --cpus 1 --memory 256m --memory-swap 256m --mount $mount -w /work $rustImage python3 scripts/measure_process.py --report data/compact-store/probe-resources.json --stdout data/compact-store/probe.json target/linux/release/examples/storage_probe data/compact-store/v1 validation/ontoserver-baseline.json
docker run --rm --cpus 1 --memory 256m --memory-swap 256m --mount $mount -w /work $rustImage target/linux/release/snomed-ecl-engine hierarchy data/compact-store/v1 '<<' 195967001 --display
```

Omit `--display` for one numeric SCTID per line. The displayed form uses decimal strings for JSON codes. These operators are accepted: `<`, `<<`, `<!`, `<<!`, `>`, `>>`, `>!`, `>>!`.

The probe compares complete, numeric-sorted result digests with the saved baseline from OneLondon's Ontoserver. It reports load time separately from 100 warm iterations per query. The iterations include traversal, result allocation and sorting. These small asthma queries do not establish performance for broad expansions or refinements.
