# Full ECL acceptance

The finished engine must implement the complete ECL 2.3 language. The [production inventory](../validation/ecl-conformance.json) covers both official grammars at commit `b0e07105ae395821bcc953f3d6084b57dc7bef2c`. Regenerate or check it with `scripts/conformance_inventory.py`; hashes normalise text to UTF-8 with LF line endings.

Production names provide a coverage inventory, not a conformance score. Every production needs positive and negative syntax evidence. Each semantic rule also needs an independent expected result, including cases not supported by comparison servers. A final release cannot retain an unsupported standard feature or an unexplained mismatch.

Membership imports now retain reference set component types across active and inactive rows. Additional tests cover typed predicates, tuple restrictions and case-insensitive reverse operators. The [open forms](#open-grammar-forms) remain required work; diagnostics do not close them.

The [latest validation](../validation/ecl-completion-results.json) passes 89
Unicode-enabled tests, 65 tests without default features and all 121 official
syntax examples. All 10,000 corpus result sets are unchanged on a freshly
imported and verified packed UK index. Independent membership checks cover
inactive REFERS TO rows, and every PCD refset matches its active and inactive RF2
members. The broad inactive reverse-membership check uses a separate 2 GiB
allocation because it loads every typed table; it does not establish that path
at 256 MiB. Full-language resource limits and the open semantic forms remain
acceptance work.

The earlier [combined evaluation](../validation/combined-ecl-results.json) passes
84 Unicode-enabled tests, 60 tests without default features and all 121 official
syntax examples. All 1,000 preceding corpus sets are unchanged at one CPU and
256 MiB. Independent RF2 checks pass for 17 scalar cases, nine member queries and
a tuple, 52 history cases, ten term cases, 18 description cases, and every PCD
refset's active and inactive members. This integrates the member-typing changes
with streamed descriptions; it does not close the semantic gaps below.

| Required area | Data and behaviour needed | Current evidence or remaining work |
|---|---|---|
| Literals, wildcard, eight hierarchy operators and Boolean sets | All concept metadata, active parent/child graph, set operations and explicit grouping | Synthetic evaluator comparisons and 17 version-pinned queries against OneLondon's Ontoserver |
| Syntax details | Brief and long syntax, comments, whitespace, annotations, lexical boundaries and invalid input | Basic parser tests, official-example classification and `tests/lexical.rs`, which adds valid and invalid forms for lexical rules, member value lexemes and keyword spellings. Inventory entries citing it are partial syntax evidence, not proof that each rule is exhaustively covered |
| Top/bottom | Relative hierarchy selection over input sets | Implemented; synthetic overlapping-set checks and corpus cases |
| Alternate identifiers | Identifier components, scheme aliases and resolution | Implemented with lazy RF2 Identifier storage, configured aliases, exact codes and synthetic import/CLI checks. See [aliases](aliases.md) |
| Nested refinements | Attribute type/value expressions and comparisons | Implemented; synthetic fixtures and OneLondon's Ontoserver comparisons |
| Cardinalities and groups | Exact multiplicity, group zero, group identity, zero/finite/unbounded ranges | Published inferred rows evaluated; adversarial group/absence fixtures. Does not normalise arbitrary redundant relationship input |
| Reverse and dotted attributes | Reverse relationships and successive value projections | Ungrouped reverse counts distinct sources. Concept-valued dot chains and terminal concrete projections are implemented; concrete numbers retain exact precision. A reverse flag inside an attribute group or with a concrete value is an [open semantic form](#open-grammar-forms) |
| Concrete values | Exact numeric comparison, strings, sets and Boolean semantics | Exact decimals without floating point; exact case-sensitive strings/sets and Booleans tested. ECL 2.3 removed concrete prefix/wildcard matching; regression tests reject the former syntax |
| Membership and containing-any | Concept membership and reverse membership | Implemented for active concept-referencing rows, including inactive concepts; synthetic tests, 45 matches against OneLondon's Ontoserver, independent RF2 scans and PCD checks. Member filters can select inactive rows |
| Concept filters | Active status, definition status, module and effective time | Implemented with metadata already in the core; synthetic fixtures and 50 complete matches against OneLondon's Ontoserver. Three reference rejections are recorded separately |
| Description filters | All descriptions, language memberships, term matching, wildcards, type, dialect, acceptability, IDs and metadata | Metadata predicates have 18 independent RF2 matches. The optional ICU backend evaluates word prefixes, wildcard sets and negation, with synthetic Unicode checks and ten independent RF2 matches. Standard and configured dialect aliases are implemented; broader collation/lexical coverage remains required |
| Member filters and projections | Typed fields, metadata, comparisons and non-concept result types | Descriptor-driven decimal/date/UUID fields and configurable member collation have synthetic checks. Standard predicates, concept projections and terminal typed rows are implemented. Identifiers naming no substrate concept, `component` values, integer lexeme validation with exact promotion beyond 64 bits and grammar-valid predicate lexemes have synthetic tests, an independent row-scan evaluator and [RF2 orphan checks](../validation/orphan-member-results.json). The treatment of absent identifiers is a documented interpretation. `^` on a description-based reference set and member filters without `^` or `^R` are discussed under [restrictions and open forms](#open-grammar-forms). See [member filters](member-filters.md) |
| History supplements | Historical association members, defined profiles and explicit subsets | Implemented with one-hop profile/subset evaluation and reversed MOVED FROM handling. See [history](history.md) |
| Resource behaviour | Bounded work and memory, cancellation, complete results and concurrency policy | Sequential limits tested; full-language resource and concurrency measurements pending |

The earlier [typed-set corpus run](../validation/types-corpus-results.json) evaluates all 1,000 expressions with every previous complete set unchanged. This is workload coverage, not a language-conformance score. Its generated cases omit many combinations and lexical edges. The [additional evaluation checks](../validation/schema-results.json) cover nine scalar projections, nine member queries, a tuple projection, 52 history queries, fresh checks against OneLondon's Ontoserver, and every PCD refset's active and inactive members. All 121 official example files parse. Parsing does not prove correct evaluation.

The [type and lexical checks](../validation/types-results.json) add heterogeneous scalar sets, exact field types across refsets and cardinality bounds beyond machine integers. All 13 independent scalar checks and 64 Unicode-enabled tests pass. Continue with the remaining projection/type semantics. Continue testing lexical and collation details. Choose indexes based on these semantics and measured query work. Full Snowstorm is a comparison option where Lite lacks a feature. Requests to OneLondon's remote Ontoserver remain small correctness probes.

The pinned brief grammar permits `\*` in concrete strings through `escapedWildChar`; the long grammar uses `escapedChar` there. The parser accepts the brief form as a literal asterisk. Both forms retain exact string equality. Prefix and wildcard search syntax remains confined to text filters. Invalid tokens produce syntax errors rather than obsolete unsupported-feature messages.

Cardinality bounds may exceed machine integer ranges. The parser checks their original decimal order before capping values above `u64::MAX`. Such bounds exceed every possible row/group count in this format, whose offsets are `u32`; this preserves evaluation on the finite RF2 store.

Nested typed sets now feed concept operations when every remaining value is a concept, including the empty set. Regression cases cover hierarchy, extrema, refinements, filters, member predicates, dotted attributes, numeric output order and limits. The parser accepts adjacent member markers such as `{{Mactive=0}}`, as permitted by the grammar's optional whitespace. The quoted `active="*"` form shown in the [description-filter examples](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.8-description-filters) is accepted consistently in concept, description and member filters, alongside the grammar's unquoted wildcard.

The [nested-query evaluation round](../validation/nested-results.json) passes 67 Unicode-enabled tests, 47 tests without default features and 17 independent RF2 checks. All 121 official examples still parse. The [new corpus run](../validation/nested-corpus-results.json) preserves all 1,000 previous complete sets.

## Open grammar forms

`Semantic` errors distinguish unresolved or disallowed combinations from missing
engine capabilities (`Unsupported`). Changing that diagnostic does not establish
conformance. Grouped reverse attributes and member filters without a refset
operator remain open ECL 2.3 acceptance items.

**Grouped reverse attributes.** Both pinned ABNF grammars admit a reverse flag
inside a group. The specification defines reversal through relationship source
and destination concepts, while groups belong to the source. It does not explain
how those group identities compose when selecting destinations. The logical
model permits reverse attribute-name modifiers without resolving that question.
The parser and evaluator return `Semantic`; this remains an unresolved normative
gap. See [refinements](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.2-refinements)
and [cardinalities](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.3-cardinality).
The [local boundary probes](../validation/semantic-boundary-queries.json) record
comparison-server behaviour, which does not define the language.

**Member filters without an operator.** The ABNF permits member filters when the
refset operator is absent. The logical model describes filters on memberOf
results, without defining the operator-free form. The parser returns `Semantic`.
The upstream [question about this grammar form](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/10)
remains unresolved. This is still a conformance item, not an implemented operator.

**Reverse concrete values.** The grammar admits reversal with scalar values,
but reversal is described in terms of destination concepts. Such combinations
return `Semantic`. The [upstream reverse-value question](https://github.com/IHTSDO/snomed-expression-constraint-language/issues/11)
has no settled rule; the engine does not invent scalar relationship sources.

**Concept-based membership.** Section 6.1 limits membership to concept-referencing
refsets and also defines wildcard selection across refsets. New imports record
`concept_refsets` and `non_concept_refsets` in the membership manifest. RF2
descriptors supply declared component types; otherwise rows of every active
state provide the evidence. A selection containing only known non-concept
refsets returns `Semantic`. Mixed and wildcard selections retain concept members.
In particular, inactive concept rows remain queryable when a refset also has
description rows. Older manifests still open but cannot diagnose non-concept
refsets without the new metadata. See [simple constraints](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.1-simple-expression-constraints)
and [member filters](member-filters.md).

**Tuple projections.** Section 6.1 restricts multiple-field projections to the
final operation. Tuple operands inside other operators return `TypeMismatch`.
Synthetic checks cover Boolean sets, hierarchy, refinements, member predicates,
filters, dotted attributes and history. This is a stated restriction, unlike the
unresolved grouped-reverse semantics above.

**Operator spelling.** Both `^R` and `^r` are accepted. Reverse attributes accept
`R`, `r` and case-insensitive `reverseOf`, including adjacent concept IDs. The
ABNF terminals and [official parsing guidance](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/implementation-considerations/7.2-parsing)
permit case-insensitive spelling. The pinned ANTLR grammar restricts `^R` to a
capital R, so it disagrees with the ABNF on this point.

The [packed-index evaluation round](../validation/packed-results.json) preserves
all 1,000 complete corpus sets across the directory, raw container and compressed
container. It passes 72 Unicode-enabled tests, 49 tests without default features,
121 official syntax examples and independent scalar, member, history, term,
description and PCD checks. These checks show that compression preserves current
semantics. They do not close the grouped-reverse or implicit-member-filter forms above.

The [member semantics round](../validation/orphan-member-results.json) records how absent identifiers and non-concept values behave. Default membership returns concepts present in the indexed release; other projections preserve identifiers, and concept operations reject missing references. This remains a documented interpretation. Integer fields retain exact lexical validation after numeric promotion. Grouped reverse, operator-free member filters and the other unresolved semantic questions above still prevent a full ECL 2.3 claim. The ICU backend has English, Swedish and Danish checks; broader collation evidence remains separate from the generated corpus.

The [concept-filter probes](../validation/ontoserver-concept-filters.json) compare complete sets for every expression OneLondon's Ontoserver accepted. Its HTTP 422 responses for a nested module expression and two empty-date predicates are not successful comparisons. Synthetic fixtures cover these valid forms, predicate combinations, inactive concepts, missing indexes and evaluation limits. Concept filters add no persistent index bytes.

[Description predicates](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.8-description-filters) such as `< 64572001 |Disease| {{ term = (match:"gas" wild:"*itis")}}` evaluate with `--features unicode`. The [term comparison](descriptions.md#term-comparison-evidence) records definition-scope and inequality differences against OneLondon's Ontoserver. Numeric queries do not load description data. Include both text data and the Unicode backend in full-engine resource measurements.
