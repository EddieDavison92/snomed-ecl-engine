# Use the CLI

Build with `cargo build --locked --release --bin snomed-ecl-engine`, or install from the checkout with `cargo install --locked --path .`. Native Windows builds need the MSVC C++ tools and Windows SDK. [Build and run](setup.md#build-in-docker) includes the Linux Docker alternative.

Add `--features unicode` to build or install term matching. Its ICU4C prerequisites and package cost are described in the [Unicode build guide](setup.md#build-with-term-matching).

The executable is `target/release/snomed-ecl-engine` (`.exe` on Windows). Run `--help`, `COMMAND --help` or `--version`. Agents can follow the repository [SKILL.md](../SKILL.md) for the complete import and query workflow.

```sh
snomed-ecl-engine stores
snomed-ecl-engine use data/compact-store/v1
snomed-ecl-engine stats
snomed-ecl-engine expand '404684003' --display
snomed-ecl-engine expand '<< 404684003' --count
```

## Choose an index once

`stores` lists the indexes it finds in the working directory and
`data/compact-store`, or in the paths given to it. Directories holding
`manifest.json` and packed index files are both listed; anything else is
skipped. `use PATH` checks a path and records it as an absolute path in
`state.json` under the platform's config directory, outside the repository.
`use --clear` forgets it.

`stats`, `verify`, `expand`, `query`, `batch` and `hierarchy` then take no path.
Each resolves the index from its own argument first, then `SNOMED_ECL_STORE`,
then the recorded selection, and says which of the three it used. Give the path
explicitly in scripts and CI, where a developer's selection should not apply.

`diff` always takes both paths, because comparing an index with itself has no
use.

## Query interactively

`query` opens and verifies one index, then evaluates expressions until `:quit`:

```text
ecl> << 195967001 |Asthma|
  129 concepts in 0.935 ms
```

`:display` toggles terms, `:count` toggles totals only, `:stats` prints the
manifest. Parse and evaluation errors print and return to the prompt rather than
ending the session. Results are listed in pages of 40 with the full total beside
them; `expand` redirected to a file, or `--json`, returns every code.

## Compare two indexes

`diff OLD_STORE NEW_STORE ECL` evaluates one expression against both and reports
what the definition gained and lost. Use it to see what a release or a refset
supplement changed:

```sh
snomed-ecl-engine diff data/compact-store/v1 data/compact-store/v1-pcd '< 900000000000455006' --display
```

Each index is opened, evaluated and closed in turn, so only one is resident at a
time. Terms come from the index each code belongs to, so a concept the newer
release dropped still shows the term the older index held. Redirected output and
`--json` give the complete added and removed sets; a terminal lists the first 40
of each with the totals. Member projections returning values or rows are refused
rather than compared as concepts.

## Prepare an archive for import

`inspect ARCHIVE` reads an archive's release metadata without importing it: the
SHA-256, the release date, which required Snapshot files are present, and the
edition URIs its own module dependencies declare. It ends with the `import`
command for that archive.

The edition module is the root of the package's dependency graph, so a module
another module depends on is a component of the edition rather than the edition
itself. A well-formed package has exactly one root; `inspect` says so when it
does not, instead of guessing.

The checksum confirms a download is intact. It does not establish where the file
came from, so compare it with the value the release distributor published before
importing. `import` verifies it before reading any content.

Running the executable with no command lists the commands, names the selected index and gives the next step. Terminal output has an index summary, a code/display table with `--display`, and separate parse, evaluation and index-open timings on stderr. Every result is returned. Query timing excludes display lookup and output. Import reports nine stage starts with elapsed time on stderr. Stages have different costs; the stage number is not a completion percentage.

## Select output for scripts

| Command | Redirected default or `--plain` | Explicit `--json` |
|---|---|---|
| `stores` | One index object per line | Same |
| `use` | Store and edition JSON | Same |
| `inspect` | Archive summary JSON | Same |
| `diff` | Comparison JSON | Same |
| `stats` | Manifest JSON | Manifest JSON |
| `import` | Manifest and elapsed time JSON | Same |
| `add-refsets` | Combined manifest JSON | Same |
| `pack` | File size and elapsed time JSON | Same |
| `verify` | Verified section and component counts JSON | Same |
| `expand` | One code per line | One code object per line |
| `expand --display` | Code/display JSONL | Same |
| `expand --count` | Integer | Object with `total` |
| `batch` | One JSON response per request | Same |
| `hierarchy` | One code per line | One code object per line |
| `hierarchy --display` | Code/display JSONL | Same |

`--plain` forces the original script format in a terminal. `--json` and `--plain` are mutually exclusive. `NO_COLOR` disables colour; redirection and `TERM=dumb` also suppress colour. Diagnostics remain on stderr. CLI errors exit non-zero. Closing a results pipe early is treated as a normal exit.

`batch STORE` loads the index once and reads newline-delimited JSON from stdin.
Set `"display": true` on a request to get `concepts`, an array of code and
label objects, in place of the bare `codes` array. Labels are resolved after
evaluation and the display index opens on first use, so a batch that never asks
never pays for it. `count_only` returns neither. It always emits JSONL, including in a terminal. Individual query errors return an error object and do not stop the batch. An empty successful expansion has total zero. See [the agent batch workflow](../SKILL.md#reuse-the-index-for-many-queries) for schemas and process handling.

`"offset"` and `"limit"` return a window of a result. `total` still counts the
whole answer, so a caller can show 200 of 838,955 without fetching the rest and
without mistaking the page for the set.

A request carrying `"search"` instead of `"ecl"` looks concepts up by name
through the word index rather than evaluating an expression. It returns ranked
`concepts` with their codes, labels and active flags, and `total` matches. It is
capped by `limit`, defaulting to 50, because it answers what a person meant
rather than producing a set. `"include_inactive": true` keeps retired concepts.

A request carrying `"concept"` returns that concept's descriptions, parents,
children, relationship groups and reference set membership. Concrete values keep
their published type. An unknown SCTID returns `{"error":"NotFound"}`.

[Member projections](ecl-support.md) can return distinct typed values or rows. `expand` emits these as JSONL, and `--count` counts values or rows. `--display` requires concept results. Batch responses use `result_type: "values"` with `values`, or `result_type: "rows"` with `rows`, instead of `codes`; `count_only` omits the array. Concept result formats are unchanged.

The presentation code uses Rust's standard library and belongs only to the CLI binary. Library users get no terminal output or UI dependencies. A full-screen workbench is deferred. Full ECL implementation remains required; [conformance](ecl-support.md) tracks the outstanding work.

`build-search STORE_DIRECTORY` adds the word index to an index that predates
it, then repack. Import builds it, so this is only for older stores.

Use `add-refsets BASE_STORE ARCHIVE DESTINATION RELEASE_DATE SHA256` for supplementary simple refsets, including PCD. Read [refset loading](indexes.md) for supported definitions, collision handling and release provenance.

Use `pack STORE NEW_FILE` to create a compressed single-file index, then
`verify NEW_FILE` to check every component. Every query command accepts that file
as its store argument. See [single-file indexes](indexes.md#one-file) for compression
options, temporary disk requirements and the current memory limits.
