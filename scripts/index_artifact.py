"""Read benchmark metadata from a directory or a version 2 index container."""
import hashlib
import json
from pathlib import Path


def manifest_bytes(path):
    path = Path(path)
    if path.is_dir():
        return (path / 'manifest.json').read_bytes()
    with path.open('rb') as file:
        header = file.read(56)
        if len(header) != 56 or header[:8] != b'SNECL002':
            raise ValueError('Unsupported container header')
        length = int.from_bytes(header[8:16], 'little')
        size = int.from_bytes(header[16:24], 'little')
        if length != path.stat().st_size or not 0 < size <= min(4 * 1024 * 1024, length - 56):
            raise ValueError('Invalid container size')
        table = file.read(size)
        if hashlib.sha256(table).digest() != header[24:56]:
            raise ValueError('Container table checksum mismatch')
        return json.dumps(json.loads(table)['manifest'], sort_keys=True, separators=(',', ':')).encode()


def read_manifest(path):
    return json.loads(manifest_bytes(path))
