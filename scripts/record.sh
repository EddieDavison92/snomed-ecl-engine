#!/bin/sh
# Records the README's terminal clips from docs/recordings with VHS in Docker.
# Usage: scripts/record.sh BIN_DIR LIBRARY_DIR [TAPE...]
# BIN_DIR holds a Linux snomed-ecl-engine; LIBRARY_DIR holds a UK index.
set -eu
bin=$(cd "$1" && pwd)
library=$(cd "$2" && pwd)
shift 2
[ $# -gt 0 ] || set -- docs/recordings/expand.tape docs/recordings/search.tape docs/recordings/query.tape
for tape in "$@"; do
  docker run --rm -v "$PWD":/repo -w /repo \
    -v "$bin":/opt/ecl/bin:ro -v "$library":/opt/ecl/library \
    -e PATH=/opt/ecl/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -e SNOMED_ECL_HOME=/opt/ecl/library \
    ghcr.io/charmbracelet/vhs:v0.10.0 "$tape"
done
