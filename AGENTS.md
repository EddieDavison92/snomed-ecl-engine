# Working on the ECL engine

- Keep this personal repository private until the owner changes that decision.
- Use plain British English and Conventional Commits. Work on a `feat/`, `fix/` or `docs/` branch.
- Read `docs/plan.md` before implementation. This repository currently contains research and preparation, not an engine.
- Keep RF2 archives, extracted terminology, generated indexes, credentials and reference checkouts out of Git. `data/`, `.local/` and `references/` are ignored.
- Retrieve TRUD credentials at runtime through 1Password. TRUD embeds credentials in URLs: never log or save its raw API response.
- Pin the RF2 archive checksum, edition URI, reference commits and benchmark image digest.
- Use the official ECL grammar and specification as the authority. Other engines are comparison targets, not definitions of correctness.
- Match release, modules, inferred relationship view and active-component rules before comparing results. Compare complete code sets, not counts alone.
- Report unsupported ECL explicitly. Never return a partial answer as a successful expansion.
- Keep the core independent of FHIR, HTTP, cloud services and authoring. Do not add a reasoner to evaluate published inferred RF2 relationships.
- Preserve relationship groups and typed concrete values. Do not flatten groups or compare decimals as binary floating point.
- Build a slow, clear test evaluator before optimising. Use synthetic fixtures in Git and licensed release tests locally.
- Measure import cost separately from query cost. Count all services and operating-system page cache in resource comparisons.
- Reference repositories have their own licences. Do not copy their code without an explicit reuse decision and attribution.
