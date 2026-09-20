# Member filters and typed results

The importer retains concept-based refset rows, including inactive rows, in checksum-verified `members/<refset>.bin` files. Each file loads on first use and remains cached for the store's lifetime. Ordinary membership queries keep using the smaller active-membership index. Old indexes without member files still support their previous queries; member filters and projections require reimport.

## Supported queries

Member predicates apply to the same RF2 row. Multiple member-filter blocks also constrain that row. Members are active by default; an explicit `active` predicate changes that selection. Referenced concepts may be inactive.

```text
^999002271000000101 {{M mapGroup=#1,mapPriority=#1,mapTarget=wild:"J459"}}
^900000000000527005 {{M targetComponentId=195967001}}
^[targetComponentId]900000000000527005 {{M referencedComponentId=67415000}}
^900000000000534007 {{M sourceEffectiveTime>="20260101"}}
```

Supported field types are concept IDs, integers compared with exact decimal predicates, strings, Booleans, dates and member UUIDs. Metadata includes `active`, `moduleId`, `effectiveTime`, `refsetId` and `referencedComponentId`. String predicates require the `unicode` Cargo feature. Member strings currently use English collation because RF2 member rows do not declare a language.

The importer reads `c`, `i` and `s` field representations from RF2 filenames. It recognises standard `*EffectiveTime` fields as dates and MRCM `grouped` as Boolean. General descriptor-driven custom types, arbitrary decimal-valued member columns and configurable member-string collation remain required.

The grammar also permits member-filter forms without an explicit refset operator. Their evaluation remains unsupported. This milestone handles member filters attached to `^` or `^R`.

The [member-filter specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.10-member-filters) defines predicate behaviour. The [simple-expression specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.1-simple-expression-constraints) defines field projection and tuple restrictions.

## Library and CLI results

`eval::evaluate_result` and `eval::evaluate_result_with_limits` return `QueryResult`:

- `Concepts(Vec<u32>)` contains sorted unique concept ordinals, including single concept-field projections.
- `Rows(Vec<BTreeMap<String, MemberValue>>)` contains typed fields for terminal non-concept or multi-field projections. Each matching member produces a row.

`MemberValue` distinguishes `concept`, `number`, `boolean`, `time` and `string`. JSON encodes concept IDs and exact numbers as strings. The existing `evaluate` functions accept concept-valued results only. A tuple in a concept subquery returns `TypeMismatch`, even when no member could match. Non-concept scalar set algebra remains unsupported.

```text
^[referencedComponentId,mapTarget,mapGroup]999002271000000101 {{M mapGroup=#1,mapPriority=#1,mapTarget=wild:"J459"}}
```

`expand` prints typed rows as JSONL. `--count` counts rows for a row-valued result; `--display` rejects that result before printing anything. A batch response uses `result_type: "rows"` and a `rows` array instead of `codes`. `count_only` omits the array. Concept responses retain their existing schema.

`[*]` selects all non-metadata fields, including `referencedComponentId`. Unknown field names return `InvalidField`. Invalid field/value combinations return `TypeMismatch`. Work, cancellation and output-allocation limits fail without a partial successful result. Lazy table caches are separate from the temporary-result budget and currently have no eviction policy.

Some inactive UK map rows reference concepts absent from the concept Snapshot. Their original fields remain available in tuples. A concept-valued projection reaching an absent concept returns `TypeMismatch`; it does not fabricate an ordinal or omit that row. Resolving this case and reviewing scalar/tuple set semantics remain part of full conformance.

## Supplementary refsets

When the base has typed tables, `add-refsets` preserves them and adds active and inactive simple member rows. Shared module-dependency and descriptor tables are merged with schema and duplicate-UUID checks. Unchanged tables are checksum-verified and copied. Concept ordinals can change without rewriting those files because their identifiers are stored as SCTIDs.

The supplementary loader still accepts simple refset packages and their supported metadata, such as PCD. It does not accept arbitrary maps, clinical extension definitions or updates to populated simple refsets. A legacy base without typed tables does not acquire a partial member index from a supplement.

## Release validation

The UK Monolith import retains 7,177,107 rows in 582 member files, totalling 593,059,086 bytes before compression. The numeric, membership, description and display files are unchanged. All files together total 1,110.58 MiB. This is the current uncompressed representation, not the compact-format target.

The import took 110.48 seconds with two CPUs and a 3 GiB container limit. Container-charged peak memory was 1,981,546,496 bytes. This functional run used a Windows bind mount and is not an isolated import-speed comparison.

[Import measurements](../validation/member-import-results.json) record byte counts and the unchanged earlier file hashes. All nine populated RF2 concept-set probes and the 19-row tuple projection match their independent expected results. The two successful association probes also match OneLondon's Ontoserver. Every one of the 1,039 PCD refsets matches its independently scanned inactive-member set, covering 4,524 inactive RF2 rows.

Reproduce the independent fixed probes with:

```sh
python scripts/check_member_fields_rf2.py --archive RF2_ZIP --store INDEX_DIRECTORY --reference ONTOSERVER_REPORT --output data/validation/new-member-check.json
```

The checker scans RF2 separately, compares complete concept sets and preserves duplicate member tuples. Its text reference covers fixed ASCII map-code probes, not general Unicode collation. [Member validation](../validation/member-field-results.json) records matches, query timings and reference failures. OneLondon's Ontoserver returned HTTP 500 for the populated UK map-field probes and HTTP 422 for several date/status probes. Failed requests are not correctness matches or proof that every equivalent expression is unsupported.
