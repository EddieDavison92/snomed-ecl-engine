# Full ECL acceptance

The finished engine must implement the complete ECL 2.3 language. The [production inventory](../validation/ecl-conformance.json) covers both official grammars at commit `b0e07105ae395821bcc953f3d6084b57dc7bef2c`. Regenerate or check it with `scripts/conformance_inventory.py`; hashes normalise text to UTF-8 with LF line endings.

Production names provide a coverage inventory, not a conformance score. Every production needs positive and negative syntax evidence. Each semantic rule also needs an independent expected result, including cases not supported by comparison servers. A final release cannot retain an unsupported standard feature or an unexplained mismatch.

| Required area | Data and behaviour needed | Current evidence or remaining work |
|---|---|---|
| Literals, wildcard, eight hierarchy operators and Boolean sets | Active concept metadata, parent/child graph, set operations and explicit grouping | Synthetic evaluator comparisons and 17 version-pinned OneLondon queries |
| Syntax details | Brief and long syntax, comments, whitespace, annotations, lexical boundaries and invalid input | Basic parser tests and official-example classification; lexical edge coverage remains partial |
| Top/bottom | Relative hierarchy selection over input sets | Pending |
| Alternate identifiers | Identifier components, scheme aliases and resolution | Pending |
| Nested refinements | Attribute type/value expressions and comparisons | Grouped relationship rows stored; evaluator pending |
| Cardinalities and groups | Exact multiplicity, group zero, group identity, zero/finite/unbounded ranges | Storage preservation tested; query semantics pending |
| Reverse and dotted attributes | Reverse relationships and successive value projections | Pending |
| Concrete values | Exact numeric comparison, strings, sets and Boolean semantics | Numeric/string spelling retained; typed comparison and Boolean support pending |
| Membership and containing-any | Typed member records, referenced-component resolution and reverse membership | Pending |
| Concept filters | Active status, definition status, module and effective time | Metadata stored; filter semantics pending |
| Description filters | All descriptions, language memberships, term matching, wildcards, type, dialect, acceptability, IDs and metadata | Preferred displays alone are insufficient; semantic indexes and evaluation pending |
| Member filters and projections | Typed fields, metadata, comparisons and non-concept result types | Pending |
| History supplements | Historical association members, defined profiles and explicit subsets | Pending |
| Resource behaviour | Bounded work and memory, cancellation, complete results and concurrency policy | Sequential limits tested; full-language resource and concurrency measurements pending |

The next implementation step adds membership and refinements while retaining this complete scope. Choose indexes based on those semantics and measured query work. Full Snowstorm is required as a comparison where Snowstorm Lite lacks a feature. Remote OneLondon requests remain small correctness probes.
