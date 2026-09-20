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

**A comparison on the broad corpus.** The 1,000-expression comparison has been
re-run: 879 of 1,000 expressions now match Snowstorm's complete code sets, up
from 719. The 10,000-expression corpus still has no comparison: several of its
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
