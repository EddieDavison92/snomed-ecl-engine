#!/bin/sh
# Records the README's terminal clips from docs/recordings with VHS in Docker.
# Usage: scripts/record.sh BIN_DIR LIBRARY_DIR [TAPE...]
# BIN_DIR holds a Linux snomed-ecl-engine; LIBRARY_DIR holds a UK index.
# setup.tape downloads a release, so it runs only when named, and needs
# TRUD_API_KEY: the key goes into a temporary, ignored copy of the tape and is
# typed into the hidden login prompt. VHS echoes each typed command, so that
# tape's log is shown with the key removed. Both files are deleted afterwards.
set -eu
bin=$(cd "$1" && pwd)
library=$(cd "$2" && pwd)
shift 2
[ $# -gt 0 ] || set -- docs/recordings/expand.tape docs/recordings/search.tape docs/recordings/query.tape
docker build -q -t snomed-ecl-vhs docs/recordings >/dev/null
secret=
trap '[ -z "$secret" ] || rm -f "$secret" "$secret.log"' EXIT INT TERM
# Prints stdin with every occurrence of the key replaced, reading the key from
# the environment so it never appears in arguments.
redact() {
  awk 'BEGIN { key = ENVIRON["TRUD_API_KEY"] }
       { while (key != "" && (i = index($0, key))) $0 = substr($0, 1, i - 1) "***" substr($0, i + length(key))
         print }'
}
record() {
  docker run --rm -v "$PWD":/repo -w /repo \
    -v "$bin":/opt/ecl/bin:ro -v "$library":/opt/ecl/library \
    -e PATH=/opt/ecl/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    -e SNOMED_ECL_HOME=/opt/ecl/library \
    snomed-ecl-vhs "$1"
}
for tape in "$@"; do
  if ! grep -q @TRUD_KEY@ "$tape"; then
    record "$tape"
    continue
  fi
  : "${TRUD_API_KEY:?$tape needs TRUD_API_KEY}"
  secret=$(mktemp --suffix=.tape docs/recordings/.key-XXXXXX)
  awk '{ i = index($0, "@TRUD_KEY@")
         if (i) $0 = substr($0, 1, i - 1) ENVIRON["TRUD_API_KEY"] substr($0, i + 10)
         print }' "$tape" > "$secret"
  status=0
  record "$secret" > "$secret.log" 2>&1 || status=$?
  redact < "$secret.log"
  rm -f "$secret" "$secret.log"
  secret=
  [ "$status" -eq 0 ] || exit "$status"
done
