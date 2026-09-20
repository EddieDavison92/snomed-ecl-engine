"""Record compact corpus evidence and require all earlier complete sets to survive."""
import argparse
import hashlib
import json
import math
import statistics
from pathlib import Path


def compare_prior(report, prior, corpus=None, prior_corpus=None):
    for key in ('edition', 'archive_sha256'):
        if report[key] != prior[key]:
            raise ValueError(f'Changed comparison input: {key}')
    if report['corpus_sha256'] != prior['corpus_sha256']:
        if corpus is None or prior_corpus is None:
            raise ValueError('Changed corpus requires --corpus and --prior-corpus')
        for document, run in ((corpus, report), (prior_corpus, prior)):
            if hashlib.sha256(document).hexdigest() != run['corpus_sha256']:
                raise ValueError('Corpus checksum does not match its run')
        current_cases = {r['id']: r for r in json.loads(corpus)['cases']}
        for row in json.loads(prior_corpus)['cases']:
            if current_cases.get(row['id']) != row:
                raise ValueError(f'Changed or missing regression expression: {row["id"]}')
        for row in report['results']:
            if any(row[key] != current_cases[row['id']][key] for key in ('ecl', 'category')):
                raise ValueError(f'Report expression differs from corpus: {row["id"]}')
        if {r['id'] for r in report['results']} != set(current_cases):
            raise ValueError('Report does not cover the entire corpus')
    current = {r['id']: r for r in report['results']}
    if len(current) != len(report['results']):
        raise ValueError('Duplicate case IDs')
    preserved = 0
    for row in prior['result_digests']:
        if row['status'] == 'evaluated':
            if row['id'] not in current or any(current[row['id']].get(key) != row[key]
                                              for key in ('status', 'total', 'sha256')):
                raise ValueError(f'Changed complete result set: {row["id"]}')
            preserved += 1
    return preserved


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--prior', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--corpus', type=Path, help='Current corpus when extending the workload')
    parser.add_argument('--prior-corpus', type=Path, help='Prior corpus when extending the workload')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new output path')
    report = json.loads(args.report.read_text(encoding='utf-8-sig'))
    prior = json.loads(args.prior.read_text(encoding='utf-8-sig'))
    assert report['exit_code'] == 0
    preserved = compare_prior(report, prior,
                              args.corpus.read_bytes() if args.corpus else None,
                              args.prior_corpus.read_bytes() if args.prior_corpus else None)
    summary = {k: v for k, v in report.items() if k not in ('results', 'startup_log')}
    for category in summary.get('categories', {}).values():
        for key in ('evaluation', 'snowstorm_count_http'):
            if category.get(key):
                samples = category[key].pop('samples_ms', [])
                category[key]['sample_count'] = len(samples)
    times = sorted(t for r in report['results'] for t in r.get('rust_request_samples_ms', []))
    assert times, 'No request samples'
    summary.update(
        features=['import', 'unicode'],
        source_sha256=hashlib.sha256(args.report.read_bytes()).hexdigest(),
        prior_sha256=hashlib.sha256(args.prior.read_bytes()).hexdigest(),
        prior_complete_result_sets_preserved=preserved,
        median_request_ms=statistics.median(times),
        p95_request_ms=times[math.ceil(len(times)*.95)-1],
        result_digests=[{k: r[k] for k in ('id', 'status', 'total', 'sha256') if k in r}
                        for r in report['results']],
        result_sizes={
            'empty': sum(r.get('total') == 0 for r in report['results']),
            'singleton': sum(r.get('total') == 1 for r in report['results']),
            'multiple': sum(r.get('total', 0) > 1 for r in report['results']),
            'largest': max(r.get('total', 0) for r in report['results']),
        },
    )
    args.output.write_text(json.dumps(summary, indent=2)+'\n', encoding='utf-8')
    print(json.dumps({k: summary[k] for k in ('status_counts', 'prior_complete_result_sets_preserved',
                                             'median_request_ms', 'p95_request_ms')}))


if __name__ == '__main__':
    main()
