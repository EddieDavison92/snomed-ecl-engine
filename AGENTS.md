# Working on the ECL engine

- Keep this personal repository private until the owner changes that decision.
- Use plain British English and Conventional Commits. During early development, work directly on `main` and push completed, checked changes. Create a separate branch only when the owner requests it or concurrent work needs isolation.
- Read `docs/plan.md` before implementation. The prototype evaluates basic ECL and refinements. Track remaining semantics and extended ECL in `docs/ecl-support.md` and `validation/ecl-conformance.json`.
- Keep RF2 archives, extracted terminology, generated indexes, credentials and reference checkouts out of Git. `data/`, `.local/` and `references/` are ignored.
- Retrieve TRUD credentials at runtime through 1Password. TRUD embeds credentials in URLs: never log or save its raw API response.
- Pin the RF2 archive checksum, edition URI, reference commits and benchmark image digest.
- Use the official ECL grammar and specification as the authority. Other engines are comparison targets, not definitions of correctness.
- Match release, modules, inferred relationship view and active-component rules before comparing results. Compare complete code sets, not counts alone.
- Report unsupported ECL explicitly. Never return a partial answer as a successful expansion.
- Full ECL 2.3 syntax and semantics are mandatory for completion. Unsupported-feature errors are temporary development behaviour, not an acceptable final substitute. Measure final resource use with all required semantic indexes included.
- Keep the core independent of FHIR, HTTP, cloud services and authoring. Do not add a reasoner to evaluate published inferred RF2 relationships.
- This repository owns the engine library, index format, offline importer, CLI, conformance tests and benchmarks. The deployment wrapper must live in a separate repository that consumes this library. Keep API handlers, authentication, cloud SDKs, index distribution and deployment configuration there. Vercel is the preferred host for that separate application.
- Keep offline import dependencies optional. Serverless execution must not require always-on compute. Measure the complete engine's package, index acquisition, process startup and memory separately from warm queries.
- Preserve relationship groups and typed concrete values. Do not flatten groups or compare decimals as binary floating point.
- Build a slow, clear test evaluator before optimising. Use synthetic fixtures in Git and licensed release tests locally.
- Measure import cost separately from query cost. Count all services and operating-system page cache in resource comparisons.
- Reference repositories have their own licences. Do not copy their code without an explicit reuse decision and attribution.
