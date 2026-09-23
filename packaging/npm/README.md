# snomed-ecl-engine

Evaluate SNOMED CT Expression Constraint Language against a local index, without
running a terminology server.

```sh
npx snomed-ecl-engine --help
npm install --global snomed-ecl-engine
```

This package installs the prebuilt executable for Linux (x64 and arm64, glibc),
macOS (x64 and arm64) or Windows (x64). It includes the RF2 importer but not
term matching in description filters; for that, use the `unicode` build from
the [releases](https://github.com/EddieDavison92/snomed-ecl-engine/releases).

You need a SNOMED CT release you are licensed to use. See the
[project README](https://github.com/EddieDavison92/snomed-ecl-engine) for the
quick start, the CLI guide and what ECL it supports.

MIT licensed. SNOMED CT is not included and is licensed separately.
