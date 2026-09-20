"""Record compact corpus evidence and require all earlier complete sets to survive."""
import argparse
import hashlib
import json
import math
import statistics
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--prior', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new output path')
    report = json.loads(args.report.read_text(encoding='utf-8-sig'))
    prior = json.loads(args.prior.read_text(encoding='utf-8-sig'))
    for key in ('edition', 'archive_sha256', 'corpus_sha256'):
        assert report[key] == prior[key], f'Changed comparison input: {key}'
    assert report['exit_code'] == 0
    current = {r['id']: r for r in report['results']}
    assert len(current) == len(report['results']), 'Duplicate case IDs'
    preserved = 0
    for row in prior['result_digests']:
        if row['status'] == 'evaluated':
            assert all(current[row['id']][key] == row[key] for key in ('status', 'total', 'sha256')), row['id']
            preserved += 1
    summary = {k: v for k, v in report.items() if k not in ('results', 'startup_log')}
    times = sorted(t for r in report['results'] for t in r['rust_request_samples_ms'])
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
    )
    args.output.write_text(json.dumps(summary, indent=2)+'\n', encoding='utf-8')
    print(json.dumps({k: summary[k] for k in ('status_counts', 'prior_complete_result_sets_preserved',
                                             'median_request_ms', 'p95_request_ms')}))


if __name__ == '__main__':
    main()
