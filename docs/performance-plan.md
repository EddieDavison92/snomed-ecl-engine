# Compact storage and evaluation plan

Full ECL correctness comes first. The next substantial performance work should change the representation before tuning its scans. Extend the independent test evaluator to refinements, groups, cardinalities and membership before that rewrite. Keep complete release-set digests, typed tuple tests, cancellation and resource-limit checks as acceptance tests.

Claude Fable 5.1 analysed the actual UK description index on 20 September 2026. The ignored `.local/analyse_store.py` and `.local/analysis.json` contain the script and output. Its findings change the order of the earlier evaluator experiments below.

## What the storage analysis establishes

These figures use decimal MB and the pinned UK Monolith 42.5.0, before typed member tables. Compression byte counts are Python measurements. Packed layouts and posting sizes are estimates, not Rust runtime measurements.

| Item | Observed data | Proposed implication |
|---|---|---|
| Numeric core | 90.26 MB; 7 modules, 341 dates, 127 attribute types, maximum group 30 | Dictionary coding and adaptive widths estimate 64.79 MB |
| Description text | 230.77 MB raw; 28.44 MB with independent 16 KiB zstd level-3 blocks | Test block compression without a trained dictionary first |
| Trained dictionary | 26.79 MB compressed blocks, before dictionary and lookup overhead | About 1.66 MB saved; defer the extra format complexity |
| Description metadata | 3.56 million rows, one language and four language refsets | Pack common values, with wider representations for other editions |
| Display file | 68.95 MB | Test a description-row pointer per concept, preserving display selection |
| Tokens | 155,947 distinct tokens; 12,493,716 concept-level postings | Build and measure the candidate index before accepting a size estimate |
| Descendant ordering | Disease falls from 42,738 SCTID-ordered runs to 16 DFS-ordered runs | Test exact interval sets on a reordered DAG |

The proposed 195 MB complete file is not yet supported by measurements. It excludes typed refset tables and unfinished semantic data. It also needs section tables, dictionaries, offsets, checksums and lookup permutations. Sorting description IDs separately, for example, needs a way back to their description rows. The sampled transitive-closure estimate is unreliable and must not set a memory budget.

The [complete hierarchy scan](../validation/hierarchy-layout-results.json) now
counts every descendant-or-self set in this release. It finds 12,768,452 pairs
across 1,151,519 concepts. A root-first DFS forest, stably partitioned into active
then inactive concepts, reduces the interval count from 8,095,879 to 2,044,534.
Median, p95 and p99 counts are 1, 2 and 10 intervals; the maximum is 30,344.
This supports testing the layout but does not establish a runtime speedup.

Uncompressed interval endpoints need 16,356,272 bytes, plus 4,606,080 bytes of
offsets and 4,606,076 bytes for one permutation. These are extra structures, not
the complete index. Limiting stored lists to 64 intervals would need 11,999,912
endpoint bytes and leave 2,111 concepts for traversal. Measure both strategies
before choosing one; avoid a fallback threshold based only on the median.

Reproduce the count with the existing index, without changing its bytes:

```sh
cargo run --locked --release --no-default-features --example measure_hierarchy_layout -- INDEX
```

The example follows every hierarchy edge, including multiple parents, checks
that the permutation is bijective, and retains inactive and disconnected
concepts. It does not load descriptions or typed member rows. The existing
evaluator still uses SCTID-ordered ordinals.

Keep the comparisons with Snowstorm and Snowstorm Lite, including matched per-request medians. Describe their broader server responsibilities alongside resource measurements. Hermes is also a useful architectural comparison for an embedded engine. Claims about any product's complete ECL coverage need version-pinned evidence. OneLondon's Ontoserver remains a modest correctness comparison, not the definition of correct ECL.

## Revised experiment order

1. Extend the independent evaluator with generated refinement, group, zero/finite cardinality, inequality and membership cases. Include typed member rows and history as those semantics land. Compare sets or tuples, not totals alone.
2. Prototype a versioned section container with current encodings to measure container and mmap effects separately. Put numeric data first and declare optional sections explicitly. A numeric-only artefact needs its own valid section table; arbitrary truncation must fail. Measure open, first query, page faults and charged memory. Keep the current reader until the prototype proves useful.
3. Test DFS ordinals and adaptive widths. Preserve every parent in the DAG, stable external SCTIDs and numeric output ordering. Measure exact interval counts for all concepts, including the heavy tail, before choosing stored closure or traversal. An active-concept prefix needs an explicit partition and preserved hierarchy semantics. Root descendants are not the same set as all concepts.
4. Compress descriptions and typed members. Compare 16 and 64 KiB zstd blocks, decode implementations and bounded block caches. Include locators, long terms, UTF-8 boundaries, description IDs, dialect metadata and display pointers in total costs. Measure large map and OWL member tables too. A numeric-only benchmark cannot establish a 256 MiB budget for the full engine.
5. Add conservative text candidates. Prefix postings may reduce verification work. Candidate generation must contain every result allowed by ECL, including inactive descriptions when selected. Folded tokens need locale tests before they can safely discard candidates. Retain ICU verification until a replacement passes independent Unicode and language-tailoring checks.
6. Measure reverse type/value indexes, reusable bounded scratch storage, subexpression caching and selective conjunction ordering. Charge cached results to a bounded session budget. Recheck performance and cancellation after each change.

Each step needs unchanged correctness results and separate measurements for import time, import peak memory, file size, distribution size, cold open, first evaluation and warm per-request median/p95. Count filesystem cache in container memory. Deployment download and provider cold-start measurements belong to the separate application repository.

## Format and candidate-selection conditions

The [single-file container](container.md) now preserves current component
encodings behind a common reader. It supports raw sections and independent
16/64 KiB zstd blocks, including every typed member table. This isolates file
layout and compression from evaluator semantics. Normal opening retains checksum
and structural checks; `verify` checks all cold sections too. Description metadata
now uses adaptive dictionaries in memory, repeated dialect combinations share
lists, and term text uses an on-disk reader. The unchanged 1,000-query corpus
peaked at 219.4 MiB under a 1 GiB limit, compared with 542.6 MiB before this change.
It also passed under 256 MiB. See the [description measurements](descriptions.md#compact-runtime-measurements).
Persistent compact columns, mmap, DFS ordinals, description row pointers, term
postings and bounded loading of large typed member tables remain unimplemented.

The independent evaluator now checks 1,000 generated combinations of refinements, groups, cardinalities and membership in `tests/basic_ecl.rs`, using separate row scans in `tests/support/mod.rs`. Concrete and typed scalar semantics have their own fixtures and independent RF2 checks. This supplies comparison evidence for later rewrites; remaining ECL semantics still need completion first.

The [startup probe](../validation/startup-results.json) found a separate I/O defect before the format rewrite. Deserialising the 199,749-byte manifest directly from `File` issued 199,750 reads. `BufReader` reduces that to 26 reads and returns identical data. Direct store opening fell from 21.75 seconds to 0.91 seconds in the paired probe on the Windows Docker mount. These are single store-open samples with filesystem caches retained; manifest parsing has six samples per method. No checksum or structural validation was removed, and no index bytes changed. The latest corpus process opened in 1.24 seconds, including its second manifest read. Use this corrected baseline for subsequent container and mmap experiments.

Small values in this release justify adaptive encodings, not fixed UK limits. Wider modules, dates, groups, languages, dialects and custom fields must remain representable. Preserve inactive descriptions, member rows, association targets, exact decimals and configured identifier schemes required by full ECL.

An mmap reader needs a reviewed safe interface around mapping creation, bounded accessors and a rule that mapped files cannot be mutated. A dependency does not remove the caller's unsafe obligations. Combining files alone does not reduce resident memory.

Measure checksums and structural validation separately. Import and an explicit `verify` command can do exhaustive checks, but lazy opening still needs bounds, schema and integrity checks before using each section. Specify the corruption guarantees for unopened sections and how a pinned artefact becomes trusted. A hash does not prove structural validity.

Missing optional text is acceptable for numeric queries. Missing data required by a requested operator must return an error. Separately delivered terms must remain tied to the numeric edition and manifest. Measure the consuming deployment's current package limits before making a bundling claim.

Prefix postings for `match:"gas"` cannot restrict the independent `wild:"*itis"` branch of an OR expression. That branch needs its own complete candidate strategy or a bounded scan. Single-digit milliseconds for the combined query is an experiment, not an established result.

## Earlier evaluator review

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

Manifest and identifier JSON writes now use buffers and explicitly flush before
syncing their files. In the [PCD import check](../validation/buffered-import-results.json),
the same packed base and supplement took 81.36 seconds before this change and
49.08 seconds afterwards. Both runs used two CPUs and 2 GiB; filesystem caches
were retained and the host was shared. The resulting manifest bytes and every
component checksum are identical. These are single observed runs, not a general
import-speed estimate.

The supplementary loader reads displays sequentially when remapping ordinals. It does not issue a file seek for every concept. These choices are covered by complete-set validation. Larger evaluator changes should follow the experiments above, independently of ECL feature additions.
