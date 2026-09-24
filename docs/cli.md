# Use the CLI

Install an executable from a release or build one; see the
[README](../README.md#install). Run `snomed-ecl-engine --help`,
`snomed-ecl-engine COMMAND --help` or `--version`. With no command, it lists the
commands, names the selected index and suggests the next step.

```sh
snomed-ecl-engine download
snomed-ecl-engine list
snomed-ecl-engine expand '404684003' --display
snomed-ecl-engine expand '<< 404684003' --count
```

## Manage indexes

`download` fetches the newest UK Monolith Snapshot from NHS England's
[TRUD](https://isd.digital.nhs.uk/trud/), checks it against the SHA-256 TRUD
publishes, and adds it as below. Set `TRUD_API_KEY` to the API key on your TRUD
account page first; the account must be subscribed to the item. `--list` shows
the releases TRUD holds and `--release ID` fetches an older one. The archive is
deleted once the index is built, unless you give `--keep-archive`. The key is
never printed, and is removed from any error.

`add ARCHIVE` builds an index from an RF2 Snapshot ZIP, packs it into one file
in the library folder and selects it. It checks the archive against the
checksum your distributor published: give it with `--sha256`, or confirm the one
it shows when asked. The index is named by edition and release date, such as
`uk-20260826`; `--name` chooses another, and `--edition` names the edition when
the archive does not.

`list` shows the library's indexes by name and release, then any other index
directly inside the working directory or `data/`, or inside the folders given.
`use` selects one, recorded outside the repository, and `use --clear` forgets
it. `remove NAME` deletes one from the library after asking; `--yes` skips the
question in scripts.

Anywhere an index is expected, give a path, a library name such as
`uk-20260826`, a release such as `uk@2026-08` or `uk@2026-08-26`, or an edition
alone, such as `uk`, for its latest release. A partial date picks the latest
release that matches it.

`stats`, `verify`, `expand`, `query`, `batch` and `hierarchy` take no index when
one is selected. Each resolves the index from its own argument first, then
`SNOMED_ECL_STORE`, then the selection, and says which it used. Name the index
explicitly in scripts and CI, where a developer's selection should not apply.

The library folder is `SNOMED_ECL_HOME` if set, else `snomed-ecl-engine/indexes`
in the platform's data folder: `%LOCALAPPDATA%` on Windows, `~/Library/Application
Support` on macOS and `$XDG_DATA_HOME` or `~/.local/share` on Linux.

## Query interactively

`query` opens one index, then evaluates expressions until `:quit`:

```text
ecl> << 195967001 |Asthma|
  129 concepts in 0.935 ms
```

`:display` toggles terms, `:count` toggles totals only and `:stats` prints the
manifest. `:search TEXT` and `:lookup CODE` browse without leaving the session.
Errors print and return to the prompt. Results are listed in pages of 40 beside
the full total; `expand --json`, or `expand` redirected to a file, returns every
code.

## Expand an expression

`expand ECL` evaluates one expression. In a terminal it lists the first 200
concepts with their terms and the total; `--codes` drops the terms. Redirected
output returns every code, one per line. `--count` gives the total, `--display`
adds terms to redirected output, and `--csv` writes a `code,display` table of
every concept, for a spreadsheet:

```sh
snomed-ecl-engine expand uk '<< 73211009 |Diabetes mellitus|' --csv > diabetes.csv
```

## Find and describe concepts

`search TEXT` finds concepts whose terms contain every word, best match first.
The last word may be the start of one, so `search chronic kid` finds chronic
kidney disease as you type.
`--within ECL` searches only an expression's concepts, `--limit N` shows more
than 50, and `--inactive` includes retired concepts.

`lookup SCTID` describes one concept: its terms, status, parents, children,
attribute groups and reference set membership. `history SCTID` shows what
replaced a concept and what it replaced. Redirected, all three answer in the
same JSON as the [batch requests](#answer-requests-in-batch).

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
snomed-ecl-engine diff uk@2026-08 uk@2026-11 '< 900000000000455006' --display
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

`add` runs `inspect`, `import` and `pack` in one step. Run them yourself to
keep the unpacked directory, choose display refsets or build somewhere other
than the library. [Indexes](indexes.md) covers what `import` accepts,
`add-refsets` for supplements such as UK PCD, and `pack` and `verify`.

## Output

A terminal gets readable summaries and tables with terms, with parse,
evaluation and index-open timings on stderr. Redirected output, or `--plain`,
gives plain lines for scripts. `--json` gives JSON:

| Command | Redirected or `--plain` | `--json` |
|---|---|---|
| `list` | One index object per line | Same |
| `use` | Index and edition JSON | Same |
| `add` · `download` | Name, path, edition and size JSON | Same |
| `download --list` | One release object per line | Same |
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
| `expand --csv` | `code,display` rows with a header | Not combined |
| `search` · `lookup` · `history` | One JSON answer, as `batch` gives | Same |
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
