# How the index is built, compressed and read

The UK Monolith edition, 1.15 million concepts and 3.56 million descriptions,
packs into one 152 MiB file. This page covers how `import` builds it, how each
section is stored and compressed, and how queries read it. Commands and
validation rules are in [indexes](indexes.md); timings in
[benchmarks](benchmarks.md).

## Building

`import` reads one verified RF2 Snapshot ZIP and writes a directory of
sections, in eleven stages:

| Stage | Produces |
|---|---|
| Verify the archive checksum | Nothing is read before this passes |
| Read concepts and module dependencies | Sorted SCTIDs, one dense `u32` ordinal each |
| Read inferred relationships | Is-a graph both ways, grouped attribute rows |
| Read concrete values | Attribute rows pointing at exact decimal or string values |
| Index concept reference set membership | `membership.bin` |
| Select displays | One label per concept, from the configured language refsets |
| Index descriptions and language memberships | Description rows by concept |
| Write the store | `core.bin`, `display.bin`, `descriptions.bin` |
| Index description words | `search.bin` |
| Index typed reference set members | `members/<refset>.bin` |
| Index historical associations | `history.bin` |

From the second stage on, concepts are referred to by ordinal, so a relationship
row is three `u32` values rather than three 18-digit codes. The directory is
published by rename once every checksum is written. `pack` then turns it into a
single file.

## What each section holds

UK Monolith, 26 August 2026:

| Section | Holds | Directory | Packed |
|---|---|---:|---:|
| `descriptions.bin` | Description rows by concept, with terms and dialects | 376.0 MiB | 56.9 MiB |
| `display.bin` | One label per concept | 42.6 MiB | 42.6 MiB |
| `members/` | 582 typed reference set tables, 7.2 million rows | 370.4 MiB | 25.1 MiB |
| `core.bin` | SCTIDs, flags, dates, modules, hierarchy, attributes, concrete values | 86.1 MiB | 14.6 MiB |
| `search.bin` | Word index: each word's concept ordinals | 16.5 MiB | 7.6 MiB |
| `history.bin` | Historical associations keyed from both ends | 6.5 MiB | 2.4 MiB |
| `membership.bin` | Each concept reference set's member ordinals | 4.4 MiB | 0.8 MiB |
| Total | | 902.6 MiB | 152.0 MiB |

## Encodings

Each section is a sequence of length-prefixed arrays, little-endian. Most arrays
are fixed-width, so a reader can seek straight to one row. Where a section is
always read whole, the importer stores a smaller encoding and the reader
expands it on load.

**Sorted lists as varint gaps.** The word index and reference set membership are
sorted lists of ordinals. Each list is stored as its first value and then the
gaps between neighbours, seven bits a byte. Concepts sharing a word or a
reference set sit close together in SCTID order, so most gaps take one byte
rather than four. Postings fell from 47.7 MiB to 14.0 MiB before compression.
Both lists are decoded to `u32` arrays when the section loads, so queries read
plain arrays.

**Member rows sorted by referenced component.** Each table is written with its
rows sorted by `referencedComponentId`, and that column stored as varint gaps.
Row order carries no meaning in ECL. Sorted, neighbouring rows of a map usually
share their other fields too, so zstd finds long matches. Member tables packed
to 70.3 MiB in release order and 25.1 MiB sorted.

**Member UUID and refsetId are not stored.** ECL names reference set fields from
`referencedComponentId` on and gives the member UUID no meaning. `refsetId` is
the same on every row of a table. The UUID was 16 random bytes a row that no
compressor can shrink, a quarter of the packed index. See
[ECL support](ecl-support.md#by-feature).

**Labels as dictionary frames.** Each label is its own zstd frame, compressed at
level 12 against a 112 KiB dictionary trained on every nth label. A label is too
short to compress alone, and one frame per label keeps reads random-access. The
uncompressed lengths stay in memory, so search can rank candidates by label
length without reading them. Labels fell from 65.8 MiB to 42.6 MiB.

**Exact values stay text.** Concrete decimals keep their RF2 spelling in a value
dictionary, and relationship groups are stored as numbers on each attribute row.
Nothing is converted to binary floating point.

## Packing

`pack` writes one file: a 56-byte header, a JSON table of sections with the
manifest, then each section on a 4 KiB boundary. Section layout, codecs and
checksums are in [indexes](indexes.md#container-format).

Sections are split into independent blocks, each compressed with zstd at level
15 and hashed. Blocks are compressed in parallel and written in order, so the
file is identical whatever the thread count. Decoding costs the same at any
level. On the UK edition, level 15 packs to within 1% of level 19's size in about
a third of the time, while level 12 and below leave the compressed sections about
45% larger. Packing takes 98 s on two CPUs, and scales with cores.

Block size follows how a section is read:

| Section | Blocks | Why |
|---|---|---|
| Most sections | 64 KiB | Read whole on first use |
| `descriptions.bin` | 8 KiB | Describing one concept reads about a dozen small arrays, each costing a block decode |
| `display.bin` | Stored raw | Labels are already compressed; raw bytes allow positional reads without a lock |

## Reading

Opening a file reads the header and table, then decodes the core and
membership: 155 ms on one CPU. Every other section opens on first use and stays
cached for the store's lifetime:

| Read | How |
|---|---|
| A numeric query | Core only; never touches descriptions or labels |
| Label for a result | One positional read of its frame, then a dictionary decode, about 0.8 µs |
| Search | Word index loaded once; candidates ranked by label length, then only survivors read |
| Describe one concept | Only that concept's rows: a dozen small reads, 0.25 ms warm |
| Description filter | Whole description index loaded once, dictionary-coded in memory |
| Member filter | That reference set's table loaded once; row orders built per column on first use |
| History supplement | History section loaded once, keyed from both ends |

Compression shrinks the file, not the working set. A loaded section is held
decoded, so memory depends on which sections a workload touches. The
1,000-expression corpus peaks at 141 MiB; loading every semantic index, as the
10,000-expression corpus does, peaks at 266 MiB.

## What each technique saves

Packed UK edition, adding one technique at a time to fixed-width sections
compressed at zstd level 3:

| Technique | Packed |
|---|---:|
| Fixed-width sections, member UUID and refsetId stored, zstd level 3 | 388.7 MiB |
| Stop storing member UUID and refsetId | 298.0 MiB |
| Word postings as varint gaps | 269.8 MiB |
| Reference set membership as varint gaps | 258.9 MiB |
| Labels as dictionary frames | 236.1 MiB |
| zstd level 15, in parallel | 197.5 MiB |
| Member rows sorted by referenced component | 152.0 MiB |

None of these costs warm query latency: sections read whole decode into the
same arrays, and interleaved runs of the 1,000-expression corpus with and
without them could not be told apart. A label read costs about 0.2 µs more for
the decode; the first member filter on the largest table is faster, 53 ms
against 66 ms, because there is less to decompress.

The description arrays are the largest remaining section. Encoding their offsets
as lengths would save about 5 MiB but break the fixed-width reads that describe a
concept without loading the rest, so they stay as they are.
