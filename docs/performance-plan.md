# Performance experiments after membership

Claude CLI reviewed the supplied evaluator, store and benchmark code using `claude-fable-5-1` on 20 September 2026. It ran without tools or write access through a text-only advisory request. The recommendations below were checked against the code; they are proposals, not measured speedups. The local raw response stays in ignored `.local/`.

## First experiments

1. Avoid whole-index work for small hierarchy and dotted results. Record visited/result ordinals, reuse bounded scratch storage and clear only touched entries. Compare sorting small results with scanning the current marker vector. Preserve overlapping seeds, direct operators and sorted unique output.
2. Stop attribute and group counting once the result is certain. A finite maximum fails as soon as it is exceeded; an unbounded maximum succeeds once its minimum is met. Prepare and validate every subexpression before skipping row scans. Zero cardinalities, group identity, reverse distinct-source counts and unsupported errors must remain correct.
3. Use existing row order. Attribute rows are sorted by group, type and value. Restrict group and single-type scans to their slices. Merge ordered group IDs from ordinary and concrete rows instead of allocating and sorting per candidate.
4. Reduce repeated preparation. Hoist is-a resolution, specialise single-value membership tests, reuse reverse-target scratch storage and avoid repeated allocation when parsing stored decimal values.

Measure each change separately against unchanged code-set digests, work limits and cancellation tests. Include tiny and broad hierarchy sets, absent attributes, zero/finite cardinalities, groups, concrete inequality and reverse duplicate rows. Report internal evaluation and full request time separately.

## Candidate rejection

A type-to-source posting list could start a positive refinement from concepts that actually have the required attribute. Candidate rejection is safe only where the attribute must exist. A minimum of zero, including `[0..0]`, cannot use that restriction. Inequality still requires a matching row with a different value; absence is not inequality.

For conjunction, any necessary condition may narrow candidates. For disjunction, every alternative needs a valid restriction before their union can restrict the result. Handle is-a separately because it is stored as graph edges. Compare posting-list storage cost with query savings before adding it to the format.

For a small left-hand set intersected with a broad descendant expansion, test each candidate's ancestor path instead of materialising the whole expansion. Choose a measured threshold and preserve normal evaluation as the fallback. Top-of-set queries may benefit from the same approach.

## Storage and startup

Measure read, checksum, decode and structural validation separately. Bulk section decoding or a hashing reader may reduce duplicate passes. Keep bounds, ordering, graph and type checks: a matching checksum does not establish that an arbitrary file is structurally valid.

Try dictionary-coded module/date columns and narrower group/type columns in a separately versioned format. Retain an overflow representation or adaptive width; rejecting legitimate RF2 values merely to fit a byte is unacceptable. Measure resident memory, compressed distribution size, import peak and cold-open latency independently.

Compare LTO, code-generation units and size optimisation in query-only builds. The consuming application should decide panic behaviour. Do not change the library's behaviour solely for one deployment wrapper.

Full ECL needs text and typed member data. A description-filter index must preserve relevant descriptions, not just displays, and support `match:` / `wild:` search with the specified language and collation rules. Keep that index separate from numeric query data. Publish final resource claims only after descriptions, dialects, filters, history and projections are implemented.

## Current implementation choices

Membership uses one stored sorted list per refset. A single-refset lookup binary-searches its key; unions use bounded packed marker words. Containing-any checks the smaller candidate/member list against the larger and stops at its first intersection. No full reverse membership copy or result cache is stored.

The supplementary loader reads displays sequentially when remapping ordinals. It does not issue a file seek for every concept. These choices are covered by complete-set validation. Larger evaluator changes should follow the experiments above, independently of ECL feature additions.
