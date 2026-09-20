"""Compare complete membership results with an independent RF2 row scan."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import zipfile

from benchmark_ecl import ROOT, IMAGE, digest


def rows(archive, name):
    with archive.open(name) as file:
        lines = io.TextIOWrapper(file, encoding="utf-8-sig")
        header = next(lines).rstrip("\r\n").split("\t")
        for line in lines:
            yield dict(zip(header, line.rstrip("\r\n").split("\t"), strict=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--store", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--binary", default="target/linux-core/release/snomed-rust-ecl-engine")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a new output path")
    manifest = json.loads((args.store / "manifest.json").read_text())
    with args.archive.open("rb") as file:
        if hashlib.file_digest(file, "sha256").hexdigest() != manifest["archive_sha256"].lower():
            raise ValueError("Archive checksum differs from store")
    refsets = {"723562003", "51971000001109", "999002271000000101", "900000000000508004"}
    reverse_concepts = {"195967001", "404684003"}
    expected = {"^" + refset: set() for refset in sorted(refsets)}
    expected.update({"^R" + concept: set() for concept in sorted(reverse_concepts)})
    scanned = 0
    with zipfile.ZipFile(args.archive) as archive:
        names = [name for name in archive.namelist() if "/Snapshot/" in name and name.endswith(".txt")]
        concepts = next(name for name in names if name.rsplit("/", 1)[-1].startswith("sct2_Concept_"))
        concept_ids = {row["id"] for row in rows(archive, concepts)}
        for name in names:
            if "Refset" not in name.rsplit("/", 1)[-1]:
                continue
            for row in rows(archive, name):
                scanned += 1
                if row["active"] != "1" or row["refsetId"] not in concept_ids:
                    continue
                refset, component = row["refsetId"], row["referencedComponentId"]
                if component not in concept_ids:
                    continue
                if refset in refsets:
                    expected["^" + refset].add(component)
                if component in reverse_concepts:
                    expected["^R" + component].add(refset)
    command = ["docker", "run", "--rm", "-i", "--cpus", "1", "--memory", "256m", "--memory-swap", "256m",
               "--mount", f"type=bind,source={ROOT},target=/work,readonly", "-w", "/work", IMAGE,
               args.binary, "batch", args.store.resolve().relative_to(ROOT).as_posix()]
    run = subprocess.run(command, input="".join(json.dumps({"ecl": ecl}) + "\n" for ecl in expected),
                         capture_output=True, text=True, encoding="utf-8", timeout=120, check=True)
    observed = [json.loads(line) for line in run.stdout.splitlines()]
    if len(observed) != len(expected):
        raise ValueError("Missing query responses")
    results = []
    for (ecl, codes), response in zip(expected.items(), observed, strict=True):
        if response.get("edition") != manifest["edition"] or "error" in response:
            raise ValueError("Failed or wrong-edition query")
        actual = set(response["codes"])
        results.append({"ecl": ecl, "expected_total": len(codes), "observed_total": response["total"],
                        "matches": actual == codes and len(actual) == len(response["codes"]) == response["total"],
                        "expected_sha256": digest(codes), "observed_sha256": digest(actual)})
    report = {"edition": manifest["edition"], "archive_sha256": manifest["archive_sha256"],
              "refset_rows_scanned": scanned, "scope": "Independent active RF2 member rows and all concept IDs; complete code sets", "results": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    if not all(row["matches"] for row in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
