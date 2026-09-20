# Single-file index

`pack` converts an existing index into one file without importing RF2 again. It
preserves every component byte, manifest value and checksum, including typed
member tables, descriptions, identifiers, history data and displays. Existing
directory indexes remain readable.

```sh
snomed-ecl-engine pack INDEX_DIRECTORY uk.ecl
snomed-ecl-engine verify uk.ecl
snomed-ecl-engine expand uk.ecl '<< 64572001' --count
snomed-ecl-engine batch uk.ecl
```

Both the library and CLI accept either layout. `add-refsets` can read a packed
base and still writes a new directory; pack that directory afterwards. A
query-only build can pack and verify indexes without the ZIP importer.

Packing uses zstd level 3 with independent 64 KiB blocks and no dictionary.
Use `--block-kib 16` to compare smaller blocks or `--uncompressed` to isolate the
container layout. Accepted block sizes are powers of two from 4 to 1,024 KiB.
The zstd dependency needs a C compiler at build time and is linked into the
executable. It requires no external compression process at runtime.

The destination must be new. Packing creates temporary spool and output files
beside it, checks all decoded section hashes, then publishes with a hard link.
The filesystem must support hard links. Allow temporary space for roughly twice
the resulting container, in addition to the source index. Failed packing does
not replace an existing destination.

## Format

Container version 2 is independent of component format 1. All integers are
little-endian. The 56-byte header contains:

| Field | Bytes |
|---|---:|
| Magic `SNECL002` | 8 |
| Whole file length | 8 |
| JSON section-table length | 8 |
| SHA-256 of the JSON table | 32 |

The table embeds the original manifest and ordered entries containing a name,
file offset, encoded length and codec. Each section begins on a 4 KiB boundary.
Sections contain the core, membership, typed member tables in refset order,
identifiers, descriptions and displays, where declared. The file ends at the
next 4 KiB boundary. Arbitrary truncation is invalid; numeric-only prefix files
are not implemented.

Codec 0 stores original bytes. Codec 1 starts with `SNZST001`, a four-byte
decoded block size and four-byte block count. Each 36-byte block-table entry has
a four-byte compressed length and a SHA-256 of the compressed frame. Independent
zstd frames follow. The manifest retains the decoded component lengths and
hashes. Readers check compressed hashes before decoding into a bounded buffer.

## Validation and memory

Opening checks the header, table hash, section names, codecs, lengths and offsets.
Component readers retain their checksum and structural validation. Cold sections
are checked when loaded. `verify` checks every declared section, including UTF-8
display offsets, without retaining all typed member tables at once. Padding has
no semantic meaning and is not covered by component hashes. Pin a whole-file
SHA-256 when distributing an artefact.

Published indexes are immutable. Do not edit or replace files while readers use
them. Build and verify a new artefact, then open it in a new store instance.

Each reader has its own file handle and seek position. Tests cover concurrent
lazy loads and display reads, damaged headers and frames, overlapping sections,
truncation, block boundaries and failed publication.

This implementation uses safe buffered reads, not memory mapping. A reader
decodes one block at a time, but the existing evaluator still loads complete
numeric and description columns. Smaller files do not establish a smaller
resident working set. Narrower columns, bounded description access, term
postings and mmap remain separate work in the [performance plan](performance-plan.md).

Use `scripts/benchmark_pack.py` to record packing time, binary and container
hashes, block size and container-charged memory. Packing time includes checksum
passes, compression, temporary writes and verification; it is not RF2 import
time. `scripts/benchmark_corpus.py --store-directory FILE` accepts a container
and records complete result digests as well as latency and charged memory.

## UK release measurements

The [recorded evaluation round](../validation/packed-results.json) pins the
26 August 2026 UK Monolith and executable. All 587 sections passed exhaustive
verification: 1,151,519 concepts, 3,562,700 descriptions and 7,177,107 typed
member rows. No component was removed.

| Layout | File bytes | Packing seconds |
|---|---:|---:|
| Original directory, including manifest | 1,164,730,660 | Not applicable |
| Raw container | 1,166,135,296 | 80.42 |
| zstd, 16 KiB blocks | 308,817,920 | 50.22 |
| zstd, 64 KiB blocks | 303,566,848 | 33.60 |

The default saves 73.94% of stored bytes. Its core occupies 21,881,133 bytes,
membership 12,343,029, descriptions 69,333,435, displays 14,166,942 and typed
members 184,115,145. The remaining bytes cover identifiers, the table and
alignment. These are encoded section sizes, including block metadata.

Packing ran serially on one CPU with a 1 GiB limit. Charged peaks were 6.7 MiB
for raw packing, 8.9 MiB for 16 KiB blocks and 8.3 MiB for 64 KiB blocks. The
Windows bind mount makes small writes expensive; these elapsed times are not a
general compression-speed comparison. Source files remain present after packing.

The same executable ran five shuffled batches and complete enumeration for
each layout, on one CPU with a 1 GiB limit:

| Layout | Median request | Request p95 | Median 1,000-query batch | Charged peak |
|---|---:|---:|---:|---:|
| Directory | 1.81 ms | 12.49 ms | 2.97 s | 539.7 MiB |
| Raw container | 1.95 ms | 12.94 ms | 3.09 s | 530.9 MiB |
| 64 KiB compressed container | 1.87 ms | 12.75 ms | 3.05 s | 542.6 MiB |

All 1,000 result digests match in every layout. This establishes preservation
and a storage saving, not a warm-query speedup. Caches were not dropped; container
hashes were read before query timing. Charged memory covers the container,
including cache charged to it, but excludes the host and the rest of the Docker
VM. The 64 KiB store opened in 1.13 seconds; container start through first
response took 1.72 seconds. Neither measures a provider cold start or download.

The Unicode/import executable is 35,532,936 bytes, or 14,000,863 gzipped. Compared
with the preceding build, block compression adds 872,536 executable bytes.

Independent scans matched 17 scalar cases, nine member queries and a tuple,
52 history cases, ten term cases, 18 description metadata cases and all 1,039
PCD refsets for both active and inactive members. The term comparisons retain
the known differences with OneLondon's Ontoserver. Full ECL compatibility still
requires the open items in [conformance](conformance.md).
