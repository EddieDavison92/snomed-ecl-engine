"""Check every simple supplement refset against its RF2 Snapshot rows."""
import argparse
import csv
import hashlib
import io
import json
from pathlib import Path
import subprocess
import zipfile

from benchmark_ecl import ROOT, IMAGE, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--store", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a new output path")
    manifest = json.loads((args.store / "manifest.json").read_text())
    with args.archive.open("rb") as source:
        archive_hash = hashlib.file_digest(source, "sha256").hexdigest()
    assert any(s["archive_sha256"] == archive_hash for s in manifest["supplements"])
    expected, scanned, active_rows = {}, 0, 0
    with zipfile.ZipFile(args.archive) as archive:
        for name in archive.namelist():
            if "/Snapshot/" not in name or not Path(name).name.startswith("der2_Refset_Simple"):
                continue
            with archive.open(name) as file:
                for row in csv.DictReader(io.TextIOWrapper(file, encoding="utf-8-sig"), delimiter="\t", quoting=csv.QUOTE_NONE):
                    scanned += 1
                    codes = expected.setdefault(row["refsetId"], set())
                    if row["active"] == "1":
                        codes.add(row["referencedComponentId"])
                        active_rows += 1
    assert expected
    expected = dict(sorted(expected.items(), key=lambda p: int(p[0])))
    command = ["docker", "run", "--rm", "-i", "--cpus", "1", "--memory", "256m", "--memory-swap", "256m",
               "--mount", f"type=bind,source={ROOT},target=/work,readonly", "-w", "/work", IMAGE,
               "target/linux-core/release/snomed-ecl-engine", "batch", args.store.resolve().relative_to(ROOT).as_posix()]
    run = subprocess.run(command, input="".join(json.dumps({"ecl": "^" + r}) + "\n" for r in expected),
                         capture_output=True, text=True, encoding="utf-8", timeout=180, check=True)
    responses = [json.loads(line) for line in run.stdout.splitlines()]
    results = []
    for (refset, wanted), response in zip(expected.items(), responses, strict=True):
        actual = set(response["codes"])
        results.append({"refset": refset, "total": response["total"], "sha256": digest(actual),
                        "matches": actual == wanted and response["total"] == len(actual) == len(response["codes"])
                        and response["edition"] == manifest["edition"] and archive_hash in response["supplements"]})
    report = {"edition": manifest["edition"], "supplement_sha256": archive_hash,
              "snapshot_rows": scanned, "active_rows": active_rows, "refsets": len(expected),
              "scope": "Complete code sets for every simple refset in the supplement; active member rows may reference inactive concepts.",
              "results": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in report.items() if k != "results"}))
    print(f"Complete matches: {sum(row['matches'] for row in results)}/{len(results)}")
    if not all(row["matches"] for row in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
