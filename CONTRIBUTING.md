# Contributing

Changes reach `main` through pull requests. CI must pass before merging.

## Build and check

Rust 1.93 or later; `rust-toolchain.toml` pins the exact version and rustup
selects it.

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo test --locked --no-default-features
cargo test --locked --all-features
```

`--all-features` includes term matching, which links ICU 72 or later. On Debian
or Ubuntu, install `libicu-dev` and `pkg-config` first. [Developer
setup](docs/setup.md) covers Docker builds and reproducing the benchmarks.

Tests use synthetic fixtures, so none of them needs a SNOMED CT release.

## Rules the code follows

- **The specification decides.** The official ECL grammar and specification
  define correct behaviour. Other engines are comparison targets, not
  authorities.
- **No partial answers.** Unsupported ECL fails with an explicit error. Never
  return a partial result as a successful expansion.
- **Compare like with like.** Before comparing results with another engine,
  match the release, modules, inferred relationship view and active-component
  rules, then compare whole code sets, not counts.
- **Keep values exact.** Preserve relationship groups and typed concrete
  values. Do not flatten groups or compare decimals as binary floating point.
- **Correct first, then fast.** Write a slow, clear evaluator before optimising,
  and keep an optimisation only when measurements show it does not cost query
  latency.
- **Measure separately.** Report import cost separately from query cost, and
  index acquisition, process start and memory separately from warm queries.
  Count every service and the operating system's page cache in resource
  comparisons.
- **Stay a library.** The core does not depend on FHIR, HTTP, cloud services
  or authoring tools, and does not use a reasoner: it evaluates the published
  inferred relationships. Servers, authentication and hosting belong in the
  applications that use it.

## Releasing

Bump `version` in `Cargo.toml`, merge, then push a matching tag such as
`v0.1.1`. The release workflow builds every platform and publishes to GitHub
Releases, crates.io, npm, the Homebrew tap and ghcr.io. crates.io and npm
trust the workflow through OIDC trusted publishing, so they need no stored
token. The tap needs `HOMEBREW_TAP_TOKEN`, with write access to
`EddieDavison92/homebrew-tap`, and is skipped without it. A pull request that
changes packaging runs the same builds without publishing.

## Data and licensing

SNOMED CT is licensed separately; see the [README](README.md#licence). Keep RF2
archives, extracted files and built indexes out of Git: `data/` and
`references/` are ignored. Test fixtures must be synthetic.

Reference checkouts of other projects are for comparison only. Do not copy
their code without checking its licence and recording attribution.

## Commits and pull requests

Use [Conventional Commits](https://www.conventionalcommits.org/) and plain
British English. Keep each pull request to one logical change, and state how it
was tested. Changes to evaluation should say which corpus or RF2 check they
were verified against.

By contributing, you agree that your contribution is licensed under the
project's [MIT licence](LICENSE).
