---
name: snomed-ecl-engine
description: Build and use the local Rust SNOMED ECL engine. Use when cloning this repository, importing a verified RF2 Snapshot, inspecting an index, or expanding ECL through the CLI or persistent JSONL batch process.
---

# Use the SNOMED ECL engine

Run commands from this repository's root. This is an embedded engine and CLI; no MCP server, HTTP server, Elasticsearch or Redis is needed.

## Get the CLI

Clone the repository, using the caller's GitHub access if required:

```sh
git clone https://github.com/EddieDavison92/snomed-ecl-engine.git
cd snomed-ecl-engine
cargo build --locked --release --bin snomed-ecl-engine
```

Use the toolchain pinned in `rust-toolchain.toml`. With rustup installed, Cargo selects it automatically. Native Windows builds need the MSVC C++ build tools and Windows SDK. For a Linux Docker build, read [Build and run](docs/setup.md#build-in-docker).

The commands below use the Linux/macOS executable path. On Windows, use `./target/release/snomed-ecl-engine.exe`. Alternatively, install from the checkout with `cargo install --locked --path .` and use `snomed-ecl-engine` on PATH. No crates.io package or prebuilt release is published yet.

Run `./target/release/snomed-ecl-engine --help` or `COMMAND --help` for arguments. For an existing index and a query-only executable, build with `--no-default-features`. That executable cannot import RF2.

For description term filters, build or install with `--features unicode`. This requires ICU4C static development libraries, `pkg-config` and a C compiler; the [Unicode build guide](docs/setup.md#build-with-term-matching) covers the tested Linux setup. Keep this feature when also using `--no-default-features`. A build without it rejects term predicates explicitly.

## Obtain the right RF2 package

Use an RF2 archive the caller is entitled to access. Data and indexes are not included in the clone. Keep them under ignored `data/` or another private local directory.

The current importer requires **one self-contained Snapshot ZIP**, including its dependencies, package metadata, concepts, inferred relationships, concrete relationships, descriptions, language refsets and module dependencies. UK Monolith is the validated input. General extension/base merging, Full, Delta and arbitrary extracted RF2 files are not supported. The separate `add-refsets` command supports simple refset supplements such as PCD. Do not concatenate them or imply that any RF2 archive will work.

Use a versioned edition URI and the SHA-256 supplied by the release distributor. A hash computed only from the downloaded file does not establish its provenance. Do not reuse the pinned release's URI or checksum for a newer release.

If this user's configured 1Password/TRUD setup is available, [local setup](docs/setup.md#get-an-rf2-release) explains `scripts/Get-Rf2Release.ps1`. It can retrieve the latest UK Monolith or a specified release. Its 1Password reference is machine-specific; other users can supply a verified archive directly. Never print credentials or raw TRUD responses, whose URLs can contain the key. A missing credential is not a reason to invent one or write it into the repository.

## Import and inspect

This reproducible example uses the release pinned in [docs/release.json](docs/release.json), not a moving "latest" release:

```sh
./target/release/snomed-ecl-engine import data/rf2/uk_sct2mo_42.5.0_20260826000001Z.zip data/compact-store/v1 http://snomed.info/sct/83821000000107/version/20260826 1330d2f2f48022d2594306f8dbbd891f1709e639e91cb97b281a9e796cfedd2b --json
./target/release/snomed-ecl-engine stats data/compact-store/v1 --json
```

The destination must not exist. Import verifies the archive checksum, validates the supported content and writes `manifest.json`, numeric indexes, displays, descriptions and typed files under `members/`. It reports stage starts to stderr and returns the completed manifest on stdout. Existing destinations are rejected, so reuse a matching index or choose a new directory rather than deleting it automatically. Failed writes can leave a sibling `.store-building-*` directory; do not query that partial directory.

Check the manifest's `edition`, `archive_sha256` and counts before reusing an index. `stats` reads metadata only. Opening an index for a query verifies the core and declared membership checksums and structure. The pinned UK release has 1,151,519 concepts, including 838,955 active concepts.

Default display selection prefers NHS clinical realm, NHS pharmacy realm, then GB English. An optional final positional argument supplies ordered, comma-separated refset IDs. See [display selection](docs/indexes.md#displays) when another display policy is needed.

## Expand ECL

Quote the complete ECL expression so the shell preserves operators and spaces:

```sh
./target/release/snomed-ecl-engine expand data/compact-store/v1 '<< 404684003' --count --json
./target/release/snomed-ecl-engine expand data/compact-store/v1 '404684003' --display --json
./target/release/snomed-ecl-engine expand data/compact-store/v1 '(< 404684003 : 363698007 = << 39057004)' --json
```

`use PATH` records one index so later commands need no path, and `stores` lists
the indexes it can find. Agents should keep passing the path explicitly: the
selection belongs to the calling user, and `SNOMED_ECL_STORE` overrides it.
People working at a terminal can use `query` for an interactive prompt over one
open index.

`diff OLD_STORE NEW_STORE ECL --json` evaluates one expression against two
indexes and returns `added`, `removed` and `unchanged`, for comparing releases
or refset supplements. It accepts concept results only.

`--count --json` returns `{"total":...}`. Ordinary `--json` expansion emits one `{"code":"..."}` object per line. `--display --json` adds `display`, which can be null. Codes are decimal strings; preserve them as strings in JavaScript and JSON consumers. All matches are returned, including an empty stream for an empty expansion. Use `--count` to distinguish an empty success when that is all the caller needs.

Numeric queries need only the manifest and core. Displays are resolved after evaluation and require `display.bin`. Missing displays must not be replaced with invented clinical labels. Without explicit output flags, a terminal shows readable summaries and display tables; redirected output preserves the original code-line or JSON formats. Prefer explicit `--json` for agent automation.

## Reuse the index for many queries

Launch one persistent child process:

```sh
./target/release/snomed-ecl-engine batch data/compact-store/v1
```

Write one UTF-8 JSON object and a newline to its stdin, then flush. Read one JSON response line per request. Keep stdout and stderr separate. Example requests:

```jsonl
{"ecl":"<< 404684003","count_only":true}
{"ecl":"404684003"}
{"ecl":"404684003 MINUS 404684003","count_only":true}
```

A success includes `edition`, `total`, `parse_ms` and `eval_ms`. Unless `count_only` is true, it also includes `codes`. The final example returns total zero. Batch does not return display labels. Close stdin and wait for the child to exit when finished. For thousands of expressions, drain stdout while sending requests or send/read one at a time to avoid pipe deadlock.

Treat any response with `error` as failure, not an empty expansion. Parse errors include a kind (`Syntax`, `Semantic`, `Unsupported` or `Limit`), byte offset and message. `Semantic` means the grammar admits the form but ECL 2.3 defines no result for it; `Unsupported` means the engine has not implemented a valid form yet. Evaluation errors include `Semantic(...)` rulings such as memberOf over a description-based reference set, unsupported operations, type mismatches, invalid ASTs, cancellation and exhausted work/memory limits. A bad query returns an error line and allows the next request to run. A startup or stream failure exits non-zero. Enforce a caller-side timeout and handle premature EOF. Never report an interrupted or limited expansion as complete.

Requests are limited to 512 KiB per line; ECL itself is limited to 65,536 bytes, depth 64 and 4,096 parser nodes. Count-only requests still evaluate the expression; they avoid serialising the code list. Warm `eval_ms` excludes index loading, parsing, output and process startup.

## Add supplementary refsets

Use a verified simple RF2 Snapshot supplement such as PCD:

```sh
./target/release/snomed-ecl-engine add-refsets BASE_STORE PCD_ZIP NEW_STORE YYYYMMDD TRUSTED_SHA256
./target/release/snomed-ecl-engine expand NEW_STORE '^REFSET_SCTID' --count
```

Substitute the real date, checksum and refset SCTID. The base needs its display and membership files. The loader adds new refset definitions and inferred is-a edges without rereading the base RF2. It rejects collisions, unknown concepts and existing definitions. For a newer supplement, start from the original base store and use a new destination.

Inspect `supplements` in the combined manifest. Batch responses include supplement archive checksums; the edition URI still identifies the base. Exact module versions are not yet fully resolved. Supplementary descriptions and language memberships are preserved when the base has a description index. Typed simple members and supported metadata are preserved when the base has member tables. Arbitrary supplementary maps remain outside this command's scope. Read [refsets](docs/indexes.md#supplementary-refsets) before importing a different extension format.

## Pack and verify an index

```sh
./target/release/snomed-ecl-engine pack INDEX_DIRECTORY uk.ecl
./target/release/snomed-ecl-engine verify uk.ecl
./target/release/snomed-ecl-engine expand uk.ecl '<< 64572001' --count
```

All store arguments accept either a directory or a packed file. Packing retains
every semantic component and uses independent zstd blocks. Choose a new output
file and allow temporary disk space for about twice its size. The output
filesystem must support hard links. `add-refsets` accepts a packed base and
writes a new directory. Pack that directory afterwards. Query-only builds can
pack and verify too. See [container format and validation](docs/indexes.md#container-format).

Compression reduces stored bytes; the current evaluator still loads complete
description columns. Do not infer a 256 MiB full-language memory guarantee from
the compressed file size. Keep RF2 and packed indexes outside Git.

## Recognise current limits

Every ECL 2.3 feature area is implemented: hierarchy and Boolean sets,
refinements, groups and cardinalities, reverse and dotted attributes, exact
concrete comparisons, top/bottom, membership (`^`, `^R`), concept filters,
description filters, member filters and projections, history supplements and
alternate identifiers. Term matching inside description filters needs
`--features unicode`.

Three grammar-valid forms have no settled meaning in the specification and
return a `Semantic` error: a reverse flag inside an attribute group, a member
filter without a refset operator, and a reverse flag with a concrete value. Read
[ECL support](docs/ecl-support.md) before claiming a category is covered, and
never present a `Semantic` error as an empty result.

History supplements add the inactive **predecessors** of a result, not its
successors. To find what replaced an inactive concept, project the association:
`^ [targetComponentId] 900000000000527005 {{ M referencedComponentId = X }}`.

Use `--config FILE` for [identifier and dialect aliases](docs/indexes.md#query-configuration).

Do not simplify unsupported ECL silently. Ordinary ECL includes active and inactive concepts, active inferred relationships and active refset member rows. An inactive concept can be returned by a literal or membership query. An unknown literal returns an empty set. For external correctness checks, pin the same edition and supplement checksums and compare complete code sets. The optional helper for OneLondon's Ontoserver in [local setup](docs/setup.md#refresh-the-ontoserver-probes) needs a separately configured credential helper; it is not required to use this engine.

For direct Rust integration, use `NumericStore::open`, `ecl::parse` and `eval::evaluate_with_limits`; resolve returned ordinals through `store.ids`. Use `eval::evaluate_result_with_limits` to accept typed member projections as well as concept sets. Keep the opened store for repeated queries. `DisplayStore` is a separate optional lookup. The library exposes `add_refsets_snapshot` for supplementary simple refsets. The default importer API is silent; `import_snapshot_with_progress` accepts a stage callback. Hosting and deployment belong to a separate application repository.

Description queries load `descriptions.bin` on demand. Reimport older stores to build it. The first such query includes file validation and loading; `store.descriptions.get()` can preload it. Term matching needs `--features unicode`. See [description filters](docs/ecl-support.md) for supported predicates and memory measurements.

Member filters and projections load the needed `members/<refset>.bin` files. Reimport older indexes to build them. Typed projections can return distinct scalar values or rows instead of concept codes: batch responses then have `result_type: "values"` with `values`, or `result_type: "rows"` with `rows`. Preserve these types; do not interpret map targets or numbers as SNOMED IDs. `--count` counts distinct values or rows, and `--display` requires concept results. Read [member filters](docs/ecl-support.md) for examples, limits and remaining type gaps.
