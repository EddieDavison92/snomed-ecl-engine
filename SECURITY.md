# Security

The engine parses untrusted ECL and reads index files that may have been
altered. A crash, hang or excessive allocation from either is a security issue,
as is a wrong answer caused by a damaged index that `verify` does not catch.

Report issues privately through GitHub's
[private vulnerability reporting](https://github.com/EddieDavison92/snomed-ecl-engine/security/advisories/new),
not in a public issue. Include the ECL or a description of the index damage,
the version or commit, and what happened.

Only the latest release is supported.
