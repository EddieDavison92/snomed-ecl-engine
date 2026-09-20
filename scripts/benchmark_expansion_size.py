"""Measure how a complete expansion costs as the result grows.

A single speed multiple depends on which expressions you picked. This walks a
ladder of expressions chosen to be log-spaced by result size, so the shape of
the relationship is visible and a reader can find their own workload on it.

Both engines return every concept. Sets are compared before any timing counts.
"""
import argparse
import datetime
import hashlib
import json
import statistics
import subprocess
import time
import urllib.parse
from pathlib import Path

from benchmark_corpus import ROOT, IMAGE, digest, http, snowstorm, resource_snapshot
from index_artifact import manifest_bytes, read_manifest

parser = argparse.ArgumentParser()
parser.add_argument("--output", required=True, type=Path)
parser.add_argument("--ladder", type=Path, default=Path("validation/expansion-ladder.json"))
parser.add_argument("--binary", default="target/linux-core/release/snomed-ecl-engine")
parser.add_argument("--store-directory", type=Path,
                    default=Path("data/compact-store/v1-ecl-completion"))
parser.add_argument("--snowstorm", required=True,
                    help="Loopback full Snowstorm URL; requires a completed MAIN import")
parser.add_argument("--import-report", required=True, type=Path)
parser.add_argument("--memory-mib", type=int, default=256)
parser.add_argument("--repeats", type=int, default=4,
                    help="First is cold; the rest measure the server's warm cache")
args = parser.parse_args()

if urllib.parse.urlparse(args.snowstorm).hostname not in ("127.0.0.1", "localhost", "::1"):
    parser.error("This benchmark only permits loopback servers.")
if args.output.exists():
    parser.error("Choose a new report path")
if args.repeats < 1:
    parser.error("Repeats must be positive")

store = (ROOT / args.store_directory).resolve()
# The container is Linux; a Windows path separator would not resolve inside it.
store_argument = store.relative_to(ROOT).as_posix()
manifest = read_manifest(store)
ladder = json.loads((ROOT / args.ladder).read_bytes())

# Same provenance gate as the corpus benchmark: compare only when both sides
# demonstrably hold the same release.
evidence = (ROOT / args.import_report).read_bytes()
prior = json.loads(evidence)
if (prior["edition"] != manifest["edition"]
        or prior["archive_sha256"].lower() != manifest["archive_sha256"].lower()):
    raise ValueError("Prior import report is for a different release")
imports = prior["snowstorm_imports"]
if (imports.get("status") != "COMPLETED" or imports.get("branchPath") != "MAIN"
        or imports.get("type") != "SNAPSHOT"):
    raise ValueError("No completed MAIN import reported")
systems = http(args.snowstorm, "/fhir/CodeSystem", {"url": "http://snomed.info/sct", "_count": 100})
if manifest["edition"] not in [e["resource"].get("version") for e in systems.get("entry", [])]:
    raise ValueError("Snowstorm does not advertise the pinned edition")
branch_before = http(args.snowstorm, "/branches/MAIN", {})
if branch_before != prior["snowstorm_branch_before"]:
    raise ValueError("MAIN differs from the branch in the completed import report")
for ecl, expected in [("*", manifest["active_concept_count"]), ("<< 404684003", 137834)]:
    if http(args.snowstorm, "/MAIN/concepts",
            {"ecl": ecl, "returnIdOnly": "true", "limit": 1})["total"] != expected:
        raise ValueError("Snowstorm release sentinel differs")

binary = (ROOT / args.binary).read_bytes()
report = {
    "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "edition": manifest["edition"],
    "archive_sha256": manifest["archive_sha256"],
    "binary_sha256": hashlib.sha256(binary).hexdigest(),
    "manifest_sha256": hashlib.sha256(manifest_bytes(store)).hexdigest(),
    "import_evidence_sha256": hashlib.sha256(evidence).hexdigest(),
    "memory_limit_mib": args.memory_mib,
    "repeats": args.repeats,
    "ladder_sha256": hashlib.sha256((ROOT / args.ladder).read_bytes()).hexdigest(),
    "scope": (
        "Complete expansion of every rung by both engines, repeated and timed end "
        "to end. This engine runs in a container with one CPU and the stated memory, "
        "answering over JSONL to the parent process. Snowstorm answers over loopback "
        "HTTP, returnIdOnly, at its maximum page size of 10,000, using searchAfter "
        "pagination. Timings include transport on both sides, which is the cost of "
        "obtaining the code set rather than of evaluation alone."
    ),
    "rungs": [],
}

command = [
    "docker", "run", "--rm", "-i", "--name", "snomed-ecl-ladder",
    "--cpus", "1", "--memory", f"{args.memory_mib}m", "--memory-swap", f"{args.memory_mib}m",
    "--mount", f"type=bind,source={ROOT},target=/work,readonly", "-w", "/work",
    IMAGE, args.binary, "batch", store_argument,
]
process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)


def engine(ecl):
    """One complete expansion through the persistent engine process."""
    process.stdin.write(json.dumps({"ecl": ecl, "count_only": False}) + "\n")
    process.stdin.flush()
    line = process.stdout.readline()
    if not line:
        # Surface the container's own error rather than the symptom.
        process.stdin.close()
        stderr = process.stderr.read()[:2000] if process.stderr else ""
        raise RuntimeError(f"Engine process exited without a response. stderr: {stderr}")
    result = json.loads(line)
    if "error" in result:
        raise RuntimeError(f"Engine refused {ecl}: {result}")
    if result["edition"] != manifest["edition"]:
        raise ValueError("Wrong engine edition")
    return set(result["codes"])


try:
    for case in ladder["cases"]:
        ecl = case["ecl"]
        rung = {"ecl": ecl, "expected_total": case["expected_total"],
                "engine_ms": [], "snowstorm_ms": []}
        for attempt in range(args.repeats):
            start = time.perf_counter()
            ours = engine(ecl)
            rung["engine_ms"].append((time.perf_counter() - start) * 1000)

            start = time.perf_counter()
            theirs = snowstorm(args.snowstorm, ecl)
            rung["snowstorm_ms"].append((time.perf_counter() - start) * 1000)

            if attempt == 0:
                rung.update(total=len(ours), sha256=digest(ours),
                            matches=ours == theirs,
                            only_engine=len(ours - theirs),
                            only_snowstorm=len(theirs - ours))
                if not rung["matches"]:
                    raise ValueError(f"Code sets differ for {ecl}; not a timing comparison")
            elif len(ours) != rung["total"] or len(theirs) != rung["total"]:
                raise ValueError(f"Unstable result for {ecl}")
        # The first request for an expression is cold. Snowstorm caches ECL
        # results, so later requests measure a different thing entirely and are
        # never merged with the first.
        for side in ("engine", "snowstorm"):
            samples = rung[f"{side}_ms"]
            rung[f"{side}_cold_ms"] = samples[0]
            rung[f"{side}_warm_ms"] = (statistics.median(samples[1:])
                                       if len(samples) > 1 else None)
        rung["cold_ratio"] = rung["snowstorm_cold_ms"] / rung["engine_cold_ms"]
        if rung["snowstorm_warm_ms"] and rung["engine_warm_ms"]:
            rung["warm_ratio"] = rung["snowstorm_warm_ms"] / rung["engine_warm_ms"]
        report["rungs"].append(rung)
        print(json.dumps({"total": rung["total"],
                          "engine_cold_ms": round(rung["engine_cold_ms"], 1),
                          "snowstorm_cold_ms": round(rung["snowstorm_cold_ms"], 1),
                          "cold_ratio": round(rung["cold_ratio"]),
                          "snowstorm_warm_ms": round(rung["snowstorm_warm_ms"] or 0, 1)}),
              flush=True)
        args.output.write_text(json.dumps(report, indent=1), encoding="utf-8")
finally:
    process.stdin.close()
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        process.kill()

# The engine container is gone by now; the servers are still up.
report["snowstorm_resources"] = resource_snapshot("snomed-ecl-snowstorm")
report["elasticsearch_resources"] = resource_snapshot("snomed-ecl-elasticsearch")
if http(args.snowstorm, "/branches/MAIN", {}) != branch_before:
    raise ValueError("Snowstorm branch changed during the run")
args.output.write_text(json.dumps(report, indent=1), encoding="utf-8")
print(json.dumps({"rungs": len(report["rungs"]), "output": str(args.output)}))
