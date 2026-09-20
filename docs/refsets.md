# Load and query reference sets

The engine supports `^` / `memberOf` and ECL 2.3 `^R` / `refsetContainingAny` for concept-based refsets. Both compose with hierarchy, Boolean sets and refinements. Member filters and typed field projections remain required work.

```sh
snomed-rust-ecl-engine expand STORE '^723562003' --count
snomed-rust-ecl-engine expand STORE '^R195967001' --display
snomed-rust-ecl-engine expand STORE '(<<195967001) AND (^*)' --count
```

`membership.bin` contains sparse refset keys and sorted, deduplicated concept ordinals. Only active RF2 member rows contribute. Description-referencing language rows are not converted into their owning concepts. The file has its own version, bounds checks and checksum. Old stores still open, but membership queries fail explicitly if their manifest has no membership index. Reimport the base Snapshot to build it.

The current numeric loader opens membership with the core. Preferred display text remains separate. Typed member fields, all descriptions, dialect memberships and historical associations need further semantic indexes. These are part of full ECL's eventual storage budget.

## Concept status defaults

ECL includes all concepts by default, with active relationships and active refset members. An active membership may refer to an inactive concept. The earlier prototype incorrectly excluded inactive concepts from literals, wildcard and membership. This milestone corrects that default throughout set evaluation. Active inferred hierarchy edges remain unchanged.

The current [simple-expression specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/behaviour-specification-with-examples/6.1-simple-expression-constraints) explicitly permits inactive concepts in membership results. The [quick reference](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/appendices/appendix-d-ecl-quick-reference) defines the default concept population. Older syntax prose still describes an active-component default; use the current behaviour rules when checking this distinction.

The release-matched query `^51971000001109` returns 102 concepts, including five inactive concepts. Both the independent RF2 scan and OneLondon agree. Our earlier result of 97 was wrong. Old benchmark artefacts are retained as historical measurements, with the correction recorded in [basic ECL](basic-ecl.md).

## Add PCD or another simple refset supplement

```sh
snomed-rust-ecl-engine add-refsets BASE_STORE ARCHIVE NEW_STORE YYYYMMDD SHA256
```

The library equivalent is `import::add_refsets_snapshot`. Import support is optional at build time; query-only binaries can open the completed index.

The command reads the existing numeric and display indexes plus a checksum-verified supplementary RF2 Snapshot ZIP. It imports simple concept refsets and any new defining concepts, active inferred ungrouped is-a relationships and active English FSNs supplied with them. New SCTIDs are merged into the sorted dictionary and existing ordinal references are remapped. Existing attribute values, groups, concrete values and displays are preserved. The original store is unchanged. No base RF2 reread or classification is needed.

The destination must be new. Unknown referenced concepts, duplicate Snapshot member IDs, future-dated rows, existing concept definitions and populated refset collisions fail before publication. A refset already introduced by a previous supplement also fails, even if empty. To update or replace a supplement, run against the original base and publish a new combined store. This avoids retaining memberships withdrawn in the newer Snapshot. Additive extension of a populated refset and Delta updates are not implemented.

This command imports simple refset content. It does not import supplemental maps, language memberships, OWL axioms, typed member fields or arbitrary clinical extension definitions. Additional relationship types and active concrete definition rows are rejected. Published inferred is-a rows supply the new definition hierarchy; no reasoner runs.

The base edition URI and base archive checksum remain in the manifest. Each supplement records its own checksum, date, refset IDs, added concept count, input core checksum and declared module dependencies. Batch responses include supplement checksums alongside the base edition. Compare that complete identity when checking results against another server.

Dependency checks currently establish that referenced modules exist and required dates do not exceed the base edition date. They do not prove exact module-version composition or semantic compatibility with a newer dependency. `exact_module_versions_verified` is false. A complete module dependency resolver remains required; do not describe a combined store as an unmodified official edition.

## Validated PCD release

TRUD item 659 supplied PCD release 63.0.0, dated 17 July 2026 and published 20 July 2026. The archive is `uk_sct2pc_63.0.0_20260717000000Z.zip`, 12,457,701 bytes, SHA-256 `672f4ee9ffa6f385a3620129b968d42031bd68bc157c47a93565392dd635db60`. It was the latest item release returned on 20 September 2026. Credentials and downloads remain local.

The package has 1,060 defining concepts absent from the UK Monolith, including 914 active concepts. Its simple Snapshot has 129,004 rows, 124,480 active memberships and 1,039 refsets. All referenced clinical concepts exist in the pinned monolith. The combined index adds 914 hierarchy edges. New definition displays use their English FSNs.

| File | Monolith with membership | With PCD | Added bytes |
|---|---:|---:|---:|
| Core | 90,263,179 | 90,305,471 | 42,292 |
| Membership | 18,014,172 | 18,520,404 | 506,232 |
| Displays | 68,953,229 | 69,169,187 | 215,958 |

PCD adds 764,482 bytes across these files, about 0.73 MiB. Its observed import took 9.85 seconds in a two-CPU, 2 GiB container using a Windows bind mount. This was a functional validation run alongside an RF2 audit, not an isolated import benchmark.

## Evidence and reproduction

All 45 [OneLondon probes](../validation/ontoserver-membership.json) match complete code-set digests at the pinned edition. Six independent monolith RF2 scans cover direct and reverse membership. Every one of the 1,039 PCD refsets matches its full independent RF2 set, including inactive referenced concepts. Synthetic tests cover composition, absent indexes, limits, corruption, collisions and supplementary ordinal remapping.

```sh
python scripts/check_membership_rf2.py --archive BASE_RF2 --store BASE_STORE --output data/validation/membership-check.json
python scripts/check_supplement_rf2.py --archive PCD_RF2 --store COMBINED_STORE --output data/validation/pcd-check.json
python scripts/benchmark_corpus.py --store-directory BASE_STORE --output data/validation/new-corpus-run.json
```

The 1,000-expression corpus now evaluates 840 cases and rejects 160 unsupported cases. All 800 previously evaluated code-set digests are unchanged. Its 40 membership cases match OneLondon. The pinned official examples parse 73 of 121 files, with 48 unsupported and no unexpected syntax errors. These are development coverage counts, not a language-conformance percentage. [Validation summary](../validation/membership-validation.json) records the measurements; [full conformance](conformance.md) records what remains.
