"""Measure container packing with pinned binaries, component hashes and charged memory."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess

from common import ROOT, IMAGE
from index_artifact import read_manifest


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--store', required=True, type=Path)
    parser.add_argument('--destination', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--block-kib', type=int, default=64)
    parser.add_argument('--uncompressed', action='store_true')
    parser.add_argument('--binary', type=Path, default=Path('target/linux-unicode/release/snomed-ecl-engine'))
    args = parser.parse_args()
    if args.destination.exists() or args.output.exists():
        parser.error('Choose new destination and output paths')
    paths = [(ROOT / p).resolve().relative_to(ROOT).as_posix() for p in [args.binary, args.store, args.destination]]
    binary, store, destination = paths
    binary_sha256 = digest(ROOT / binary)
    manifest = read_manifest(ROOT / store)
    options = ['--uncompressed'] if args.uncompressed else ['--block-kib', str(args.block_kib)]
    # The final cgroup read happens before container exit, after the measured process.
    wrapper = '"$@"; status=$?; cat /sys/fs/cgroup/memory/memory.max_usage_in_bytes >&2; exit "$status"'
    run = subprocess.run(['docker', 'run', '--rm', '--cpus=1', '--memory=1g', '--memory-swap=1g',
        '--mount', f'type=bind,source={ROOT},target=/work', '-w', '/work', IMAGE,
        'sh', '-c', wrapper, 'pack-measure', binary, 'pack', store, destination, *options],
        text=True, encoding='utf-8', capture_output=True, timeout=1800)
    if run.returncode:
        raise RuntimeError(run.stderr)
    measurement = json.loads(run.stdout)
    peak = int(run.stderr.strip().splitlines()[-1])
    assert read_manifest(ROOT / destination) == manifest
    assert digest(ROOT / binary) == binary_sha256, 'Binary changed during packing'
    report = dict(recorded_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        edition=manifest['edition'], archive_sha256=manifest['archive_sha256'],
        binary_sha256=binary_sha256, image=IMAGE, cpus=1, memory_limit_mib=1024,
        compression='none' if args.uncompressed else 'zstd-3', block_kib=None if args.uncompressed else args.block_kib,
        container_sha256=digest(ROOT / destination), container_bytes=(ROOT / destination).stat().st_size,
        container_charged_peak_bytes=peak, manifest_preserved=True, **measurement,
        scope='Packing an existing index, not RF2 import. Includes source checksum checks, block encoding, temporary spool, container writing and final decoded checksum verification. Filesystem cache is charged; caches are not dropped. Semantic structure is checked separately by verify.')
    args.output.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
