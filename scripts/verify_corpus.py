"""Runs a corpus through one engine process and checks every complete answer.

A faster engine that returns a different set is not faster, it is wrong, so
this compares each result's total and digest with the ones recorded for the
same store before anything is timed. Timing comes from the engine's own
`eval_ms`, which excludes process start and pipe overhead.

    python scripts/verify_corpus.py BINARY STORE CORPUS DIGESTS [--repeat N]
"""

import argparse
import hashlib
import json
import pathlib
import statistics
import subprocess
import sys


def digest(codes):
    return hashlib.sha256("".join(f"{c}\n" for c in sorted(codes, key=int)).encode()).hexdigest()


parser = argparse.ArgumentParser()
parser.add_argument("binary")
parser.add_argument("store")
parser.add_argument("corpus")
parser.add_argument("digests")
parser.add_argument("--repeat", type=int, default=1, help="evaluations per case; the median is kept")
args = parser.parse_args()

cases = json.loads(pathlib.Path(args.corpus).read_text())["cases"]
report = json.loads(pathlib.Path(args.digests).read_text())
# A benchmark report's results carry the same id, status, total and sha256.
expected = {r["id"]: r for r in report.get("result_digests", report.get("results", []))}
requests = "".join(json.dumps({"ecl": case["ecl"]}) + "\n" for case in cases for _ in range(args.repeat))

run = subprocess.run(
    [args.binary, "batch", args.store], input=requests.encode(), capture_output=True, check=True
)
lines = run.stdout.decode().splitlines()
assert len(lines) == len(cases) * args.repeat, f"{len(lines)} answers for {len(cases) * args.repeat} requests"

mismatches, times, by_category = [], [], {}
for index, case in enumerate(cases):
    answers = [json.loads(lines[index * args.repeat + k]) for k in range(args.repeat)]
    first = answers[0]
    recorded = expected.get(case["id"])
    if "error" in first:
        status = "unsupported" if "Unsupported" in json.dumps(first) else "error"
        if recorded is None or recorded["status"] != status:
            mismatches.append((case["id"], "status", recorded and recorded["status"], status))
        continue
    codes = first.get("codes") or []
    if recorded is None or recorded.get("status") != "evaluated":
        mismatches.append((case["id"], "status", recorded and recorded.get("status"), "evaluated"))
    elif recorded["total"] != first["total"] or recorded["sha256"] != digest(codes):
        mismatches.append((case["id"], "set", recorded["total"], first["total"]))
    elapsed = statistics.median(a["eval_ms"] for a in answers)
    times.append(elapsed)
    by_category.setdefault(case["category"], []).append(elapsed)

times.sort()
print(f"cases {len(cases)} | evaluated {len(times)} | mismatches {len(mismatches)}")
if times:
    q = lambda p: times[min(len(times) - 1, int(p * len(times)))]
    print(f"eval ms: total {sum(times):.1f} | median {q(0.5):.3f} | p95 {q(0.95):.2f} | p99 {q(0.99):.2f} | max {times[-1]:.1f}")
    heaviest = sorted(by_category.items(), key=lambda kv: -sum(kv[1]))[:8]
    print("heaviest categories by total eval time:")
    for category, spent in heaviest:
        print(f"  {category:<32} {sum(spent):9.1f} ms over {len(spent):>5}  (max {max(spent):.1f})")
for item in mismatches[:15]:
    print("  MISMATCH", *item)
sys.exit(1 if mismatches else 0)
