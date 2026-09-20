# Developer setup

For using the built CLI, read the [CLI guide](cli.md). This page covers building
the engine and reproducing its measurements.

## Build

```sh
cargo build --locked --release --bin snomed-ecl-engine
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Use the toolchain pinned in `rust-toolchain.toml`; rustup selects it
automatically. A native Windows build needs the MSVC C++ build tools and the
Windows SDK.

`--no-default-features` omits the RF2 ZIP importer, giving a query-only
executable that can still read, pack and verify existing indexes.

### Build with term matching

Description term predicates need `--features unicode`, which links ICU4C. On
Debian or Ubuntu:

```sh
apt-get install -y libicu-dev pkg-config
cargo build --locked --release --features unicode
```

A build without the feature rejects term predicates explicitly rather than
ignoring them. Metadata-only description filters work either way. The feature
adds materially to the executable; [benchmarks](benchmarks.md) records both sizes.

### Build in Docker

Building on Windows without the MSVC toolchain, or reproducing the Linux
measurements, is easiest in a container. Run this from PowerShell with Windows
paths: Git Bash rewrites `/work` and the mount fails.

```powershell
docker run --rm `
  -v ${PWD}:/work `
  -v snomed-rust-cargo:/usr/local/cargo/registry `
  -v snomed-rust-rustup:/usr/local/rustup `
  -e CARGO_TARGET_DIR=/work/target/linux `
  -w /work rust:1.93.1-bookworm `
  cargo build --locked --release --bin snomed-ecl-engine
```

Give each concurrent build its own `CARGO_TARGET_DIR`; a git worktree must not
share one with the main checkout.

## Get an RF2 release

Use an archive you are entitled to. Nothing licensed is in this repository, and
`data/`, `.local/` and `references/` are ignored.

With the 1Password CLI unlocked, `scripts/Get-Rf2Release.ps1` retrieves the UK
Monolith from TRUD, verifies size and SHA-256 and writes a local manifest:

```powershell
./scripts/Get-Rf2Release.ps1                                        # latest
./scripts/Get-Rf2Release.ps1 -ReleaseId uk_sct2mo_42.5.0_20260826000001Z.zip
```

The script reads `op://Work/digital.nhs.uk/api-key` without printing it. TRUD
embeds credentials in download URLs: never log or save a raw response.

Without that setup, download the archive yourself and check it with
`snomed-ecl-engine inspect ARCHIVE.zip`, comparing the checksum against the
value the distributor published. The pinned release for reproducing published
figures is in [release.json](release.json).

## Reference checkouts

```powershell
foreach ($reference in (Get-Content docs/references.json -Raw | ConvertFrom-Json)) {
    git clone $reference.remote ('references/' + $reference.name)
    git -C ('references/' + $reference.name) checkout --detach $reference.commit
}
```

These are comparison targets and specification sources, pinned by commit. They
have their own licences; do not copy their code without an explicit decision and
attribution.

## Comparison servers

The benchmark harness drives Snowstorm and Snowstorm Lite over loopback only.
Both keep their imported index in a Docker volume, so a finished import can be
restarted for serving without importing again:

```powershell
docker start snomed-ecl-elasticsearch   # wait for cluster health
docker start snomed-ecl-snowstorm       # wait for /branches/MAIN
docker start snomed-ecl-serving         # Snowstorm Lite
```

Never restart a preparation container that still has `--load` in its command: it
will import a second time. `scripts/benchmark_corpus.py` refuses to compare
unless a completed MAIN snapshot import is evidenced, the advertised edition
matches the index, and two release sentinels agree. An accidental reimport or a
mismatched release stops the run instead of producing numbers.

Do not benchmark OneLondon's shared Ontoserver. Its latency and shared load make
it unsuitable, and repeated load adds no correctness evidence. Small pinned
correctness probes are fine: `scripts/Test-Ontoserver.ps1` refreshes them and
writes to `data/validation`.
