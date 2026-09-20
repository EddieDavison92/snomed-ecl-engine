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

Supported field types are concept IDs, exact numbers, strings, Booleans, dates and member UUIDs. Metadata includes `active`, `moduleId`, `effectiveTime`, `refsetId` and `referencedComponentId`. String predicates require the `unicode` Cargo feature. Member strings use English collation by default; `member_language` in the [query configuration](aliases.md) selects another language.

The importer reads field types from RF2 reference set descriptors, including inherited descriptors and datatype subtypes. It preserves exact decimals, custom dates and UUID fields. Custom header names have whitespace removed for use in ECL. Without a descriptor, `c`, `i` and `s` filename representations supply component, integer and string types; standard module-dependency dates and the MRCM `grouped` Boolean retain their known meaning. Conflicting inherited descriptors and duplicate active field positions fail import.

A quoted value such as `"20260826"` can be a date or an untyped text prefix. The stored column type resolves this ambiguity. Explicit `match:` and `wild:` predicates require a string column. Decimal columns use member-file tag 6; earlier readers reject this new column type and must be upgraded.

The grammar also permits member-filter forms without an explicit refset operator. Their evaluation remains unsupported. This milestone handles member filters attached to `^` or `^R`.

The [member-filter specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.10-member-filters) defines predicate behaviour. The [simple-expression specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.1-simple-expression-constraints) defines field projection and tuple restrictions.

## Library and CLI results

`eval::evaluate_result` and `eval::evaluate_result_with_limits` return `QueryResult`:

- `Concepts(Vec<u32>)` contains sorted unique concept ordinals, including single concept-field projections.
- `Values(Vec<MemberValue>)` contains distinct typed scalar values from single non-concept field projections and concrete dotted projections.
- `Rows(Vec<BTreeMap<String, MemberValue>>)` contains typed fields for terminal multi-field projections. Each matching member produces a row.

`MemberValue` distinguishes `concept`, `number`, `boolean`, `time` and `string`. JSON encodes concept IDs and exact numbers as strings. Scalar sets support `AND`, `OR` and `MINUS`. Numbers use a canonical exact decimal spelling, so `1.00` and `1` are one value. Strings preserve case and text. Multi-field rows preserve original field spellings and duplicate rows.

The existing `evaluate` functions accept concept-valued results only. A tuple in a subquery returns `TypeMismatch`, even when no member could match. Hierarchy operators and successive dot steps require concept inputs. A dot step selecting both concrete and concept attributes returns a `Values` set with both tagged types. Combining a scalar result with a concept-only result currently returns `TypeMismatch`; further cross-type conformance checks remain required.

```text
^[referencedComponentId,mapTarget,mapGroup]999002271000000101 {{M mapGroup=#1,mapPriority=#1,mapTarget=wild:"J459"}}
```

`expand` prints typed scalars or rows as JSONL. `--count` counts distinct scalar values or rows; `--display` requires concept results. Batch responses use `result_type: "values"` with `values`, or `result_type: "rows"` with `rows`. `count_only` omits the array. Concept responses retain their existing schema.

`[*]` selects all non-metadata fields, including `referencedComponentId`. Unknown field names return `InvalidField`. Invalid field/value combinations return `TypeMismatch`. Work, cancellation and output-allocation limits fail without a partial successful result. Lazy table caches are separate from the temporary-result budget and currently have no eviction policy.

Some inactive UK map rows reference concepts absent from the concept Snapshot. Their original fields remain available in tuples. A concept-valued projection reaching an absent concept returns `TypeMismatch`; it does not fabricate an ordinal or omit that row. Resolving this case and reviewing scalar/tuple set semantics remain part of full conformance.

## Supplementary refsets

When the base has typed tables, `add-refsets` preserves them and adds active and inactive simple member rows. Shared module-dependency and descriptor tables are merged with schema and duplicate-UUID checks. Unchanged tables are checksum-verified and copied. Concept ordinals can change without rewriting those files because their identifiers are stored as SCTIDs.

The supplementary loader still accepts simple refset packages and their supported metadata, such as PCD. It does not accept arbitrary maps, clinical extension definitions or updates to populated simple refsets. A legacy base without typed tables does not acquire a partial member index from a supplement.

## Release validation

The [descriptor/scalar evaluation round](../validation/schema-results.json) rebuilt the UK index and added PCD successfully. Core, membership, descriptions, displays and all 582 member files match the earlier checksums. The UK Identifier Snapshot has no data rows; the new empty identifier index is 11 bytes. Both published Identifier column orders have synthetic regression tests.

Nine scalar projection and set-operation checks match independent RF2 sets, including exact concrete numbers and map targets. The earlier member and history probes still match. All 1,039 PCD refsets match both their active and inactive RF2 membership sets. The [1,000-expression run](../validation/schema-corpus-results.json) preserved every earlier complete result set, with a 1.63 ms median request and 10.60 ms p95. The Windows bind-mount container start plus first request took 43.7 seconds in this run; it is not a serverless cold-start measurement.

The UK Monolith import retains 7,177,107 rows in 582 member files, totalling 593,059,086 bytes before compression. The numeric, membership, description and display files are unchanged. All files together total 1,110.58 MiB. This is the current uncompressed representation, not the compact-format target.

The import took 110.48 seconds with two CPUs and a 3 GiB container limit. Container-charged peak memory was 1,981,546,496 bytes. This functional run used a Windows bind mount and is not an isolated import-speed comparison.

[Import measurements](../validation/member-import-results.json) record byte counts and the unchanged earlier file hashes. All nine populated RF2 concept-set probes and the 19-row tuple projection match their independent expected results. The two successful association probes also match OneLondon's Ontoserver. Every one of the 1,039 PCD refsets matches its independently scanned inactive-member set, covering 4,524 inactive RF2 rows.

Reproduce the independent fixed probes with:

```sh
python scripts/check_member_fields_rf2.py --archive RF2_ZIP --store INDEX_DIRECTORY --reference ONTOSERVER_REPORT --output data/validation/new-member-check.json
```

The checker scans RF2 separately, compares complete concept sets and preserves duplicate member tuples. Its text reference covers fixed ASCII map-code probes, not general Unicode collation. [Member validation](../validation/member-field-results.json) records matches, query timings and reference failures. OneLondon's Ontoserver returned HTTP 500 for the populated UK map-field probes and HTTP 422 for several date/status probes. Failed requests are not correctness matches or proof that every equivalent expression is unsupported.
