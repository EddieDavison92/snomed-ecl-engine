# Indexes

An index is built once from one verified RF2 Snapshot and never changes. Queries
read it; nothing writes to it. Sizes and timings are in [benchmarks](benchmarks.md).

## Build one

```sh
snomed-ecl-engine inspect ARCHIVE.zip          # checksum, release, edition URI
snomed-ecl-engine import ARCHIVE.zip INDEX_DIR EDITION_URI SHA256
```

The importer takes **one self-contained Snapshot ZIP** containing concepts,
inferred relationships, concrete relationships, descriptions, language refsets
and module dependencies. UK Monolith is the validated input. Full, Delta, split
extensions, merged packages and incremental updates are not supported, and are
rejected rather than partially read.

Import verifies the archive checksum before reading content, then validates
unique concepts, active relationship IDs, concept references, dates, modifiers,
graph endpoints and acyclicity. It requires the package date to match the edition
URI and the edition's own composition dependency to be present.

The importer writes to a sibling directory and publishes it by rename once
checksums are written, so a partial index is never queried. An existing
destination is rejected: choose a new path rather than deleting one in place. A
failed write can leave a `.store-building-*` directory for inspection; do not
query it.

The format is versioned but still experimental. When it changes, rebuild from the
pinned archive rather than migrating.

## What is stored

Every concept has a dense `u32` ordinal, including inactive concepts: the ECL
default covers all concepts, while hierarchy traversal uses active inferred
edges. The core holds the sorted SCTID dictionary, module ordinals, effective
dates and active/definition flags.

Relationships reference four-byte ordinals rather than repeating 18-digit codes.
Attribute rows are ordered by source, group, type and value, and group numbers
survive import, including group zero and groups shared between ordinary and
concrete relationships.

Concrete values keep their original decimal spelling in a separate dictionary, so
import loses no precision and the evaluator compares decimals exactly. Nothing is
converted to binary floating point.

Descriptions, typed member tables, identifiers and displays are separate sections
that load on first use and stay cached for the store's lifetime. A numeric query
never loads description data.

## Displays

Display labels are a separate lookup, resolved after a result set is known. The
default policy takes the first match from:

1. NHS clinical realm, `999001261000000100`
2. NHS pharmacy realm, `999000691000001104`
3. GB English, `900000000000508004`

It falls back to an active English fully specified name, then an active English
synonym, breaking ties on the smallest description ID. Missing text returns null
and is never replaced with an invented label. Supply a different ordered refset
list as the final `import` argument; the manifest records what was used.

This selects one label per concept. It cannot answer queries about synonyms,
dialect or description metadata; those read the description index through
[description filters](ecl-support.md).

## Supplementary refsets

```sh
snomed-ecl-engine add-refsets BASE_STORE ARCHIVE DESTINATION RELEASE_DATE SHA256
```

Loads simple concept refsets from a verified Snapshot ZIP, such as the UK PCD
package, including any new defining concepts and inferred is-a rows it supplies.
`RELEASE_DATE` is `YYYYMMDD` and the destination must be new.

Existing definitions and populated refsets cannot be replaced; for an updated
supplement, start again from the original base. Descriptions, identifiers and
typed members are preserved when the base has them, and the base display index is
required. Arbitrary maps are not imported. The manifest records each supplement's
archive checksum, release date and base core checksum.

## One file

```sh
snomed-ecl-engine pack INDEX_DIR uk.ecl
snomed-ecl-engine verify uk.ecl
snomed-ecl-engine expand uk.ecl '<< 64572001' --count
```

`pack` converts an existing index without reimporting RF2, preserving every
component byte, manifest value and checksum. Both layouts work everywhere a store
is accepted. A query-only build can pack and verify without the ZIP importer.

The default is zstd level 3 over independent 64 KiB blocks, with no dictionary.
Labels (`display.bin`) stay raw: they are read a few bytes at a time, and a
compressed label costs decoding its whole block, so a search reading 400 labels
took 21 ms against 0.5 ms raw, for 55 MB more on disk in the UK edition.
`--block-kib` accepts powers of two from 4 to 1,024; `--uncompressed` isolates the
container layout from compression. The destination must be new; packing spools
beside it and publishes with a hard link, so the filesystem must support hard
links and have room for roughly twice the resulting container.

Compression reduces stored bytes. It does not reduce the resident working set:
the evaluator still loads complete numeric columns.

### Container format

Container version 2 is independent of component format 1. All integers are
little-endian. The 56-byte header is magic `SNECL002` (8), whole-file length (8),
JSON table length (8) and the table's SHA-256 (32).

The table embeds the manifest and ordered entries of name, offset, encoded length
and codec. Sections start on 4 KiB boundaries and cover the core, membership,
typed member tables in refset order, identifiers, descriptions and displays where
declared. Codec 0 is raw bytes; codec 1 is `SNZST001` followed by a decoded block
size, block count, a 36-byte entry per block holding compressed length and frame
SHA-256, then independent zstd frames.

Truncating the file is invalid, because there is no numeric-only prefix. Padding carries
no meaning and is not covered by component hashes, so pin a whole-file SHA-256
when distributing an artefact.

## Validation

Opening an index checks the header, table hash, section names, codecs, lengths
and offsets. Component readers keep their own checksum and structural validation,
and cold sections are checked when first loaded. `verify` checks every declared
section, including UTF-8 display offsets, without holding all typed member tables
at once.

Readers each hold their own file handle and position. Concurrent lazy loads,
damaged headers and frames, overlapping sections, truncation and failed
publication are covered by tests.

Published indexes are immutable. Do not edit or replace files while readers have
them open; build a new artefact and open it in a new store.

## Query configuration

`expand`, `query` and `batch` accept `--config FILE` to bind ECL alias names to
SCTIDs:

```json
{
  "identifier_schemes": {"example": "100001"},
  "member_language": "en",
  "dialects": {"en-gb": "900000000000508004", "local": "100002"}
}
```

Alias names are case-insensitive; identifier codes are case-sensitive. Supplying
`dialects` replaces the defaults, which otherwise follow
[Appendix C of the ECL specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/appendices/appendix-c-dialect-aliases).
`member_language` selects collation for member string predicates and defaults to
`en`; RF2 member rows carry no language, while description predicates use each
description's own language.

Batch responses include `query_config_sha256`, the SHA-256 of the configuration's
canonical JSON. Record it alongside the edition and supplement checksums when
comparing results across runs.

### Alternate identifiers

Import reads RF2 Identifier Snapshot files, including inactive associations. Only
active associations resolve concepts, and an association to a description or
relationship does not resolve its owning concept. An unknown code returns an
empty set; an unknown scheme alias is an error. An active identifier association
can identify an inactive concept.

Both `"example#code with spaces"` from the normative grammar and
`example#"code with spaces"` from the official examples are accepted. Escaped
characters in codes are not.

Supplements preserve base identifiers and may add distinct `(scheme, code)` keys.
Replacing a key requires a fresh import, as does adding identifier support to an
index built without it.
