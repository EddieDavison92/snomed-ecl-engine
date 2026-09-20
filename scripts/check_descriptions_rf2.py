"""Check description metadata probes against independent RF2 hierarchy and row scans."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile

from benchmark_ecl import ROOT, IMAGE, digest
from check_membership_rf2 import rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--store', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new report path')
    manifest = json.loads((args.store / 'manifest.json').read_text())
    with args.archive.open('rb') as file:
        assert hashlib.file_digest(file, 'sha256').hexdigest() == manifest['archive_sha256']
    descriptions = {}
    members = defaultdict(set)
    counts = defaultdict(int)
    with zipfile.ZipFile(args.archive) as archive:
        names = [n for n in archive.namelist() if '/Snapshot/' in n and n.endswith('.txt')]
        children = defaultdict(set)
        for name in names:
            if not name.rsplit('/', 1)[-1].startswith('sct2_Relationship_'):
                continue
            for row in rows(archive, name):
                if row['active'] == '1' and row['characteristicTypeId'] == '900000000000011006' and row['typeId'] == '116680003':
                    children[row['destinationId']].add(row['sourceId'])
        candidates, todo = set(), ['195967001']
        while todo:
            code = todo.pop()
            if code not in candidates:
                candidates.add(code)
                todo.extend(children[code] - candidates)
        del children
        for name in names:
            if not name.rsplit('/', 1)[-1].startswith(('sct2_Description_', 'sct2_TextDefinition_')):
                continue
            for row in rows(archive, name):
                counts['descriptions'] += 1
                counts['active_descriptions'] += row['active'] == '1'
                if row['conceptId'] in candidates:
                    descriptions[row['id']] = row
        for name in names:
            if not name.rsplit('/', 1)[-1].startswith('der2_cRefset_Language'):
                continue
            for row in rows(archive, name):
                if row['active'] == '1':
                    counts['language_memberships'] += 1
                    if row['referencedComponentId'] in descriptions:
                        members[row['referencedComponentId']].add((row['refsetId'], row['acceptabilityId']))
    gb, us, prefer, accept = '900000000000508004', '900000000000509007', '900000000000548007', '900000000000549004'
    fsn, syn, definition = '900000000000003001', '900000000000013009', '900000000000550004'
    checks = {
        'type=fsn': lambda d,m: d['typeId'] == fsn,
        'type=syn': lambda d,m: d['typeId'] == syn,
        'type=def': lambda d,m: d['typeId'] == definition,
        'type=(fsn syn)': lambda d,m: d['typeId'] in (fsn,syn),
        'typeId=900000000000013009': lambda d,m: d['typeId'] == syn,
        'language=en': lambda d,m: d['languageCode'] == 'en',
        'language=sv': lambda d,m: d['languageCode'] == 'sv',
        'language!=(en sv)': lambda d,m: d['languageCode'] not in ('en','sv'),
        'active=0': lambda d,m: d['active'] == '0',
        'active=*': lambda d,m: True,
        'moduleId=900000000000207008': lambda d,m: d['moduleId'] == '900000000000207008',
        'effectiveTime>="20200101"': lambda d,m: d['effectiveTime'] >= '20200101',
        'dialect=en-gb': lambda d,m: any(r == gb for r,a in m),
        'dialect=en-gb (prefer)': lambda d,m: (gb,prefer) in m,
        'dialect=en-us (accept)': lambda d,m: (us,accept) in m,
        'dialectId=(900000000000508004 900000000000509007) (prefer)': lambda d,m: bool(m & {(gb,prefer),(us,prefer)}),
        'dialect=en-gb, dialect=en-us': lambda d,m: any(r == gb for r,a in m) and any(r == us for r,a in m),
        'type=fsn, dialect=en-gb (prefer)': lambda d,m: d['typeId'] == fsn and (gb,prefer) in m,
    }
    queries = json.loads((ROOT / 'validation/description-metadata-queries.json').read_text())
    expected = []
    for query in queries:
        predicate = query['ecl'].removeprefix('<<195967001 {{D ').removesuffix('}}')
        expected.append({d['conceptId'] for key,d in descriptions.items()
                         if (predicate.startswith('active=') or d['active'] == '1') and checks[predicate](d,members[key])})
    command = ['docker','run','--rm','-i','--cpus','1','--memory','1g','--memory-swap','1g',
               '--mount',f'type=bind,source={ROOT},target=/work,readonly','-w','/work',IMAGE,
               'target/linux/release/snomed-ecl-engine','batch',args.store.resolve().relative_to(ROOT).as_posix()]
    run = subprocess.run(command,input=''.join(json.dumps({'ecl':q['ecl']})+'\n' for q in queries),capture_output=True,text=True,encoding='utf-8',timeout=180,check=True)
    observed = [json.loads(line) for line in run.stdout.splitlines()]
    results = []
    for query, codes, response in zip(queries,expected,observed,strict=True):
        assert response.get('edition') == manifest['edition'] and 'error' not in response, response
        actual = set(response['codes'])
        results.append(dict(id=query['id'],ecl=query['ecl'],expected_total=len(codes),observed_total=response['total'],
                            matches=actual == codes and len(actual) == len(response['codes']) == response['total'],
                            expected_sha256=digest(codes),observed_sha256=digest(actual),eval_ms=response['eval_ms']))
    report = dict(edition=manifest['edition'],archive_sha256=manifest['archive_sha256'],description_index=manifest['descriptions'],
                  rf2_counts=dict(counts),counts_match=all(manifest['descriptions'][k] == v for k,v in counts.items()),
                  scope='Independent active inferred RF2 hierarchy, descriptions, definitions and active language memberships. Complete result sets. One CPU and 1 GiB for Rust; first query includes lazy description loading.',results=results)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(queries=len(results),matches=sum(r['matches'] for r in results),counts_match=report['counts_match'])))
    if not report['counts_match'] or not all(r['matches'] for r in results):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
