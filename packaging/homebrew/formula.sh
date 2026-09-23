#!/bin/sh
# Writes the Homebrew formula for a release to stdout:
#   packaging/homebrew/formula.sh VERSION SHA256SUMS
set -eu
version=$1
sums=$2
base="https://github.com/EddieDavison92/snomed-ecl-engine/releases/download/v$version"

sha() {
  file="snomed-ecl-engine-v$version-$1-default.tar.gz"
  value=$(awk -v file="$file" '$2 == file { print $1 }' "$sums")
  [ -n "$value" ] || { echo "formula.sh: no checksum for $file" >&2; exit 1; }
  echo "$value"
}

# Looked up before writing, so a missing checksum stops the script.
mac_arm=$(sha aarch64-apple-darwin)
mac_intel=$(sha x86_64-apple-darwin)
linux_arm=$(sha aarch64-unknown-linux-gnu)
linux_intel=$(sha x86_64-unknown-linux-gnu)

cat <<EOF
class SnomedEclEngine < Formula
  desc "Evaluate SNOMED CT ECL against a local index"
  homepage "https://github.com/EddieDavison92/snomed-ecl-engine"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "$base/snomed-ecl-engine-v$version-aarch64-apple-darwin-default.tar.gz"
      sha256 "$mac_arm"
    end
    on_intel do
      url "$base/snomed-ecl-engine-v$version-x86_64-apple-darwin-default.tar.gz"
      sha256 "$mac_intel"
    end
  end

  on_linux do
    on_arm do
      url "$base/snomed-ecl-engine-v$version-aarch64-unknown-linux-gnu-default.tar.gz"
      sha256 "$linux_arm"
    end
    on_intel do
      url "$base/snomed-ecl-engine-v$version-x86_64-unknown-linux-gnu-default.tar.gz"
      sha256 "$linux_intel"
    end
  end

  def install
    bin.install "snomed-ecl-engine"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/snomed-ecl-engine --version")
  end
end
EOF
