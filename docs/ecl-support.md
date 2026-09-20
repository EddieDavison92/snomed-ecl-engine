# ECL support

What this engine evaluates today, against ECL 2.3. Anything unsupported fails
with an explicit error; no query returns a partial answer as a success.

## By feature

| Feature | Status | Notes |
|---|---|---|
| Concept literals, wildcard, eight hierarchy operators | Supported | Brief and long syntax, comments, optional terms |
| Boolean sets: `AND`, `OR`, `MINUS`, grouping | Supported | Comma conjunction and case-insensitive keywords |
| [Simple constraints and `memberOf`](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.1-simple-expression-constraints) | Supported | `^`, `^R`, wildcard refset selection, inactive concept members |
| [Refinements](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.2-refinements) | Supported | Nested attribute names and values, attribute groups |
| [Cardinality](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.3-cardinality) | Supported | Attribute and group cardinality, group zero, bounds beyond machine integers |
| Reverse attributes | Supported ungrouped | A reverse flag inside a group is an [open form](#open-questions) |
| Dotted attribute chains | Supported | Concept-valued chains and terminal concrete projections |
| Concrete values | Supported | Exact decimals, never binary floating point; case-sensitive strings, sets, Booleans |
| Top and bottom | Supported | Relative selection over the input set |
| Concept filters | Supported | Active status, definition status, module, effective time |
| [Description filters](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.8-description-filters) | Supported | Metadata always; term matching needs `--features unicode` |
| Dialects and acceptability | Supported | Standard aliases plus [configured aliases](cli.md#aliases) |
| [Member filters and projections](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.10-member-filters) | Supported | Typed fields from RF2 descriptors; returns concepts, scalar values or rows |
| [History supplements](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.11-history-supplements) | Supported | `HISTORY-MIN`, `-MOD`, `-MAX` and explicit subsets, one association step |
| Alternate identifiers | Supported | RF2 Identifier components with configured scheme aliases |

**History supplements add predecessors, not successors.** `X {{ + HISTORY-MOD }}`
returns `X` plus the inactive concepts that were replaced by it, so records coded
before an inactivation still match. To find what replaced an inactive concept,
project the association instead:

```text
^ [targetComponentId] 900000000000527005 {{ M referencedComponentId = 397709008 }}
```

**Term matching is optional.** A build without `--features unicode` rejects term
predicates explicitly rather than silently ignoring them. Metadata-only
description filters work in either build.

## Against the grammar

`scripts/conformance_inventory.py` checks every production in both official ABNF
grammars at commit `b0e07105ae395821bcc953f3d6084b57dc7bef2c`, recording the
result in [`validation/ecl-conformance.json`](../validation/ecl-conformance.json).
Both grammars have 180 productions:

| Status | Productions | Meaning |
|---|---:|---|
| Implemented | 18 | Exhaustive positive and negative syntax evidence |
| Partial | 161 | Implemented, with evidence that is not exhaustive across alternatives and lexical edges |
| Pending | 1 | `stringValue`: no evidence recorded in the inventory |

"Partial" describes how thorough the tests are, not whether the feature works.
The figure is not a percentage of the language, and parsing a production says
nothing about whether the engine evaluates it correctly. All 121 official syntax
examples parse.

## Open questions

Three grammar-valid forms have no clear meaning in the specification. The
parser returns a `Semantic` error for each, which is distinct from `Unsupported`.
Neither the error nor its name resolves the question, and each remains an
acceptance item.

**Reverse flag inside an attribute group**, as in `* : { R 363698007 = X }`. Both
grammars admit it. Reversal is defined through relationship source and
destination, while groups belong to the source, and the specification does not
say how those identities compose.

**Member filter without a refset operator**, as in `X {{ M active = true }}`. The
ABNF permits it; the logical model only describes filters applied to `memberOf`
results. [Upstream issue #10](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/10)
is unanswered.

**Reverse flag with a concrete value.** Reversal selects source concepts, and a
concrete value has no source concept. [Upstream issue #11](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/11)
is unanswered.

Two further restrictions are specified rather than open, and are enforced:
`memberOf` applies to concept-referencing reference sets, and a multiple-field
projection may only be the final operation.

## Spelling

Both `^R` and `^r` are accepted, as are `R`, `r` and case-insensitive
`reverseOf`. ABNF terminals are case-insensitive and the official parsing
guidance says keywords are; the pinned ANTLR grammar restricts `^R` to a capital
R and so disagrees with the ABNF here.

The brief grammar permits `\*` inside concrete strings where the long grammar
uses `escapedChar`. The parser accepts the brief form as a literal asterisk.
Both keep exact string equality: ECL 2.3 removed prefix and wildcard matching
from concrete values, and the former syntax is rejected.
