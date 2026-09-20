# Use the CLI

Build with `cargo build --locked --release --bin snomed-ecl-engine`, or install from the checkout with `cargo install --locked --path .`. Native Windows builds need the MSVC C++ tools and Windows SDK. [Build and run](compact-store.md#build-and-run) includes the Linux Docker alternative.

Add `--features unicode` to build or install term matching. Its ICU4C prerequisites and package cost are described in the [Unicode build guide](descriptions.md#build-with-unicode-term-matching).

The executable is `target/release/snomed-ecl-engine` (`.exe` on Windows). Run `--help`, `COMMAND --help` or `--version`. Agents can follow the repository [SKILL.md](../SKILL.md) for the complete import and query workflow.

```sh
snomed-ecl-engine stats data/compact-store/v1
snomed-ecl-engine expand data/compact-store/v1 '404684003' --display
snomed-ecl-engine expand data/compact-store/v1 '<< 404684003' --count
```

Terminal output has an index summary, a code/display table with `--display`, and separate parse, evaluation and index-open timings on stderr. Every result is returned. Query timing excludes display lookup and output. Import reports nine stage starts with elapsed time on stderr. Stages have different costs; the stage number is not a completion percentage.

## Select output for scripts

| Command | Redirected default or `--plain` | Explicit `--json` |
|---|---|---|
| `stats` | Manifest JSON | Manifest JSON |
| `import` | Manifest and elapsed time JSON | Same |
| `add-refsets` | Combined manifest JSON | Same |
| `expand` | One code per line | One code object per line |
| `expand --display` | Code/display JSONL | Same |
| `expand --count` | Integer | Object with `total` |
| `batch` | One JSON response per request | Same |
| `hierarchy` | One code per line | One code object per line |
| `hierarchy --display` | Code/display JSONL | Same |

`--plain` forces the original script format in a terminal. `--json` and `--plain` are mutually exclusive. `NO_COLOR` disables colour; redirection and `TERM=dumb` also suppress colour. Diagnostics remain on stderr. CLI errors exit non-zero. Closing a results pipe early is treated as a normal exit.

`batch STORE` loads the index once and reads newline-delimited JSON from stdin. It always emits JSONL, including in a terminal. Individual query errors return an error object and do not stop the batch. An empty successful expansion has total zero. See [the agent batch workflow](../SKILL.md#reuse-the-index-for-many-queries) for schemas and process handling.

[Member projections](member-filters.md) can return distinct typed values or rows. `expand` emits these as JSONL, and `--count` counts values or rows. `--display` requires concept results. Batch responses use `result_type: "values"` with `values`, or `result_type: "rows"` with `rows`, instead of `codes`; `count_only` omits the array. Concept result formats are unchanged.

The presentation code uses Rust's standard library and belongs only to the CLI binary. Library users get no terminal output or UI dependencies. A full-screen workbench is deferred. Full ECL implementation remains required; [conformance](conformance.md) tracks the outstanding work.

Use `add-refsets BASE_STORE ARCHIVE DESTINATION RELEASE_DATE SHA256` for supplementary simple refsets, including PCD. Read [refset loading](refsets.md) for supported definitions, collision handling and release provenance.
