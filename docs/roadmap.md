# Roadmap

The engine evaluates ECL 2.3 across every feature area, so what is left is not
missing features. It is a handful of performance and memory costs we can name
and measure, three grammar defects to raise upstream, and a resource figure that
only covers the numeric core.

## Now

**Description filters on a broad focus load the whole index.** A focus of up to
1,000 concepts reads only its concepts' descriptions, about 0.2 ms each. A larger
focus loads every description's metadata once per process, up to 5,940 ms, and
later filters then take about 1 ms. Reading rows for larger foci in parallel, or
a metadata-only section, would narrow that further.

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

**Grammar defects upstream.** The grammar differential leaves nothing
unexplained, but three disagreements come from the ABNF itself: a comment
cannot close after a second star, quoted search terms admit comments, and no
space is required after a filter's type letter, so `{{moduleId = x}}` also reads
as a member filter. Raise them with the ECL specification's maintainers. See
[ECL support](ecl-support.md#grammar-differential).

**Memory for the full corpus.** The 10,000-expression corpus, which loads every
semantic index, now peaks at 268 MiB against 227 MiB, so it no longer fits the
256 MiB it used to. The attribute inverse costs about 16 MB and could shrink by
keying only the values that occur rather than every concept, and the full
description metadata remains the largest single load.

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
