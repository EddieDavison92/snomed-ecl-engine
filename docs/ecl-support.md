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
| Dialects and acceptability | Supported | Standard aliases plus [configured aliases](indexes.md#query-configuration) |
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

**A member's UUID and refsetId are not fields.** [Appendix E](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/appendices/appendix-e-reference-set-fields)
names reference set fields from `referencedComponentId` on, and 6.10 gives
`moduleId`, `effectiveTime` and `active` their own filters; nothing gives the
member `id` a meaning. The index does not store it or `refsetId`, so
`^ [id] X` or `{{ M refsetId = … }}` fails with `InvalidField`, as any field the
reference set lacks does.

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
| Implemented | 179 | Every alternative, optional part and repetition count exercised by the grammar differential below, with no unexplained disagreement |
| Pending | 1 | `stringValue`: no other rule refers to it, so no expression can contain it |

The figure describes syntax evidence, not a percentage of the language, and
parsing a production says nothing about whether the engine evaluates it
correctly. All 121 official syntax examples parse.

### Grammar differential

`scripts/grammar_differential.py` builds a recogniser from each pinned ABNF
alone, generates sentences covering every alternative, optional part and
repetition count of every production, mutates them into near misses, and
compares the recogniser's verdict with the parser's. Results are in
[`validation/grammar-differential.json`](../validation/grammar-differential.json).

| Grammar | Samples | Grammatical | Disagreements | Unexplained |
|---|---:|---:|---:|---:|
| Brief | 17,607 | 5,427 | 20 | 0 |
| Long | 17,620 | 5,427 | 29 | 0 |

A second seed also leaves none unexplained. A parser refusal of kind
`Semantic`, `Limit` or `Unsupported` counts as accepting the syntax. Every
remaining disagreement arises only under the literal ABNF, in one of three
places where the text of the ABNF and the specification part company:

- **A comment cannot close after a second star.** `comment` pairs each star
  with the byte after it, so `/***/` never ends. The parser ends a comment at
  its first `*/`.
- **Quoted search terms admit comments.** `matchSearchTermSet` puts `ws`, which
  includes comments, inside the quotes, so `"a/*b*/c"` would be two words. The
  parser treats the quotes as holding text.
- **An untyped filter is a description filter.** Because no space is required
  after a filter's type letter, the ABNF also reads `{{moduleId = x}}` as the
  member filter `M` on a field named `oduleId`. 6.8 settles it: "If the type of
  a filter constraint is not specified … it is assumed that the constraint is a
  description constraint." So `^ X {{moduleId = *}} {{M active = 1}}` is a
  syntax error, since a member filter cannot follow a description filter.

The script's second recogniser applies these three readings, and only a
disagreement that survives it counts as unexplained.

A refusal is reported only once the whole text parses, so malformed text is a
`Syntax` error. Reversed cardinalities, impossible dates and unbracketed mixes
of conjunction and disjunction are grammatical and refused as `Semantic`: 6.4
requires brackets because the grammar derives such a mix more than one way.

## Open questions

Three grammar-valid forms have no clear meaning in the specification. The
parser returns a `Semantic` error for each, which is distinct from `Unsupported`,
until the specification settles them.

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
