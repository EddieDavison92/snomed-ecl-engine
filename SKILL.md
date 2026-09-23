---
name: snomed-ecl-engine
description: Build and use the SNOMED CT ECL engine. Use when importing a verified RF2 Snapshot, inspecting an index, or expanding ECL through the CLI or its persistent JSONL batch process.
---

# Use the SNOMED ECL engine

This is an embedded engine and CLI. No server, Elasticsearch or network access
is needed at query time. Run commands from the repository root.

## Get the executable

Install it with `npm install --global snomed-ecl-engine`, `brew install
eddiedavison92/tap/snomed-ecl-engine` or the [README's other
installers](README.md#install), or build it from the repository:

```sh
cargo build --locked --release --bin snomed-ecl-engine
```

The commands below write `snomed-ecl-engine`; from a build, use
`./target/release/snomed-ecl-engine` (`.exe` on Windows). Add
`--features unicode` for term matching in description filters; it needs ICU
([developer setup](docs/setup.md#build-with-term-matching)). A build without it
rejects term predicates explicitly. Run `--help` or `COMMAND --help` for
arguments.

## Get the RF2 release

The user must supply an RF2 Snapshot they are licensed to use, as one
self-contained ZIP. UK Monolith is the tested edition. Do not download a release
on the user's behalf without their credentials and consent, and never print or
save a TRUD API response: its download URLs contain the API key. Keep archives
and indexes under `data/`, which Git ignores.

```sh
snomed-ecl-engine inspect data/rf2/ARCHIVE.zip
```

`inspect` prints the archive's SHA-256, release date and edition URI. Check that
SHA-256 against the value the distributor published: a checksum of the
downloaded file alone does not show where it came from.

## Build the index

```sh
snomed-ecl-engine add data/rf2/ARCHIVE.zip --sha256 DISTRIBUTOR_SHA256 --json
snomed-ecl-engine list --json
```

`add` imports the archive, packs it into one file in the library folder, names
it by edition and release date, such as `uk-20260826`, and selects it. Always
pass `--sha256`: without a terminal, `add` refuses rather than asking. It
refuses to overwrite an index of the same name; `remove NAME --yes` deletes one.
Check the `edition` in its output before relying on the index.

## Expand ECL

Name the index explicitly: a library name such as `uk-20260826`, a release such
as `uk@2026-08`, or a path. `use` records a selection for people at a terminal,
and `SNOMED_ECL_STORE` overrides it.

```sh
snomed-ecl-engine expand uk-20260826 '<< 404684003' --count --json
snomed-ecl-engine expand uk-20260826 '404684003' --display --json
snomed-ecl-engine expand uk-20260826 '< 404684003 : 363698007 = << 39057004' --json
```

Quote the whole expression. `--count --json` returns `{"total":...}`; `--json`
emits one `{"code":"..."}` per line; `--display` adds `display`, which may be
null. Codes are decimal strings: keep them as strings. An empty expansion is an
empty stream, so use `--count` to tell it from a failure when that is all you
need. Never invent a label for a missing display.

`diff OLD NEW ECL --json` evaluates one expression against two indexes and
returns `added`, `removed` and `unchanged`.

## Answer many queries from one process

```sh
snomed-ecl-engine batch uk-20260826
```

Write one JSON object and a newline per request, flush, and read one response
line. Drain stdout while sending, or send and read one at a time, to avoid a pipe
deadlock. Close stdin and wait for exit when finished.

```jsonl
{"ecl":"<< 404684003","count_only":true}
{"ecl":"404684003","display":true}
{"search":"chronic kidney","limit":5}
{"concept":"709044004"}
```

The [CLI guide](docs/cli.md#answer-requests-in-batch) documents every request
and response field.

**Treat any response with `error` as a failure, never an empty result.** Parse
errors carry a kind, a byte offset and a message:

- `Syntax`: the text is not ECL.
- `Semantic`: the grammar admits it but ECL 2.3 gives it no meaning, such as the
  three open forms in [ECL support](docs/ecl-support.md#open-questions).
- `Unsupported`: this build or index cannot evaluate it, such as a term filter
  without `--features unicode`.
- `Limit`: the expression exceeds a size or work limit.

A bad request returns an error line and the next request still runs. Enforce a
timeout on the caller's side and handle early EOF. Never report an interrupted
or limited expansion as complete.

## Know the semantics

- Ordinary ECL includes inactive concepts and uses active inferred relationships
  and active reference set rows. A literal or membership query can return an
  inactive concept; an unknown literal returns an empty set.
- History supplements add a result's inactive **predecessors**, not successors.
  To find what replaced an inactive concept, project the association:
  `^ [targetComponentId] 900000000000527005 {{ M referencedComponentId = X }}`.
- Member projections can return typed values or rows rather than concepts:
  batch responses then carry `result_type` with `values` or `rows`. Do not treat
  map targets or numbers as SNOMED CT identifiers.
- Compare results with another engine only after pinning the same edition and
  supplements, and compare whole code sets, not counts.

[ECL support](docs/ecl-support.md) is the reference for what evaluates.
[Indexes](docs/indexes.md) covers supplements such as UK PCD
(`add-refsets`), display selection and alias configuration.

## Use the library

```rust
let store = NumericStore::open(Path::new("data/uk.ecl"))?;
let ordinals = eval::evaluate(&store, &ecl::parse("<< 64572001")?)?;
```

Keep the store open for repeated queries and resolve ordinals through
`store.ids`. The [README](README.md#embed-it) shows the rest.
