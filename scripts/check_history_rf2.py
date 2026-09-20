"""Check history profiles against independent RF2 hierarchy and association scans."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile

from benchmark_ecl import ROOT, IMAGE, digest
from check_membership_rf2 import rows

SAME = '900000000000527005'
MOD = {SAME, '900000000000526001', '900000000000528000', '1186924009'}
MOVED_FROM = '900000000000525002'
MOVED_TO = '900000000000524003'


def descendants(children, seed):
    seen, todo = set(), list(children[seed])
    while todo:
        code = todo.pop()
        if code not in seen:
            seen.add(code)
            todo.extend(children[code])
    return seen


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--store', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--binary', default='target/linux-unicode/release/snomed-ecl-engine')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new report path')
    manifest = json.loads((args.store / 'manifest.json').read_text())
    with args.archive.open('rb') as file:
        assert hashlib.file_digest(file, 'sha256').hexdigest() == manifest['archive_sha256']
    children, associations = defaultdict(list), defaultdict(list)
    with zipfile.ZipFile(args.archive) as archive:
        names = [n for n in archive.namelist() if '/Snapshot/' in n and n.endswith('.txt')]
        concepts = {r['id'] for n in names if n.rsplit('/', 1)[-1].startswith('sct2_Concept_')
                    for r in rows(archive, n)}
        for name in names:
            if name.rsplit('/', 1)[-1].startswith('sct2_Relationship_'):
                for row in rows(archive, name):
                    if row['active'] == '1' and row['typeId'] == '116680003' and row['characteristicTypeId'] == '900000000000011006':
                        children[row['destinationId']].append(row['sourceId'])
            elif 'Association' in name:
                for row in rows(archive, name):
                    if (row['active'] == '1' and 'targetComponentId' in row
                            and int(row['referencedComponentId']) // 10 % 100 in (0, 10)):
                        associations[row['refsetId']].append((row['referencedComponentId'], row['targetComponentId']))
    maximum = descendants(children, '900000000000522004')
    probes = json.loads((ROOT / 'validation/history-queries.json').read_text())
    cases = []
    for query in probes:
        seed = query['seed']
        initial = ({seed} | descendants(children, seed)) if query.get('descendants') else {seed}
        initial &= concepts
        selection = {'MIN': {SAME}, 'MOD': MOD, 'MAX': maximum, 'REPLACED': {'900000000000526001'}}[query['profile']]
        cases.append((query, initial, selection))
    corpus = json.loads((ROOT / 'validation/ecl-1000.json').read_text())
    for query in corpus['cases']:
        if query['category'] == 'history':
            cases.append((query, {query['ecl'].split()[0]} & concepts, {SAME}))
    expected = []
    for _, initial, selection in cases:
        result = set(initial)
        for refset in selection | ({MOVED_FROM} if SAME in selection else set()):
            if refset == MOVED_TO:
                continue
            for source, target in associations[refset]:
                if refset == MOVED_FROM:
                    source, target = target, source
                if target in initial:
                    assert source in concepts, f'History source is absent from this edition (refset {refset})'
                    result.add(source)
        expected.append(result)
    binary = (ROOT / args.binary).read_bytes()
    command = ['docker', 'run', '--rm', '-i', '--cpus', '1', '--memory', '1g', '--memory-swap', '1g',
               '--mount', f'type=bind,source={ROOT},target=/work,readonly', '-w', '/work', IMAGE,
               args.binary, 'batch', args.store.resolve().relative_to(ROOT).as_posix()]
    run = subprocess.run(command, input=''.join(json.dumps({'ecl': q['ecl']})+'\n' for q, _, _ in cases),
                         text=True, encoding='utf-8', capture_output=True, timeout=300, check=True)
    responses = [json.loads(line) for line in run.stdout.splitlines()]
    assert len(responses) == len(cases)
    references = {}
    if args.reference:
        reference = json.loads(args.reference.read_text(encoding='utf-8-sig'))
        assert reference['edition'] == manifest['edition']
        references = {r['id']: r for r in reference['results']}
    results = []
    for (query, _, _), want, response in zip(cases, expected, responses, strict=True):
        assert 'error' not in response, response
        actual = set(response['codes'])
        result = dict(id=query['id'], ecl=query['ecl'], total=len(want), sha256=digest(want),
                      rf2_matches=actual == want and response['total'] == len(response['codes']) == len(actual),
                      eval_ms=response['eval_ms'])
        if query['id'] in references:
            ref = references[query['id']]
            assert ref['ecl'] == query['ecl']
            result['ontoserver_complete'] = ref['complete']
            if ref['complete']:
                assert ref['reportedVersions'] == [manifest['edition']]
                codes = set((args.reference.parent / (query['id']+'.codes.txt')).read_text(encoding='utf-8-sig').splitlines())
                assert digest(codes) == ref['sha256'] and len(codes) == ref['total']
                result['ontoserver_matches'] = actual == codes
            else:
                result['ontoserver_http_status'] = ref['httpStatus']
        results.append(result)
    report = dict(edition=manifest['edition'], archive_sha256=manifest['archive_sha256'],
                  binary_sha256=hashlib.sha256(binary).hexdigest(), image=IMAGE,
                  scope='One-hop history with reversed MOVED FROM and ignored MOVED TO. Independent RF2 sets; no engine results used to construct expectations.',
                  results=results)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    assert all(r['rf2_matches'] for r in results), 'RF2 comparison failed; inspect report'
    print(json.dumps({'cases': len(results), 'rf2_matches': sum(r['rf2_matches'] for r in results),
                      'ontoserver_matches': sum(r.get('ontoserver_matches', False) for r in results)}))


if __name__ == '__main__':
    main()
