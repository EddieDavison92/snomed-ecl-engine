"""Independently check broad basic ECL sets against active RF2 rows, without reading core.bin."""
import argparse
import collections
import csv
import hashlib
import io
import json
from pathlib import Path
import subprocess
import time
import zipfile

from benchmark_ecl import IMAGE, ROOT, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "data/validation/basic-ecl-rf2.json")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a new output path")
    release = json.loads((ROOT / "docs/release.json").read_text(encoding="utf-8-sig"))
    path = ROOT / "data/rf2" / release["archiveFileName"]
    start = time.perf_counter()
    with path.open("rb") as source:
        archive_hash = hashlib.file_digest(source, "sha256").hexdigest()
    if archive_hash.lower() != release["sha256"].lower():
        raise ValueError("RF2 checksum differs")
    with zipfile.ZipFile(path) as archive:
        def rows(prefix):
            names = [name for name in archive.namelist() if "/Snapshot/" in name and Path(name).name.startswith(prefix)]
            if len(names) != 1:
                raise ValueError("Expected one component Snapshot")
            with archive.open(names[0]) as member:
                yield from csv.DictReader(io.TextIOWrapper(member, encoding="utf-8-sig"), delimiter="\t", quoting=csv.QUOTE_NONE)

        active = {int(row["id"]) for row in rows("sct2_Concept_") if row["active"] == "1"}
        print(json.dumps({"active_concepts": len(active)}), flush=True)
        children = collections.defaultdict(list)
        sources, destinations = set(), set()
        for row in rows("sct2_Relationship_"):
            if row["active"] == "1" and row["characteristicTypeId"] == "900000000000011006" and row["typeId"] == "116680003":
                source, target = int(row["sourceId"]), int(row["destinationId"])
                if source not in active or target not in active:
                    raise ValueError("Inactive inferred hierarchy endpoint")
                children[target].append(source)
                sources.add(source)
                destinations.add(target)
        finding = {404684003}
        pending = [404684003]
        while pending:
            for child in children.get(pending.pop(), []):
                if child not in finding:
                    finding.add(child)
                    pending.append(child)
    expected = {
        "large-result": finding,
        "wildcard": active,
        "wildcard-descendants": sources,
        "wildcard-ancestors": destinations,
        "large-exclusion": active - finding,
    }
    expected = {key: {str(code) for code in values} for key, values in expected.items()}
    del children, active, finding, sources, destinations
    command = ["docker", "run", "--rm", "-i", "--name", "snomed-ecl-rf2-check", "--cpus", "1", "--memory", "256m", "--memory-swap", "256m",
               "--mount", f"type=bind,source={ROOT},target=/work", "-w", "/work", IMAGE,
               "target/linux/release/snomed-rust-ecl-engine", "batch", "data/compact-store/v1"]
    cases = json.loads((ROOT / "validation/basic-ecl-queries.json").read_text())
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
    results = []
    try:
        for case in cases:
            if case["id"] not in expected:
                continue
            process.stdin.write(json.dumps({"ecl": case["ecl"]}) + "\n")
            process.stdin.flush()
            observed = json.loads(process.stdout.readline())
            if "error" in observed:
                raise ValueError("Rust query failed")
            wanted = expected[case["id"]]
            codes = set(observed["codes"])
            results.append({"id": case["id"], "ecl": case["ecl"], "total": len(codes),
                            "sha256": digest(codes), "matches_rf2": codes == wanted and len(codes) == observed["total"],
                            "only_rust": len(codes - wanted), "only_rf2": len(wanted - codes),
                            "rust_eval_ms": observed["eval_ms"]})
            print(json.dumps(results[-1]), flush=True)
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            subprocess.run(["docker", "stop", "snomed-ecl-rf2-check"], check=False, capture_output=True)
            process.wait(timeout=15)
    report = {"archive_sha256": archive_hash, "elapsed_seconds": time.perf_counter() - start,
              "scope": "Exact set differences against a separate Python RF2 reader. Wildcard uses active concept rows; descendant/ancestor wildcard uses inferred is-a source/destination sets; finding uses an independent graph traversal. Timings are single observations, not a latency benchmark.", "results": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if process.returncode != 0 or len(results) != len(expected) or any(not row["matches_rf2"] for row in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
