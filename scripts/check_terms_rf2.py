"""Check the fixed English term probes by scanning RF2 independently of the Rust index."""
import argparse
from collections import defaultdict
import gzip
import hashlib
import json
from pathlib import Path
import re
import statistics
import subprocess
import unicodedata
import zipfile

from benchmark_ecl import ROOT, IMAGE, digest
from index_artifact import read_manifest
from check_membership_rf2 import rows


def normalise(term):
    # This reference covers the fixed unaccented English probes, not general UCA matching.
    return ''.join(c for c in unicodedata.normalize('NFD', term.casefold())
                   if not unicodedata.combining(c))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--store', required=True, type=Path)
    parser.add_argument('--reference', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--binary', default='target/linux-unicode/release/snomed-ecl-engine')
    parser.add_argument('--memory-mib', type=int, default=1024)
    args = parser.parse_args()
    if args.output.exists() or args.memory_mib < 1:
        parser.error('Choose a new report path and a positive memory limit')
    manifest = read_manifest(args.store)
    with args.archive.open('rb') as file:
        assert hashlib.file_digest(file, 'sha256').hexdigest() == manifest['archive_sha256']
    queries = json.loads((ROOT / 'validation/term-queries.json').read_text())
    predicates = [
        lambda s: re.search(r'\bgas', s) is not None or s.endswith('itis'),
        lambda s: re.search(r'\basth', s) is not None,
        lambda s: 'asthma' in s,
        lambda s: re.search(r'\ballergic', s) is not None and re.search(r'\basthma', s) is not None,
        lambda s: re.search(r'\basthma', s) is not None and re.search(r'\ballergic', s) is not None,
        lambda s: s.endswith('itis'),
        lambda s: re.search(r'\bexercise|\ballergic', s) is not None,
        lambda s: re.search(r'\ballergic', s) is None,
        lambda s: re.search(r'\bzzznomatch', s) is not None,
    ]
    assert [q['id'] for q in queries] == [f'term-{i:02}' for i in range(1, 10)]
    expected = [set() for _ in queries]
    names_only = [set() for _ in queries]
    with zipfile.ZipFile(args.archive) as archive:
        names = [n for n in archive.namelist() if '/Snapshot/' in n and n.endswith('.txt')]
        children = defaultdict(set)
        for name in names:
            if name.rsplit('/', 1)[-1].startswith('sct2_Relationship_'):
                for row in rows(archive, name):
                    if (row['active'] == '1' and row['characteristicTypeId'] == '900000000000011006'
                            and row['typeId'] == '116680003'):
                        children[row['destinationId']].add(row['sourceId'])
        populations = []
        for seed in ['64572001', '195967001']:
            found, todo = set(), [seed]
            while todo:
                code = todo.pop()
                if code not in found:
                    found.add(code)
                    todo.extend(children[code] - found)
            if seed == '64572001':
                found.remove(seed)
            populations.append(found)
        del children
        for name in names:
            if not name.rsplit('/', 1)[-1].startswith(('sct2_Description_', 'sct2_TextDefinition_')):
                continue
            for row in rows(archive, name):
                if row['active'] != '1':
                    continue
                code = row['conceptId']
                applicable = ([0] if code in populations[0] else [])
                if code in populations[1]:
                    applicable.extend(range(1, len(queries)))
                if not applicable:
                    continue
                assert row['languageCode'] == 'en'
                term = normalise(row['term'])
                for i in applicable:
                    if predicates[i](term):
                        expected[i].add(code)
                        if row['typeId'] in ('900000000000003001', '900000000000013009'):
                            names_only[i].add(code)
    scoped = dict(id='term-01-names', ecl=queries[0]['ecl'][:-2] + ',type=(syn fsn)}}')
    local_queries = queries + [scoped]
    expected.append(names_only[0])
    names_only.append(names_only[0])
    wrapper = '"$@"; status=$?; cat /sys/fs/cgroup/memory/memory.max_usage_in_bytes >&2; exit "$status"'
    command = ['docker', 'run', '--rm', '-i', '--cpus', '1', '--memory', f'{args.memory_mib}m', '--memory-swap', f'{args.memory_mib}m',
               '--mount', f'type=bind,source={ROOT},target=/work,readonly', '-w', '/work', IMAGE,
               'sh', '-c', wrapper, 'terms-check', args.binary, 'batch', args.store.resolve().relative_to(ROOT).as_posix()]
    requests = [dict(ecl=q['ecl']) for q in local_queries]
    for _ in range(5):
        requests.extend(dict(ecl=q['ecl'], count_only=True) for q in local_queries)
    run = subprocess.run(command, input=''.join(json.dumps(q)+'\n' for q in requests), capture_output=True,
                         text=True, encoding='utf-8', timeout=180, check=True)
    responses = [json.loads(line) for line in run.stdout.splitlines()]
    assert len(responses) == len(requests)
    assert all('error' not in r and r['edition'] == manifest['edition'] for r in responses), responses
    reference = json.loads(args.reference.read_text(encoding='utf-8-sig'))
    assert reference['edition'] == manifest['edition']
    reference_rows = {r['id']: r for r in reference['results']}
    results = []
    for i, (query, codes, response) in enumerate(zip(local_queries, expected, responses[:len(local_queries)], strict=True)):
        actual = set(response['codes'])
        warm = [responses[j]['eval_ms'] for j in range(len(local_queries)+i, len(responses), len(local_queries))]
        assert all(responses[j]['total'] == len(codes) for j in range(len(local_queries)+i, len(responses), len(local_queries)))
        ref_id = queries[0]['id'] if query == scoped else query['id']
        ref = reference_rows[ref_id]
        ref_path = args.reference.parent / (ref_id+'.codes.txt')
        ref_codes = set(ref_path.read_text(encoding='utf-8-sig').splitlines()) if ref_path.exists() else set()
        assert ref['complete'] and ref['reportedVersions'] == [manifest['edition']]
        assert len(ref_codes) == ref['total'] and digest(ref_codes) == ref['sha256']
        assert ref['ecl'] == (queries[0]['ecl'] if query == scoped else query['ecl'])
        results.append(dict(id=query['id'], ecl=query['ecl'], expected_total=len(codes), observed_total=response['total'],
                            rf2_matches=actual == codes and len(actual) == len(response['codes']) == response['total'],
                            expected_sha256=digest(codes), observed_sha256=digest(actual),
                            names_only_total=len(names_only[i]), names_only_sha256=digest(names_only[i]),
                            ontoserver_total=len(ref_codes), ontoserver_sha256=digest(ref_codes),
                            ontoserver_matches=actual == ref_codes,
                            rust_only=len(actual-ref_codes), ontoserver_only=len(ref_codes-actual),
                            first_eval_ms=response['eval_ms'], warm_eval_ms=warm, median_warm_eval_ms=statistics.median(warm)))
    binary = (ROOT / args.binary).read_bytes()
    report = dict(edition=manifest['edition'], archive_sha256=manifest['archive_sha256'],
                  memory_limit_mib=args.memory_mib, container_charged_peak_bytes=int(run.stderr.strip().splitlines()[-1]),
                  description_index=manifest['descriptions'], binary_bytes=len(binary),
                  binary_gzip_bytes=len(gzip.compress(binary, mtime=0)), binary_sha256=hashlib.sha256(binary).hexdigest(),
                  image=IMAGE, collation='ICU4C 72.1', features=['import', 'unicode'],
                  reference_sha256=hashlib.sha256(args.reference.read_bytes()).hexdigest(),
                  reference_software=reference['software'], reference_version=reference['softwareVersion'],
                  scope=f'Independent RF2 inferred hierarchy and all active descriptions including definitions. Fixed unaccented English probes only; Python normalisation is not a general UCA reference. Full result sets, one CPU, {args.memory_mib} MiB. First query includes lazy loading; warm figures are engine evaluation only. The extra type-scoped Rust query is compared to the unscoped Ontoserver term-01 result to isolate description scope.',
                  results=results)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(queries=len(results), rf2_matches=sum(r['rf2_matches'] for r in results),
                         reference_metadata_keys=list(reference), results=results)))
    if not all(r['rf2_matches'] for r in results):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
