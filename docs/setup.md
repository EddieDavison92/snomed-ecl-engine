# Developer setup

Building from source and reproducing the measurements. For using the CLI, read
the [CLI guide](cli.md); for the checks a change must pass, read
[CONTRIBUTING](../CONTRIBUTING.md).

## Build

```sh
cargo build --locked --release --bin snomed-ecl-engine
```

Rust 1.93 or later; rustup selects the version pinned in `rust-toolchain.toml`.
A native Windows build needs the MSVC C++ build tools and the Windows SDK.

`--no-default-features` leaves out the RF2 importer, giving a query-only
executable that can still read, pack and verify existing indexes.

### Build with term matching

Description term predicates need `--features unicode`, which links ICU 72 or
later statically. On Debian or Ubuntu:

```sh
apt-get install -y libicu-dev pkg-config
cargo build --locked --release --features unicode
```

A build without the feature rejects term predicates explicitly rather than
ignoring them. Metadata-only description filters work either way. The feature
adds about 31 MiB to the executable.

### Build in Docker

The published executables and every benchmark use the `rust:1.93.1-bookworm`
image. To build the same way:

```sh
docker run --rm \
  -v "$PWD":/work \
  -v snomed-rust-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/work/target/linux-core \
  -w /work rust:1.93.1-bookworm \
  cargo build --locked --release --bin snomed-ecl-engine
```

The benchmark scripts expect the default build in `target/linux-core` and the
term-matching build in `target/linux-unicode`. For the second, install ICU in
the container and set the target directory to match:

```sh
docker run --rm \
  -v "$PWD":/work \
  -v snomed-rust-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/work/target/linux-unicode \
  -w /work rust:1.93.1-bookworm \
  sh -c 'apt-get update -qq && apt-get install -y -qq libicu-dev pkg-config &&
         cargo build --locked --release --features unicode --bin snomed-ecl-engine'
```

On Windows, run Docker from PowerShell with `${PWD}`: Git Bash rewrites `/work`
and the mount fails.

## Get an RF2 release

Use a release you are licensed to use; see the [README](../README.md#licence).
The importer takes one self-contained Snapshot ZIP, and the UK Monolith Edition
is the tested input. Keep archives under `data/`, which Git ignores.

In the UK, register with NHS England's [TRUD](https://isd.digital.nhs.uk/trud/)
and subscribe to the SNOMED CT UK Monolith Edition, RF2: Snapshot. With
`TRUD_API_KEY` set, `snomed-ecl-engine download --keep-archive` fetches and
verifies the newest release, builds an index, and keeps the ZIP in the library's
`downloads` folder. To download it yourself, check it before importing:

```sh
snomed-ecl-engine inspect data/rf2/ARCHIVE.zip
```

Compare the SHA-256 it prints with the value on TRUD's download page. The
release behind the published figures is pinned in [release.json](release.json).

`scripts/Get-Rf2Release.ps1` downloads from TRUD's API, checks size and SHA-256
and writes a manifest beside the archive. Set `TRUD_API_KEY` first, or pass
`-ApiKey`. TRUD puts the key in its download URLs, so never log or save a raw
API response.

## Reference checkouts

The grammar and comparison tooling read pinned checkouts of other projects from
`references/`, which Git ignores. [references.json](references.json) lists them:

```sh
jq -r '.[] | "\(.name) \(.remote) \(.commit)"' docs/references.json |
while read -r name remote commit; do
  git clone "$remote" "references/$name"
  git -C "references/$name" checkout --detach "$commit"
done
```

They are comparison targets and specification sources. They have their own
licences; do not copy their code without checking the licence and recording
attribution.

## Comparison servers

The benchmark harness drives Snowstorm and Snowstorm Lite over loopback only:
Snowstorm on port 18082 and Snowstorm Lite on 18081. The published runs used
these images, recorded in the evidence files:

| Server | Image | Allocation |
|---|---|---|
| Snowstorm 11.0.0 | `snomedinternational/snowstorm@sha256:fa9cce11…` | 4 CPUs, 6 GiB |
| Elasticsearch, for Snowstorm | `docker.elastic.co/elasticsearch/elasticsearch@sha256:1b6a877f…` | 4 CPUs, 6 GiB |
| Snowstorm Lite 2.7.0 | `snomedinternational/snowstorm-lite@sha256:ff167ec2…` | 1 CPU, 2 GiB |

Load each with the same RF2 release as the index, following the servers' own
documentation. Keep their data in Docker volumes so a finished import can be
restarted for serving without importing again, and never restart a container
whose command still loads the release: it will import a second time.

`scripts/benchmark_corpus.py` refuses to compare unless a finished MAIN snapshot
import is evidenced, the advertised edition matches the index, and two release
sentinels agree. An accidental reimport or a mismatched release stops the run
instead of producing numbers.

## Terminal clips

The README's GIFs are recorded from the tapes in
[docs/recordings](recordings) with [VHS](https://github.com/charmbracelet/vhs)
in Docker. Give the script a folder holding a Linux executable and a library
folder holding a UK index:

```sh
scripts/record.sh ~/ecl/bin ~/ecl/library
```

It writes `docs/images/expand.gif`, `search.gif` and `query.gif`. The image is
pinned to VHS 0.10.0: 0.12.0 captured no frames under Docker Desktop. Run it
from a Linux filesystem, since Docker Desktop did not write the output through a
WSL `/mnt/c` mount.
