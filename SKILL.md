---
name: snomed-ecl-engine
description: Build and use the SNOMED CT ECL engine. Use when downloading or importing an RF2 Snapshot, managing indexes, expanding ECL, finding or describing concepts, or answering queries through the CLI or its persistent JSONL batch process.
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

The user must supply an RF2 Snapshot they are licensed to use. UK Monolith is
the tested edition. Either the user sets `TRUD_API_KEY` and `download` fetches
it, or they give you one self-contained ZIP to `add`.

Download only with the user's own key and consent. Never print `TRUD_API_KEY`
or save a TRUD API response: its download URLs contain the key. `download`
redacts the key from its errors.

## Build the index

```sh
snomed-ecl-engine download --list
snomed-ecl-engine download --json
```

`download` fetches the newest UK Monolith Snapshot from NHS England's TRUD,
checks it against the SHA-256 TRUD publishes, then builds it as `add` does and
deletes the archive. `--release ID` takes another release from `--list`. The
user's TRUD account must be subscribed to the item. Allow a few minutes for the
download and about two for the build; progress goes to stderr.

For an archive the user gives you, check it first:

```sh
snomed-ecl-engine inspect data/rf2/ARCHIVE.zip
snomed-ecl-engine add data/rf2/ARCHIVE.zip --sha256 DISTRIBUTOR_SHA256 --json
```

`inspect` prints the archive's SHA-256, release date and edition URI. The
SHA-256 for `--sha256` must be the value the distributor published: a checksum
of the downloaded file alone does not show where it came from. Always pass it:
without a terminal, `add` refuses rather than asking. Keep archives under
`data/`, which Git ignores.

Both commands pack the index into one file in the library folder, name it by
edition and release date, such as `uk-20260826`, select it, and print its
`name`, `store` and `edition`. Check the `edition` before relying on the index.
Neither overwrites an index of the same name.

```sh
snomed-ecl-engine list --json
snomed-ecl-engine remove uk-20260826 --yes
```

`list` names the library's indexes and their releases. Remove one only when the
user asks.

## Expand ECL

Name the index explicitly: a library name such as `uk-20260826`, a release such
as `uk@2026-08`, or a path. `use` records a selection for people at a terminal,
and `SNOMED_ECL_STORE` overrides it.

```sh
snomed-ecl-engine expand uk-20260826 '<< 404684003' --count --json
snomed-ecl-engine expand uk-20260826 '404684003' --display --json
snomed-ecl-engine expand uk-20260826 '< 404684003 : 363698007 = << 39607008' --json
```

Quote the whole expression. Redirected output, as a script or agent sees it,
gives one code per line; `--json` emits one `{"code":"..."}` per line and
`--count --json` returns `{"total":...}`. `--display` adds `display`, which may
be null, and `--csv` writes a `code,display` table of every concept for the
user to open in a spreadsheet. Codes are decimal strings: keep them as strings.
An empty expansion is an empty stream, so use `--count` to tell it from a
failure when that is all you need. Never invent a label for a missing display.

`diff OLD NEW ECL --json` evaluates one expression against two indexes and
returns `added`, `removed` and `unchanged`.

## Find and describe concepts

```sh
snomed-ecl-engine search uk-20260826 chronic kidney disease --limit 10
snomed-ecl-engine search uk-20260826 left --within '<< 404684003 : 363698007 = << 39607008'
snomed-ecl-engine lookup uk-20260826 709044004
snomed-ecl-engine history uk-20260826 155574008
```

`search` finds concepts whose terms contain every word, the last as a prefix,
best match first. It leaves out inactive concepts unless given `--inactive`.
`lookup` returns one concept's terms, parents, children, attribute groups and
reference set membership. `history` returns what replaced a concept and what it replaced.
Redirected, each prints one JSON answer, the same as the matching batch request.
Use `search` to find a code rather than guessing one, then confirm it with
`lookup` before writing it into ECL.

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
