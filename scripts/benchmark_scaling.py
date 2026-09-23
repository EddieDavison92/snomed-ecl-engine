"""Measure shared-index library throughput under aggregate Docker CPU and RAM limits."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess

from common import IMAGE, ROOT, summary


def checksum(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def compact(raw):
    batches = [r['wall_ms'] for r in raw['rounds']]
    requests = [m['request_ms'] for r in raw['rounds'] for m in r['measurements']]
    evaluation = [m['eval_ms'] for r in raw['rounds'] for m in r['measurements']]

    def distribution(values):
        result = summary(values)
        del result['samples_ms']
        return dict(result, sample_count=len(values))

    categories = {}
    for category in sorted({c['category'] for c in raw['cases']}):
        indices = {i for i, c in enumerate(raw['cases']) if c['category'] == category}
        values = [m['request_ms'] for r in raw['rounds'] for m in r['measurements']
                  if m['case'] in indices]
        categories[category] = distribution(values)
    result = {key: value for key, value in raw.items() if key not in ('rounds', 'cases')}
    result.update(batch_wall=summary(batches), request=distribution(requests),
                  evaluation=distribution(evaluation), categories=categories,
                  expressions_per_second=raw['verified_complete_sets'] * 1000 / summary(batches)['median_ms'])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--binary', default='target/linux-unicode/release/examples/benchmark_scaling')
    parser.add_argument('--store', default='data/uk.ecl')
    parser.add_argument('--corpus', default='validation/ecl-10000.json')
    parser.add_argument('--baseline', default='validation/engine-10000-results.json')
    parser.add_argument('--samples', type=int, default=5)
    parser.add_argument('--configuration', action='append', help='CPU count,memory MiB,worker threads; repeat to compare')
    args = parser.parse_args()
    if args.output.exists() or args.samples < 1:
        parser.error('Choose a new output and a positive sample count')
    configurations = [tuple(map(int, c.split(','))) for c in
                      (args.configuration or ['1,512,1', '4,1024,1', '2,512,2', '4,1024,4'])]
    if any(len(c) != 3 or min(c) < 1 for c in configurations):
        parser.error('Each configuration needs positive CPU, memory and worker counts')
    baseline = json.loads((ROOT / args.baseline).read_text(encoding='utf-8'))
    pins = {key: baseline[key] for key in ('edition', 'archive_sha256', 'corpus_sha256', 'container_sha256')}
    if checksum(ROOT / args.corpus) != pins['corpus_sha256'] or checksum(ROOT / args.store) != pins['container_sha256']:
        parser.error('Corpus or index differs from the baseline')
    report = dict(pins, recorded_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  image=IMAGE, binary_sha256=checksum(ROOT / args.binary),
                  example_source_sha256=checksum(ROOT / 'examples/benchmark_scaling.rs'),
                  runner_source_sha256=checksum(Path(__file__)),
                  baseline_sha256=checksum(ROOT / args.baseline),
                  git_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                  docker_info=json.loads(subprocess.check_output(['docker', 'info', '--format', '{{json .}}'], text=True)),
                  complete=False, runs=[],
                  scope='Sequential container runs on the same host; no competing task containers. One shared index per process. All CPU and RAM limits apply to the whole container, with no extra swap. Warm library calls, no JSONL or HTTP transport. Do not compare absolute times directly with the earlier CLI corpus benchmark. Filesystem caches retained. Host is not exclusively reserved. No full-ECL or serverless cold-start claim.')
    # Record hardware and Docker versions without host paths or unrelated configuration.
    report['docker_info'] = {key: report['docker_info'].get(key) for key in
                           ('NCPU', 'MemTotal', 'ServerVersion', 'KernelVersion', 'OperatingSystem', 'Architecture', 'CgroupVersion')}
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def save():
        args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')

    save()
    for number, (cpus, memory, workers) in enumerate(configurations):
        name = f'snomed-scaling-{cpus}cpu-{workers}workers'
        stem = f'{args.output.stem}-{number + 1}-{cpus}cpu-{memory}m-{workers}workers'
        raw_path = ROOT / 'data/validation' / (stem + '.json')
        log_path = ROOT / '.local' / (stem + '.log')
        if raw_path.exists() or log_path.exists():
            raise ValueError(f'Existing run files: {stem}')
        raw_path.parent.mkdir(parents=True, exist_ok=True)
        log_path.parent.mkdir(parents=True, exist_ok=True)
        command = ['docker', 'run', '--rm', '--name', name, '--cpus', str(cpus),
                   '--memory', f'{memory}m', '--memory-swap', f'{memory}m',
                   '--mount', f'type=bind,source={ROOT},target=/work,readonly', '-w', '/work', IMAGE,
                   args.binary, args.store, args.corpus, args.baseline, str(workers), str(args.samples)]
        print(f'Running {cpus} CPUs, {memory} MiB, {workers} workers', flush=True)
        try:
            with raw_path.open('w', encoding='utf-8') as output, log_path.open('w', encoding='utf-8') as log:
                result = subprocess.run(command, stdout=output, stderr=log, timeout=1800, check=True)
            raw = json.loads(raw_path.read_text(encoding='utf-8'))
            assert raw['verified_complete_sets'] == len(baseline.get('result_digests', baseline.get('results', [])))
            assert raw['cpu_max'].split() == [str(cpus * 100000), '100000']
            assert int(raw['memory_max']) == memory * 1024 * 1024
            run = compact(raw)
            run.update(cpus=cpus, memory_mib=memory, raw_report=str(raw_path.relative_to(ROOT)),
                       raw_sha256=checksum(raw_path), exit_code=result.returncode)
            report['runs'].append(run)
            print(json.dumps({k: run[k] for k in ('cpus', 'workers', 'batch_wall', 'request', 'expressions_per_second', 'memory_peak_bytes')}), flush=True)
            save()
        except BaseException as error:
            subprocess.run(['docker', 'stop', name], capture_output=True, check=False)
            report['failure'] = dict(cpus=cpus, memory_mib=memory, workers=workers, error=str(error))
            save()
            raise
    baseline_ms = report['runs'][0]['batch_wall']['median_ms']
    for run in report['runs']:
        run['throughput_speedup_vs_first'] = baseline_ms / run['batch_wall']['median_ms']
    report['complete'] = True
    save()


if __name__ == '__main__':
    main()
