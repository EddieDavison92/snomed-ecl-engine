"""Derive the README's paired request medians from the frozen comparison reports."""
import hashlib
import json
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parent.parent
lite_path = ROOT / 'docs/basic-ecl-results.json'
lite = json.loads(lite_path.read_text())
matched = [r for r in lite['results'] if r['matches_snowstorm']]
report = {
    'lite_source_sha256': hashlib.sha256(lite_path.read_bytes()).hexdigest(),
    'lite_complete_matches': len(matched),
    'lite_timed_samples_per_engine': sum(len(r['rust_transport']['samples_ms']) for r in matched),
    'lite_scope': 'Pooled raw request samples for 18 complete-set matches; 17 small cases have three samples each and one broad case has one. Rust JSONL versus paginated FHIR HTTP with displays. The four mismatched cases are excluded; see the historical default-population correction in docs/benchmarks.md.',
    'rust_lite_cohort_request_median_ms': statistics.median(v for r in matched for v in r['rust_transport']['samples_ms']),
    'lite_request_median_ms': statistics.median(v for r in matched for v in r['snowstorm_http']['samples_ms']),
    'full_snowstorm_source': 'docs/full-snowstorm-results.json',
    'full_snowstorm_scope': 'Use the separately documented 719-expression cohort in docs/benchmarks.md; do not compare its latency directly with the Lite cohort.',
}
full_path = ROOT / report['full_snowstorm_source']
full = json.loads(full_path.read_text())
paired = [r for r in full['results'] if r.get('matches_snowstorm') is True]
report.update({
    'full_snowstorm_source_sha256': hashlib.sha256(full_path.read_bytes()).hexdigest(),
    'full_snowstorm_complete_matches': len(paired),
    'full_snowstorm_timed_samples_per_engine': sum(len(r['rust_request_samples_ms']) for r in paired),
    'rust_full_cohort_request_median_ms': statistics.median(v for r in paired for v in r['rust_request_samples_ms']),
    'full_snowstorm_request_median_ms': statistics.median(v for r in paired for v in r['snowstorm_count_samples_ms']),
})
(ROOT / 'validation/readme-comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
