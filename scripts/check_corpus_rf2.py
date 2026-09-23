"""Check generated hierarchy and historical projection cases directly against RF2."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re
import zipfile

from common import digest
from check_membership_rf2 import rows


PATTERNS = {
    'descendants': r'<< (\d+)', 'ancestors': r'> (\d+)',
    'children': r'<! (\d+)', 'parents': r'>! (\d+)',
    'union': r'\(<< (\d+)\) OR \(<< (\d+)\)',
    'intersection': r'\(<< (\d+)\) AND \(<< (\d+)\)',
    'exclusion': r'\(<< (\d+)\) MINUS \(<< (\d+)\)',
    'nested-hierarchy': r'>! \(<! (\d+)\)',
    'top': r'!!> \((\d+) OR (\d+)\)',
    'bottom': r'!!< \((\d+) OR (\d+)\)',
    'strict-descendants': r'< (\d+)', 'ancestors-or-self': r'>> (\d+)',
    'children-or-self': r'<<! (\d+)', 'parents-or-self': r'>>! (\d+)',
    'member-projection': r'\^ \[targetComponentId\] 900000000000526001 \{\{ M referencedComponentId = (\d+) \}\}',
}


def closure(graph, start):
    found, pending = set(), list(graph.get(start, ()))
    while pending:
        node = pending.pop()
        if node not in found:
            found.add(node)
            pending.extend(graph.get(node, ()))
    return found


def expected(category, ids, parents, children, associations):
    a = ids[0]
    down = lambda code: closure(children, code) | {code}
    if category == 'descendants':
        return down(a)
    if category == 'strict-descendants':
        return closure(children, a)
    if category == 'ancestors':
        return closure(parents, a)
    if category == 'ancestors-or-self':
        return closure(parents, a) | {a}
    if category in ('children', 'children-or-self', 'parents', 'parents-or-self'):
        graph = children if category.startswith('children') else parents
        return set(graph.get(a, ())) | ({a} if category.endswith('or-self') else set())
    if category == 'nested-hierarchy':
        return {parent for child in children.get(a, ()) for parent in parents.get(child, ())}
    if category in ('top', 'bottom'):
        graph = parents if category == 'top' else children
        selected = set(ids)
        return {code for code in selected if not (closure(graph, code) & selected)}
    if category == 'member-projection':
        return associations.get(a, set())
    left, right = down(a), down(ids[1])
    return {'union': set.union, 'intersection': set.intersection, 'exclusion': set.difference}[category](left, right)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--corpus', type=Path, default=Path('validation/ecl-10000.json'))
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new output path')
    corpus = json.loads(args.corpus.read_text(encoding='utf-8'))
    report = json.loads(args.report.read_text(encoding='utf-8'))
    assert hashlib.sha256(args.corpus.read_bytes()).hexdigest() == report['corpus_sha256']
    with args.archive.open('rb') as file:
        archive_hash = hashlib.file_digest(file, 'sha256').hexdigest()
    assert archive_hash == report['archive_sha256'] == corpus['archive_sha256']
    parents, children, associations = defaultdict(set), defaultdict(set), defaultdict(set)
    with zipfile.ZipFile(args.archive) as archive:
        for name in archive.namelist():
            if '/Snapshot/' not in name:
                continue
            relationship = Path(name).name.startswith('sct2_Relationship_')
            association = Path(name).name.startswith('der2_cRefset_Association')
            if not (relationship or association):
                continue
            for row in rows(archive, name):
                if row['active'] != '1':
                    continue
                if relationship and row['typeId'] == '116680003' and row['characteristicTypeId'] == '900000000000011006':
                    source, target = int(row['sourceId']), int(row['destinationId'])
                    parents[source].add(target)
                    children[target].add(source)
                elif association and row['refsetId'] == '900000000000526001':
                    associations[int(row['referencedComponentId'])].add(int(row['targetComponentId']))
    actual = {row['id']: row for row in report['results']}
    checked = []
    for case in corpus['cases']:
        if case['category'] not in PATTERNS:
            continue
        match = re.fullmatch(PATTERNS[case['category']], case['ecl'])
        assert match, case['id']
        values = expected(case['category'], list(map(int, match.groups())), parents, children, associations)
        row = actual[case['id']]
        expected_hash = digest({str(code) for code in values})
        assert row['ecl'] == case['ecl'] and row['status'] == 'evaluated', case['id']
        assert row['total'] == len(values) and row['sha256'] == expected_hash, case['id']
        checked.append(dict(id=case['id'], total=len(values), sha256=expected_hash, rf2_matches=True))
    evidence = dict(edition=report['edition'], archive_sha256=archive_hash,
                    corpus_sha256=report['corpus_sha256'], binary_sha256=report['binary_sha256'],
                    source_report_sha256=hashlib.sha256(args.report.read_bytes()).hexdigest(),
                    scope='Independent Python traversal of active inferred RF2 IS-A rows and active REPLACED BY projections. Compares complete-set digests and counts for matching generated templates, not all language semantics.',
                    results=checked)
    args.output.write_text(json.dumps(evidence, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(dict(cases=len(checked), rf2_matches=len(checked))))


if __name__ == '__main__':
    main()
