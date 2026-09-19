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

## OneLondon

Eight complete, version-pinned probes succeeded against Ontoserver 6.25.4. They cover a literal, descendants, descendants-or-self, ancestors, children, parents, exclusion and an attribute refinement. Counts and SHA-256 digests are tracked in [ontoserver-baseline.json](../validation/ontoserver-baseline.json); actual code lists remain local.

## Local Snowstorm Lite

Snowstorm Lite 2.7.0 started with the same verified archive and exact edition URI. Import began at 22:21 UTC. At 22:28 UTC it had reached 24% of concept indexing. A sampled container-memory reading during import was 5.5 GiB within the 6 GiB cap. This is neither a measured peak nor a serving-memory figure.

Import is still running at this recorded status. A background watcher waits up to 45 minutes from 22:28 UTC, runs `benchmark_local.py` after the import-complete message, and stops the container. It records success, failure or timeout in `data/baseline-status.json`. Successful import alone does not establish correct query results.

The eventual eight-query report is `data/validation/snowstorm-lite-smoke.json`. That report contains complete-result comparisons and small warm HTTP samples, not a controlled benchmark. Full Snowstorm and the Rust reference engines have not been run.

## Checks completed

The downloader ran successfully against TRUD and verified the archive. The inventory script processed all snapshot files. The OneLondon wrapper retrieved and checked all probe results, including the exact returned edition. Python scripts compiled and PowerShell scripts passed syntax parsing. The local benchmark runner has not yet completed against the importing server.
