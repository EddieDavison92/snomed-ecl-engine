# Roadmap

The engine evaluates ECL 2.3 across every feature area, so what is left is not
missing features. It is a handful of performance defects we can name and
measure, test evidence that is thinner than it looks, and a resource figure that
only covers the numeric core.

## Now

**Description filters still load the whole index.** The first description-filter
query in a process loads every description's metadata, up to 5,940 ms; later
ones take about 1 ms. Describing a concept no longer pays this: it reads that
concept's rows by position in about 0.2 ms. Filters could do the same when the
focus is small, reading only its concepts' rows, and load the index only for a
broad focus.

**Cold start.** Opening an index takes 94 ms uncompressed and 177 ms packed.
Opening checks that stored indexes are in range; it does not re-derive the
semantic invariants that import proved and the checksum protects. What is left,
measured with `examples/open_breakdown.rs`:

- *Decompression, about 83 ms of the packed figure.* zstd expands 21.8 MiB into
  the 86 MiB core before a query can run. Decoding blocks on demand, which the
  container format already supports through its per-block table and hashes,
  would move that cost to the queries that need those bytes.
- *Attributes the query never uses.* Attribute rows are roughly 35 MiB of the
  86 MiB core and only refinements need them. A hierarchy or Boolean query
  reads and decodes them for nothing. Making them lazy, as
  descriptions and member tables already are, would cut the core a query must
  touch to about 41 MiB.
- *Reading the core at all.* Memory-mapping an uncompressed index would make
  opening it close to free and let pages fault in on demand. It needs `unsafe`
  and careful alignment, and rules out compression, so it is a separate layout
  rather than a replacement.

The second is worth doing next: it halves what a serverless invocation must read
and is the difference between fitting a 128 MiB budget and not.

**A comparison on the broad corpus.** The 10,000-expression corpus has no
server comparison: several of its
expansions take minutes each to page out of Snowstorm, so that run has never
finished. Either sample it, or report engine-only figures for that corpus and
say why. See [benchmarks](benchmarks.md).

## Next

**Evidence depth.** 161 of 180 grammar productions carry non-exhaustive
evidence. Close the alternatives and lexical edges, starting with `stringValue`,
which has none recorded. See [ECL support](ecl-support.md#against-the-grammar).

**Full-engine resource measurement.** Current figures measure the numeric core
with data loaded on demand. Measure the complete engine with every semantic index
resident, including typed member tables and the Unicode backend, and report the
minimum allocation at which the whole workload completes.

**Warm-up.** A server's first scoped search or attribute refinement builds the
attribute inverse, about 20 ms, and its first search loads the word index, about
100 ms. The refset client asks for both at start-up; the engine could do it
itself for `batch --workers`.

## Blocked

Three forms are valid under the grammar but have no clear meaning in the
specification:

- a reverse flag inside an attribute group, `* : { R 363698007 = X }`
- a member filter with no refset operator, `X {{ M active = true }}`
- a reverse flag applied to a concrete value

Issues [#10](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/10)
and [#11](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/11)
cover the second and third, and both are unanswered. Until these are ruled on,
this engine cannot claim full ECL 2.3, and guessing an interpretation would
produce results that silently differ from other engines. The reasoning for each
is in [open questions](ecl-support.md#open-questions).

## Out of scope

This repository owns the library, index format, offline importer, CLI,
conformance tests and benchmarks. It gains no HTTP server, authentication, cloud
SDK, index distribution or deployment configuration: a deployment wrapper belongs
in a separate application that depends on this library.

The engine reads the published inferred relationship view. It does not classify,
and will not gain a reasoner.
