# Use the CLI

Install an executable from a release or build one; see the
[README](../README.md#install). Run `snomed-ecl-engine --help`,
`snomed-ecl-engine COMMAND --help` or `--version`. With no command, it lists the
commands, names the selected index and suggests the next step.

```sh
snomed-ecl-engine stores
snomed-ecl-engine use data/uk.ecl
snomed-ecl-engine stats
snomed-ecl-engine expand '404684003' --display
snomed-ecl-engine expand '<< 404684003' --count
```

## Choose an index once

`stores` lists the indexes directly inside the working directory and `data/`,
or inside the paths given to it. Both index directories and packed
files are listed; anything else is skipped. `use PATH` checks a path and records
it in `state.json` under the platform's config directory, outside the
repository. `use --clear` forgets it.

`stats`, `verify`, `expand`, `query`, `batch` and `hierarchy` then take no path.
Each resolves the index from its own argument first, then `SNOMED_ECL_STORE`,
then the recorded selection, and says which of the three it used. Give the path
explicitly in scripts and CI, where a developer's selection should not apply.

## Query interactively

`query` opens one index, then evaluates expressions until `:quit`:

```text
ecl> << 195967001 |Asthma|
  129 concepts in 0.935 ms
```

`:display` toggles terms, `:count` toggles totals only and `:stats` prints the
manifest. Errors print and return to the prompt. Results are listed in pages of
40 beside the full total; `expand --json`, or `expand` redirected to a file,
returns every code.

## Walk the hierarchy

`hierarchy OPERATOR SCTID` lists one concept's relatives without writing ECL.
The operator is one of `<` (descendants), `<<` (descendants and self), `>`
(ancestors) and `>>` (ancestors and self); add `!` for direct children or
parents only, as in `<!` or `>>!`. `--display` adds terms.

## Compare two indexes

`diff OLD_STORE NEW_STORE ECL` evaluates one expression against both and reports
what it gained and lost. Use it to see what a new release or a refset supplement
changed:

```sh
snomed-ecl-engine diff data/uk-2026-08.ecl data/uk-2026-11.ecl '< 900000000000455006' --display
```

Each index is opened, evaluated and closed in turn, so only one is in memory at
a time. Terms come from the index each code belongs to, so a concept the newer
release dropped still shows its old term. `--json`, or redirected output, gives
the whole added and removed sets; a terminal lists the first 40 of each with the
totals. Member projections that return values or rows are refused rather than
compared as concepts.

## Build an index

`inspect ARCHIVE` reads an archive's release metadata without importing it: the
SHA-256, the release date, which required Snapshot files are present, and the
edition URIs its module dependencies declare. It ends with the `import` command
for that archive.

The edition module is the root of the package's dependency graph. A well-formed
package has exactly one root; `inspect` says so when it does not, instead of
guessing.

The checksum shows a download is intact, not where it came from, so compare it
with the value the distributor published. `import` verifies it before reading
any content, and reports each of its eleven stages on stderr with the elapsed
time. Stages take different times, so the stage number is not a percentage.

[Indexes](indexes.md) covers what `import` accepts, `add-refsets` for
supplements such as UK PCD, and `pack` and `verify` for single-file indexes.

## Output

A terminal gets readable summaries, and a code and display table with
`--display`, with parse, evaluation and index-open timings on stderr. Redirected
output, or `--plain`, gives plain lines for scripts. `--json` gives JSON:

| Command | Redirected or `--plain` | `--json` |
|---|---|---|
| `stores` | One index object per line | Same |
| `use` | Index and edition JSON | Same |
| `inspect` | Archive summary JSON | Same |
| `diff` | Comparison JSON | Same |
| `stats` | Manifest JSON | Same |
| `import` | Manifest and elapsed time JSON | Same |
| `add-refsets` | Combined manifest JSON | Same |
| `pack` | File size and elapsed time JSON | Same |
| `verify` | Verified section and component counts JSON | Same |
| `expand` | One code per line | One code object per line |
| `expand --display` | Code and display JSONL | Same |
| `expand --count` | Integer | Object with `total` |
| `hierarchy` | One code per line | One code object per line |
| `hierarchy --display` | Code and display JSONL | Same |
| `batch` | One JSON response per request | Same |

`--json` and `--plain` are mutually exclusive. `NO_COLOR`, redirection and
`TERM=dumb` suppress colour. Diagnostics go to stderr, and errors exit non-zero.
Closing a results pipe early is a normal exit.

Every result is returned in full. Codes are decimal strings; keep them as
strings in JavaScript and other JSON consumers. [Member
projections](ecl-support.md) can return typed values or rows instead of
concepts: `expand` emits them as JSONL, `--count` counts them, and `--display`
refuses them.

## Answer requests in batch

`batch STORE` opens the index once and reads one JSON request per line from
stdin, writing one JSON response per line to stdout, in a terminal too. A bad
request returns an error object and the batch continues. Requests are limited
to 512 KiB a line; an expression to 65,536 bytes, depth 64 and 4,096 parser
nodes.

Each request names one of four operations.

**`ecl`** evaluates an expression:

| Field | Effect |
|---|---|
| `count_only` | Leave out the result array; `total` and the metadata remain |
| `display` | Return `concepts`, objects with `code`, `display` and `active`, in place of `codes` |
| `offset`, `limit` | Return a window of the result; `total` still counts all of it |

A success carries `edition`, `supplements` (their archive checksums),
`query_config_sha256`, `total`, `parse_ms` and `eval_ms`, then `codes` or
`concepts`. A member projection returns `result_type` of `values` or `rows`
with a `values` or `rows` array instead. `eval_ms` excludes loading the index,
parsing, output and process start.

**`search`** looks concepts up by name through the word index. It returns ranked
`concepts` with codes, labels and active flags, `total` matches and `search_ms`.
`limit` defaults to 50. `include_inactive: true` keeps retired concepts, and
`within` takes an expression and keeps only matches in its answer:
`{"search":"left","within":"<< 404684003 : 363698007 = << 39057004"}` looks for
"left" among findings sited in the lung.

**`concept`** returns one concept's descriptions, parents, children,
relationship groups and reference set membership, with `lookup_ms`. Concrete
values keep their published type. An unknown SCTID returns
`{"error":"NotFound"}`.

**`history`** returns a concept's historical associations in both directions:
`successors`, what replaced it, and `predecessors`, what it replaced, each naming
the association.

Any request may carry an `id`, any JSON value, which comes back on its response.
`batch STORE --workers N` answers with N threads sharing one index, writing each
response as it finishes, so responses arrive out of order; match them by `id`.
Without `--workers`, responses come in request order.

A failure carries `error`. A parse error adds a kind (`Syntax`, `Semantic` or
`Limit`), a byte `offset` and a `message`. An evaluation error names the cause,
such as `Unsupported` for a feature the build or index lacks, or a limit that
was reached. Treat any `error` as a failure, never as an empty result.
