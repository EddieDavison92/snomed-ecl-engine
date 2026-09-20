"""Check typed scalar projections against independent RF2 decimal and field sets."""
import argparse
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile

from benchmark_ecl import ROOT, IMAGE
from check_membership_rf2 import rows


def number(value):
    value = Decimal(value)
    if value == 0:
        return '0'
    text = format(value, 'f')
    return text.rstrip('0').rstrip('.') if '.' in text else text


def encoded(kind, value):
    return json.dumps(dict(type=kind, value=value), sort_keys=True, separators=(',', ':'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--store', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--binary', default='target/linux-unicode/release/snomed-ecl-engine')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new report path')
    manifest = json.loads((args.store/'manifest.json').read_text())
    with args.archive.open('rb') as file:
        assert hashlib.file_digest(file, 'sha256').hexdigest() == manifest['archive_sha256']
    concrete = set()
    single = set()
    groups = set()
    all_groups = set()
    priority_one_groups = set()
    targets = set()
    with zipfile.ZipFile(args.archive) as archive:
        for name in archive.namelist():
            if '/Snapshot/' not in name or not name.endswith('.txt'):
                continue
            if 'RelationshipConcreteValues' in name:
                for row in rows(archive, name):
                    if (row['active'] == '1' and row['typeId'] == '1142138002'
                            and row['characteristicTypeId'] == '900000000000011006'):
                        value = encoded('number', number(row['value'].removeprefix('#')))
                        concrete.add(value)
                        if row['sourceId'] == '377442002':
                            single.add(value)
            elif 'Map' in name and 'Refset' in name:
                for row in rows(archive, name):
                    if row['refsetId'] != '999002271000000101':
                        continue
                    value = encoded('number', number(row['mapGroup']))
                    all_groups.add(value)
                    if row['active'] != '1':
                        continue
                    groups.add(value)
                    if row['mapPriority'] == '1':
                        priority_one_groups.add(value)
                    if row['mapGroup'] == '1' and row['mapPriority'] == '1':
                        targets.add(encoded('string', row['mapTarget']))
    a = '^[mapGroup]999002271000000101'
    b = '^[mapGroup]999002271000000101 {{M mapPriority=#1}}'
    cases = [
        ('concrete-all', '* . 1142138002', concrete),
        ('concrete-single', '377442002 . 1142138002', single),
        ('concrete-minus', '(* . 1142138002) MINUS (377442002 . 1142138002)', concrete-single),
        ('mixed-or', '377442002 OR (* . 1142138002)', concrete | {encoded('concept', '377442002')}),
        ('mixed-and', '377442002 AND (* . 1142138002)', set()),
        ('mixed-minus', '(* . 1142138002) MINUS 377442002', concrete),
        ('empty-dot-union', '(* . 1142138002) OR ((377442002 MINUS 377442002) . 1142138002)', concrete),
        ('map-groups', a, groups),
        ('map-priority-groups', b, priority_one_groups),
        ('scalar-and', f'({a}) AND ({b})', groups & priority_one_groups),
        ('scalar-or', f'({a}) OR ({b})', groups | priority_one_groups),
        ('scalar-minus', f'({a}) MINUS ({b})', groups-priority_one_groups),
        ('map-targets', '^[mapTarget]999002271000000101 {{M mapGroup=#1,mapPriority=#1}}', targets),
        ('unspaced-member-marker', f'{a} {{{{MmapPriority=#1}}}}', priority_one_groups),
        ('all-member-states', f'{a} {{{{Mactive="*"}}}}', all_groups),
        ('nested-concrete-dot', '((377442002 OR (377442002 . 1142138002)) MINUS (377442002 . 1142138002)) . 1142138002', single),
        ('nested-empty-hierarchy', '<< (377442002 AND (377442002 . 1142138002))', set()),
    ]
    command = ['docker', 'run', '--rm', '-i', '--cpus', '1', '--memory', '1g', '--memory-swap', '1g',
               '--mount', f'type=bind,source={ROOT},target=/work,readonly', '-w', '/work', IMAGE,
               args.binary, 'batch', args.store.as_posix()]
    run = subprocess.run(command, input=''.join(json.dumps(dict(ecl=ecl))+'\n' for _, ecl, _ in cases),
                         text=True, encoding='utf-8', capture_output=True, check=True, timeout=300)
    actual = [json.loads(line) for line in run.stdout.splitlines()]
    assert len(actual) == len(cases)
    result = []
    for (case, ecl, expected), response in zip(cases, actual, strict=True):
        assert 'error' not in response, (case, response)
        assert response['edition'] == manifest['edition'], case
        if case == 'nested-empty-hierarchy':
            assert 'result_type' not in response and 'codes' in response, case
            values = [encoded('concept', code) for code in response['codes']]
        else:
            assert response['result_type'] == 'values', case
            values = [json.dumps(v, sort_keys=True, separators=(',', ':')) for v in response['values']]
        matches = set(values) == expected and len(values) == len(expected) == response['total']
        assert matches, case
        result.append(dict(id=case, ecl=ecl, rf2_matches=matches, total=len(expected),
                           sha256=hashlib.sha256(('\n'.join(sorted(expected))+'\n').encode()).hexdigest(),
                           eval_ms=response['eval_ms']))
    report = dict(edition=manifest['edition'], archive_sha256=manifest['archive_sha256'], image=IMAGE,
                  binary_sha256=hashlib.sha256((ROOT/args.binary).read_bytes()).hexdigest(),
                  scope='Independent RF2 sets. Python Decimal preserves exact numbers. Duplicate scalars collapse; tuple semantics are checked separately.',
                  results=result)
    args.output.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(dict(cases=len(result), rf2_matches=sum(r['rf2_matches'] for r in result))))


if __name__ == '__main__':
    main()
