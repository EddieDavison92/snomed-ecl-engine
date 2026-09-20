# Preparation results

Status recorded on 19 September 2026. This is a preparation snapshot, not a performance comparison with a Rust engine.

## Verified dataset

The UK Monolith 42.5.0 archive passed its TRUD SHA-256 check. Its 26 files occupy 3,779,530,988 uncompressed bytes and contain 29,604,902 snapshot rows across component and refset files.

| Component | Snapshot rows | Active rows |
|---|---:|---:|
| Concepts | 1,151,519 | 838,955 |
| Relationships | 9,416,720 | 4,568,005 |
| Concrete relationships | 322,462 | 320,982 |
| Descriptions | 3,543,762 | 2,686,121 |

These are file row counts, not yet validated unique-component counts. The detailed inventory and module dependency rows are in ignored `data/rf2/uk_sct2mo_42.5.0_20260826000001Z.inventory.json`.

The composition module is dated 26 August 2026. Its dependency rows identify International core/model content dated 1 February 2026 and clinical extension content dated 29 July 2026. Do not substitute a newer International release when reconstructing this edition.

## OneLondon's Ontoserver

Eight complete, version-pinned probes succeeded against Ontoserver 6.25.4. They cover a literal, descendants, descendants-or-self, ancestors, children, parents, exclusion and an attribute refinement. Counts and SHA-256 digests are tracked in [ontoserver-baseline.json](../validation/ontoserver-baseline.json); actual code lists remain local.

## Local Snowstorm Lite

Snowstorm Lite 2.7.0 started with the same verified archive and exact edition URI. Import began at 22:21 UTC. At 22:28 UTC it had reached 24% of concept indexing. A sampled container-memory reading during import was 5.5 GiB within the 6 GiB cap. This is neither a measured peak nor a serving-memory figure.

Import completed at 22:39:10 UTC. Snowstorm Lite reported 1,057.257 seconds for the import, about 17.6 minutes, under this preparation run's settings. The index occupies 506,801,604 bytes. The watcher ran the local comparison successfully and stopped the container at 22:39 UTC.

The eight-query report is `data/validation/snowstorm-lite-smoke.json`. All eight complete result sets matched the version-pinned baseline from OneLondon's Ontoserver. The report contains small warm HTTP samples, not a controlled benchmark. Later Rust comparisons are recorded in [basic ECL](basic-ecl.md) and [refinements](refinements.md).

## Full Snowstorm update, 20 September 2026

Snowstorm 11.0.0 completed the same UK Monolith Snapshot import on MAIN at 01:12:47 UTC. Its import log reports 4,360 seconds, about 72.7 minutes. This includes Snowstorm's broader terminology and semantic indexing work; it is not an equivalent-work comparison with the current Rust importer.

The 1,000-expression comparison stopped after five HTTP 400 responses. All five were top/bottom expressions (`!!>` and `!!<`), which this Snowstorm build's parser rejected at the first `!`. Before the stop, 24 complete result sets matched Rust, 10 expressions were recorded as unsupported by Rust, and 961 remained unattempted. No result-set mismatches were recorded in that partial run, and no warm timing batches completed.

The partial report is `data/validation/ecl-1000-full-snowstorm.json`. Both containers stopped after the failed comparison; the Elasticsearch volume remains available. Resume with a serving-only Snowstorm container, not the original container's `--import` command. At that point the benchmark needed to distinguish comparison-server language rejections from transport failures and complete the remaining corpus.

The subsequent [full-corpus run](full-snowstorm.md) processed all 1,000 expressions and completed five timing batches. It recorded 719 complete matches, one concrete inequality discrepancy confirmed against OneLondon's Ontoserver in Rust's favour, 80 Snowstorm parser rejections and 200 Rust coverage gaps. Both services stopped after the run; no reimport was needed.

## Checks completed

The downloader ran successfully against TRUD and verified the archive. The inventory script processed all snapshot files. The Ontoserver wrapper retrieved and checked all probe results, including the exact returned edition. Python scripts compiled and PowerShell scripts passed syntax parsing. The local benchmark runner completed all eight probes with matching results.
