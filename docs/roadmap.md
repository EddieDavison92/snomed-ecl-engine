# Roadmap

The engine evaluates ECL 2.3 across every feature area. What remains is depth of
evidence, one known performance defect, and resource measurement for the
complete engine rather than for the numeric core alone.

## Now

**Subsumption cost.** Every hierarchy operator allocates two dense marker arrays
sized to the whole store and scans all of it, so `<< 24700007` costs the same as
a query returning 100,000 concepts. Against the default work budget that caps one
expression at about 43 subsumption operators, which a codelist-derived union
reaches easily. The fix is to make the cost proportional to nodes touched:
generation-stamped markers reused across the query, and results collected from
the touched set. Raising the budget would hide it.

**Cold start.** Opening a packed index takes 525 ms, and 285 ms uncompressed.
A serverless invocation pays that before it answers anything. Measured with
`examples/open_breakdown.rs`, the cost splits four ways, and each has a
different fix.

- *Structural validation, 88 ms.* Every open re-derives graph invariants,
  including a topological sort for acyclicity, on bytes whose SHA-256 already
  matches the manifest. The same bytes passed those checks when the index was
  written. Skipping validation when the section hash matches keeps corruption
  detection, which the checksum does better, and drops the work. `verify` keeps
  the full checks.
- *Reading and decoding the core, 145 ms uncompressed and 460 ms packed.* The
  decoder builds each vector one element at a time through a per-element
  `Result`: 1.15 million concept IDs, 1.6 million edges, 2.96 million attribute
  rows. Decoding in bulk from the byte buffer would vectorise.
- *Attributes the query never uses.* Attribute rows are roughly 35 MiB of the
  86 MiB core and are only needed for refinements. A hierarchy or Boolean query
  loads, checksums and validates them for nothing. Making them lazy, as
  descriptions and member tables already are, would cut all three costs for the
  common case.
- *Everything else.* Memory-mapping an uncompressed index would make opening it
  close to free and let pages fault in on demand. It needs `unsafe` and careful
  alignment, and it rules out compression, so it is a separate layout rather
  than a replacement.

The first two are worth doing before the last two. Together they should put a
hierarchy query well under 100 ms.

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

**Concurrency.** The library runs four workers over one shared index. The CLI is
sequential and the batch process handles one request at a time.

## Blocked

Three forms are valid under the grammar but have no settled meaning in the
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
