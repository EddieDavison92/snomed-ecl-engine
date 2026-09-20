# The 10,000-expression corpus

The [current corpus](../validation/ecl-10000.json) contains 10,000 distinct ECL
expressions across 45 categories. Its first 1,000 entries preserve the
[original corpus](../validation/ecl-1000.json), including IDs, categories and
expression text.

Each original category now contains 320 cases. A further 20 categories contain
100 cases each, including inclusive hierarchy operators, combined refinements,
reverse cardinality, concept metadata, dialects, term prefixes, wildcards, term
sets, history profiles and refset-containing queries.

The generator verifies the pinned UK RF2 archive and samples with seed
`20260826`. New historical projections use active REPLACED BY rows. Term cases
use description-derived prefixes in three quarters of cases and deliberate
non-matches in the remainder. The final run contains 75 non-empty results in
each term category and 280 non-empty member projections. The original 40 empty
projection cases remain regression cases.

## Measurements

The [initial baseline](../validation/ecl-10000-baseline-results.json) records
the engine before the latest ECL compatibility changes. The
[subsequent run](../validation/ecl-10000-completion-results.json) preserves all
10,000 complete result sets, including every original case. Its measurements are:

| Measurement | Result |
|---|---:|
| CPU and memory limits | One CPU, 256 MiB, no additional swap |
| Median request | 1.75 ms |
| Request p95 | 13.79 ms |
| Median 10,000-request batch | 35.05 seconds |
| Container-charged peak memory | 227.15 MiB |
| Empty / singleton / larger result sets | 2,158 / 5,375 / 2,467 |
| Largest result set | 111,171 concepts |

Each expression is enumerated once to record its complete-set digest, then
counted in five seeded shuffled batches through one persistent JSONL process.
Count requests still evaluate and materialise the complete result. Timings
include request transport. No result cache is enabled.

The index is the complete current UK packed index, with descriptions, displays
and typed members. Queries load data as needed. This workload does not touch
every possible member table or prove that every valid ECL query fits 256 MiB.
The Windows bind mount retained filesystem caches. Corpus regeneration and a
separate development build shared the host during the initial baseline. The
subsequent benchmark ran after this task's builds and other checks finished.
The runs do not establish a performance improvement attributable to the code
changes or an isolated comparison with a terminology server.

## Independent correctness checks

The [RF2 reference check](../validation/ecl-10000-rf2-results.json) independently
derives expected sets for 3,920 expressions. It traverses active inferred IS-A
relationships using Python sets and reads active REPLACED BY rows directly.
All counts and complete-set digests match the engine.

The remaining expressions have regression digests, with targeted independent
and specification-based tests elsewhere in the repository. They have not all
received independent RF2 validation. Increasing the corpus size does not prove
full ECL 2.3 conformance. Typed scalar and tuple outputs, negative syntax tests
and unsupported semantic combinations remain separate conformance tests.

## Reproduce the run

Run these commands from the repository root with the pinned RF2 archive and
Unicode-enabled Linux executable available:

```sh
python scripts/generate_corpus.py
python scripts/benchmark_corpus.py --binary target/linux-unicode/release/snomed-ecl-engine --store-directory data/compact-store/v2-uk-64.ecl --output data/validation/new-10000-run.json
python scripts/summarise_corpus.py --report data/validation/new-10000-run.json --prior validation/ecl-10000-completion-results.json --output validation/new-10000-results.json
python scripts/check_corpus_rf2.py --archive data/rf2/uk_sct2mo_42.5.0_20260826000001Z.zip --report data/validation/new-10000-run.json --output validation/new-10000-rf2-results.json
```

The benchmark defaults to 10,000 cases. Pass `--corpus validation/ecl-1000.json`
to reproduce older workloads. The generator accepts `--size 1000` for the old
workload. Both generated corpora were reproduced without byte differences.
When comparing a larger corpus to an older one, supply `--corpus` and
`--prior-corpus` to the summary script. It verifies the pinned corpus files,
preserved expression text and every previous complete result set.

Raw timing samples remain under ignored `data/validation/`. Tracked evidence
contains expressions, aggregate measurements and digests, without result IDs.
