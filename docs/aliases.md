# Identifier schemes and dialect aliases

`expand` and `batch` accept `--config FILE`. The file binds ECL alias names to SCTIDs, encoded as decimal strings:

```json
{
  "identifier_schemes": {"example": "100001"},
  "member_language": "en",
  "dialects": {"en-gb": "900000000000508004", "local": "100002"}
}
```

The two small SCTIDs above are synthetic examples. Use the identifier scheme and language refset concepts supplied with your edition. Alias names are case-insensitive; identifier codes are case-sensitive. A supplied `dialects` map replaces the defaults. Omitting it retains the aliases listed in [Appendix C of the ECL specification](https://docs.snomed.org/snomed-ct-specifications/snomed-ct-expression-constraint-language/appendices/appendix-c-dialect-aliases).

`member_language` selects collation for member string predicates. It defaults to `en` and accepts a two-letter language code, such as `sv`. RF2 member rows have no language field. Description predicates continue to use each description's own language.

```sh
snomed-ecl-engine expand STORE 'example#code' --config aliases.json
snomed-ecl-engine expand STORE '* {{ D dialect=local (preferred) }}' --config aliases.json
snomed-ecl-engine batch STORE --config aliases.json < queries.jsonl
```

Alternate identifiers require an index built with identifier support. Import reads RF2 Identifier Snapshot files, including inactive associations, and writes `identifiers.json`. No identifier file in the release produces an empty index. Older stores without this index must be rebuilt before identifier queries can run.

Only active identifier associations resolve concepts. An association to a description or relationship does not resolve its owning concept. An unknown code returns an empty set. An unknown scheme alias is an error. Concept activity does not alter resolution: an active identifier association can identify an inactive concept.

Both `"example#code with spaces"` from the normative grammar and `example#"code with spaces"` from the official examples are accepted. The code must not contain an escaped character.

The library exposes `QueryConfig` through `NumericStore.config`. Call `normalise()` after building a configuration programmatically. Batch responses contain `query_config_sha256`, the SHA-256 of its canonical JSON representation. Record this alongside the edition and supplement checksums when comparing results.

Identifier data loads on first use and passes checksum, row count and key validation. Refset supplements preserve the base identifiers and can add distinct `(scheme, code)` keys. Replacing a key requires a fresh import.
