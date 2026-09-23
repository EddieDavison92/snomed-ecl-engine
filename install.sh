#!/bin/sh
# Installs snomed-ecl-engine from a GitHub release on Linux or macOS:
#
#   curl -fsSL https://raw.githubusercontent.com/EddieDavison92/snomed-ecl-engine/main/install.sh | sh
#
# Environment:
#   SNOMED_ECL_VERSION      release to install, such as 0.1.1 (default: latest)
#   SNOMED_ECL_BUILD        default, query or unicode (unicode is Linux only)
#   SNOMED_ECL_INSTALL_DIR  where to put the executable (default: ~/.local/bin)
set -eu

repo=EddieDavison92/snomed-ecl-engine
build=${SNOMED_ECL_BUILD:-default}
dir=${SNOMED_ECL_INSTALL_DIR:-$HOME/.local/bin}

fail() {
  echo "install.sh: $*" >&2
  exit 1
}

case "$(uname -s)" in
  Linux) os=unknown-linux-gnu ;;
  Darwin) os=apple-darwin ;;
  *) fail "no build for $(uname -s); on Windows, use install.ps1" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  arm64 | aarch64) arch=aarch64 ;;
  *) fail "no build for $(uname -m)" ;;
esac
target=$arch-$os

if [ -n "${SNOMED_ECL_VERSION:-}" ]; then
  tag=v${SNOMED_ECL_VERSION#v}
else
  tag=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$tag" ] || fail "could not find the latest release"
fi

name=snomed-ecl-engine-$tag-$target-$build
url=https://github.com/$repo/releases/download/$tag
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

curl -fsSL "$url/SHA256SUMS" -o "$tmp/SHA256SUMS" || fail "could not download $tag checksums"
expected=$(awk -v file="$name.tar.gz" '$2 == file { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "$tag has no $build build for $target"
curl -fsSL "$url/$name.tar.gz" -o "$tmp/$name.tar.gz" || fail "could not download $name"

if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$name.tar.gz" | cut -d ' ' -f 1)
else
  actual=$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d ' ' -f 1)
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for $name.tar.gz"

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dir"
cp "$tmp/$name/snomed-ecl-engine" "$dir/snomed-ecl-engine"
chmod 755 "$dir/snomed-ecl-engine"
echo "Installed $("$dir/snomed-ecl-engine" --version) to $dir"

case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "Add $dir to your PATH to run snomed-ecl-engine by name." ;;
esac
