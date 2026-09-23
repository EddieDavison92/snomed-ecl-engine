# Roadmap

The engine evaluates ECL 2.3 across every feature area, so what is left is not
missing features. It is a handful of performance and memory costs that can be
named and measured, three grammar defects to raise upstream, and three forms
the specification does not define.

## Now

**The first description filter in a process loads the whole index.** A focus of
up to 1,000 concepts reads only its concepts' descriptions, about 0.2 ms each. A
larger focus loads every description's metadata once per process, about 0.74 s
on one core, after which filters take about 2 ms. A serverless function pays
that on every invocation that needs it. Reading rows for larger foci in
parallel, or a metadata-only section, would narrow it.

**Cold start.** Opening an index takes 70 ms uncompressed and 155 ms packed on
one CPU. Opening checks that stored indexes are in range; it does not re-derive
the semantic invariants that import proved. What is left, measured with
`examples/open_breakdown.rs`:

- *Decompression, about 85 ms of the packed figure.* zstd expands 14.6 MiB into
  the 86 MiB core before a query can run. Decoding blocks on demand, which the
  container format already supports through its per-block table and hashes,
  would move that cost to the queries that need those bytes.
- *Attributes the query never uses.* Attribute rows are about 35 MiB of the
  86 MiB core, and only refinements need them. Making them lazy, as descriptions
  and member tables already are, would cut the core a hierarchy or Boolean query
  must read to about 41 MiB.
- *Reading the core at all.* Memory-mapping an uncompressed index would make
  opening close to free and let pages load on demand. It needs `unsafe` and
  careful alignment, and rules out compression, so it would be a separate layout
  rather than a replacement.

Lazy attributes are worth doing next: they halve what a serverless invocation
must read, which is the difference between fitting a 128 MiB budget and not.

**Workers do not scale.** On the 10,000-expression corpus, two library workers
are 26% faster than one, and four are slower than two. Find the contention
before recommending `batch --workers` beyond two. See
[benchmarks](benchmarks.md#engine-only-measurements).

## Next

**Grammar defects upstream.** The grammar differential leaves nothing
unexplained, but three disagreements come from the ABNF itself: a comment
cannot close after a second star, quoted search terms admit comments, and no
space is required after a filter's type letter, so `{{moduleId = x}}` also reads
as a member filter. Raise them with the ECL specification's maintainers. See
[ECL support](ecl-support.md#grammar-differential).

**Memory with every index loaded.** The 10,000-expression corpus, which loads
all description metadata and the other semantic indexes, peaks at 266 MiB and
needs a 320 MiB allocation; the target is 256 MiB. The peak is decoded
sections, not file cache. The attribute index that refinements use costs about
16 MB and could shrink by keying only the values that occur, and the
description metadata is the largest single load.

**Full-engine resource measurement.** Measure the engine with every section
resident at once, including all 582 member tables of the UK release and the
term-matching build, and report the smallest allocation at which that workload
completes.

**Warm-up.** A server's first scoped search or attribute refinement builds the
attribute index, about 20 ms, and its first search loads the word index, about
100 ms. `batch --workers` could build both at start-up.

## Blocked

Three forms are valid under the grammar but have no clear meaning in the
specification:

- a reverse flag inside an attribute group, `* : { R 363698007 = X }`
- a member filter with no refset operator, `X {{ M active = true }}`
- a reverse flag applied to a concrete value

Issues [#10](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/10)
and [#11](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/11)
cover the second and third, and both are unanswered. Until these are ruled on,
guessing an interpretation would produce results that silently differ from
other engines. The reasoning for each is in [open
questions](ecl-support.md#open-questions).

## Out of scope

This repository is the library, index format, offline importer, CLI,
conformance tests and benchmarks. It will not gain an HTTP server,
authentication, cloud SDKs, index distribution or deployment configuration:
those belong in the applications that use it.

The engine reads the published inferred relationship view. It does not classify,
and will not gain a reasoner.
