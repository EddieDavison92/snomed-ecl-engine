"""Compare fixed member-filter probes with a separate RF2 row scan and Ontoserver."""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import zipfile

from benchmark_ecl import ROOT, IMAGE, digest
from check_membership_rf2 import rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--store', required=True, type=Path)
    parser.add_argument('--reference', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--binary', default='target/linux-unicode/release/snomed-ecl-engine')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new report path')
    manifest = json.loads((args.store / 'manifest.json').read_text())
    with args.archive.open('rb') as file:
        assert hashlib.file_digest(file, 'sha256').hexdigest() == manifest['archive_sha256']
    queries = json.loads((ROOT / 'validation/member-field-queries.json').read_text())
    assert [q['id'] for q in queries] == [f'member-field-{i:02}' for i in range(1, 10)]
    expected = [set() for _ in queries]
    tuples = []
    with zipfile.ZipFile(args.archive) as archive:
        for name in archive.namelist():
            if '/Snapshot/' not in name or 'Refset' not in name or not name.endswith('.txt'):
                continue
            for row in rows(archive, name):
                refset, code = row['refsetId'], row['referencedComponentId']
                active = row['active'] == '1'
                if refset == '999002271000000101':
                    target = row['mapTarget']
                    # These fixed probes use ASCII map codes, not general Unicode term matching.
                    flags = [active and target == 'J459', active and target.startswith('J45'),
                             active and row['mapGroup'] == '1' and row['mapPriority'] == '1' and target == 'J459',
                             active and row['mapGroup'] != '2' and int(row['mapPriority']) < 2 and target.startswith('J45'),
                             False, False, not active and target == 'J459',
                             active and row['effectiveTime'] >= '20200101' and target == 'J459', False]
                    for i, match in enumerate(flags):
                        if match:
                            expected[i].add(code)
                    if flags[2]:
                        tuples.append({'referencedComponentId': {'type': 'concept', 'value': code},
                                       'mapTarget': {'type': 'string', 'value': target},
                                       'mapGroup': {'type': 'number', 'value': row['mapGroup']}})
                elif refset == '900000000000527005' and active:
                    if code == '67415000':
                        expected[4].add(row['targetComponentId'])
                    if row['targetComponentId'] == '195967001':
                        expected[5].add(code)
                elif refset == '900000000000534007' and active and row['sourceEffectiveTime'] >= '20260101':
                    expected[8].add(code)
    tuple_query = dict(ecl='^[referencedComponentId,mapTarget,mapGroup]999002271000000101 {{M mapGroup=#1,mapPriority=#1,mapTarget=wild:"J459"}}')
    requests = [dict(ecl=q['ecl']) for q in queries] + [tuple_query]
    for _ in range(3):
        requests.extend(dict(ecl=q['ecl'], count_only=True) for q in queries)
    binary = (ROOT / args.binary).read_bytes()
    command = ['docker', 'run', '--rm', '-i', '--cpus', '1', '--memory', '1g', '--memory-swap', '1g',
               '--mount', f'type=bind,source={ROOT},target=/work,readonly', '-w', '/work', IMAGE,
               args.binary, 'batch', args.store.resolve().relative_to(ROOT).as_posix()]
    run = subprocess.run(command, input=''.join(json.dumps(q)+'\n' for q in requests), capture_output=True,
                         text=True, encoding='utf-8', timeout=240, check=True)
    responses = [json.loads(line) for line in run.stdout.splitlines()]
    assert len(responses) == len(requests)
    assert all('error' not in r and r['edition'] == manifest['edition'] for r in responses), responses
    reference = json.loads(args.reference.read_text(encoding='utf-8-sig'))
    assert reference['edition'] == manifest['edition']
    reference_rows = {r['id']: r for r in reference['results']}
    results = []
    for i, (query, codes, response) in enumerate(zip(queries, expected, responses[:len(queries)], strict=True)):
        actual = set(response['codes'])
        warm = [responses[j]['eval_ms'] for j in range(len(queries)+1+i, len(responses), len(queries))]
        assert all(responses[j]['total'] == len(codes) for j in range(len(queries)+1+i, len(responses), len(queries)))
        result = dict(id=query['id'], ecl=query['ecl'], expected_total=len(codes), observed_total=response['total'],
                      rf2_matches=actual == codes and len(actual) == len(response['codes']) == response['total'],
                      expected_sha256=digest(codes), observed_sha256=digest(actual),
                      first_eval_ms=response['eval_ms'], warm_eval_ms=warm, median_warm_eval_ms=statistics.median(warm))
        ref = reference_rows[query['id']]
        assert ref['ecl'] == query['ecl']
        result['ontoserver_complete'] = ref['complete']
        if ref['complete']:
            ref_codes = set((args.reference.parent / (query['id']+'.codes.txt')).read_text(encoding='utf-8-sig').splitlines())
            assert ref['reportedVersions'] == [manifest['edition']]
            assert len(ref_codes) == ref['total'] and digest(ref_codes) == ref['sha256']
            result.update(ontoserver_total=len(ref_codes), ontoserver_sha256=digest(ref_codes),
                          ontoserver_matches=actual == ref_codes)
        else:
            result['ontoserver_http_status'] = ref['httpStatus']
        results.append(result)
    canonical = lambda rows: sorted(json.dumps(row, sort_keys=True) for row in rows)
    tuple_result = responses[len(queries)]
    tuple_matches = tuple_result['result_type'] == 'rows' and canonical(tuple_result['rows']) == canonical(tuples)
    report = dict(edition=manifest['edition'], archive_sha256=manifest['archive_sha256'],
                  member_table_count=len(manifest['member_tables']),
                  member_bytes=sum(t['bytes'] for t in manifest['member_tables']),
                  member_rows=sum(t['rows'] for t in manifest['member_tables']),
                  binary_bytes=len(binary), binary_sha256=hashlib.sha256(binary).hexdigest(), image=IMAGE,
                  reference_software=reference['software'], reference_version=reference['softwareVersion'],
                  scope='Fixed ASCII map-code predicates, association projections and module dates. Independent complete RF2 sets and member tuples. One CPU, 1 GiB; first evaluations include lazy file loading. HTTP failures are not conformance matches.',
                  tuple_query=tuple_query['ecl'], tuple_count=len(tuples), tuple_matches=tuple_matches, results=results)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(rf2_matches=sum(r['rf2_matches'] for r in results), tuple_matches=tuple_matches,
                         counts=[r['observed_total'] for r in results])))
    if not tuple_matches or not all(r['rf2_matches'] for r in results):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
