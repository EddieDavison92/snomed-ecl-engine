"""Measure the fixed corpus, retaining unsupported cases and comparing full sets when requested."""
import argparse
import collections
import datetime
import gzip
import hashlib
import json
from pathlib import Path
import random
import subprocess
import time
import urllib.parse
import urllib.error

from benchmark_ecl import ROOT, IMAGE, digest, http, resource_snapshot, summary


def known_language_rejection(ecl, status, body):
    # This pinned Snowstorm parser rejects ECL 2.3 extrema at the first '!'.
    # Do not treat arbitrary HTTP 400s or wrong result sets as unsupported.
    return (status == 400 and ecl.lstrip().startswith(("!!>", "!!<"))
            and "mismatched input '!'" in body and "Syntax error" in body)


def snowstorm(base, ecl):
    codes, cursor, total = set(), None, None
    while True:
        params = {"ecl": ecl, "returnIdOnly": "true", "limit": 10000}
        if cursor:
            params["searchAfter"] = cursor
        page = http(base, "/MAIN/concepts", params)
        if total is not None and page["total"] != total:
            raise ValueError("Snowstorm total changed between pages")
        total = page["total"]
        entries = page["items"]
        if len(set(entries)) != len(entries) or codes.intersection(entries):
            raise ValueError("Snowstorm returned duplicate codes")
        codes.update(entries)
        if len(codes) == total:
            return codes
        next_cursor = page.get("searchAfter")
        if not entries or not next_cursor or next_cursor == cursor or len(codes) > total:
            raise ValueError("Snowstorm pagination incomplete")
        cursor = next_cursor


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--binary", default="target/linux-core/release/snomed-rust-ecl-engine")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--memory-mib", type=int, default=256, help="Container memory and swap limit; record larger semantic-index runs separately")
    parser.add_argument("--store-volume", help="Optional Docker volume holding core.bin and manifest.json")
    parser.add_argument("--store-directory", type=Path, default=Path("data/compact-store/v1"), help="Store within this checkout; also supplies the manifest when using a Docker volume")
    parser.add_argument("--snowstorm", help="Optional loopback full Snowstorm URL; requires a completed, release-matched MAIN import")
    parser.add_argument("--import-id", help="Completed local Snowstorm import job ID")
    parser.add_argument("--import-report", type=Path, help="Prior report with completed import evidence and the unchanged MAIN head, for a serving-only restart")
    parser.add_argument("--timeout-seconds", type=int, default=3600)
    args = parser.parse_args()
    if args.output.exists() or args.samples < 1 or args.timeout_seconds < 1 or args.memory_mib < 1:
        parser.error("Choose a new report path and positive sample count")
    if args.snowstorm and urllib.parse.urlparse(args.snowstorm).hostname not in ("127.0.0.1", "localhost", "::1"):
        parser.error("Only local comparison servers are allowed")
    corpus_path = ROOT / "validation/ecl-1000.json"
    corpus = json.loads(corpus_path.read_text())
    store_directory = (ROOT / args.store_directory).resolve()
    try:
        store = store_directory.relative_to(ROOT).as_posix()
    except ValueError:
        parser.error("Store directory must be within the mounted checkout")
    manifest = json.loads((store_directory / "manifest.json").read_text())
    supplements = [s["archive_sha256"] for s in manifest.get("supplements", [])]
    if args.snowstorm and supplements:
        parser.error("Snowstorm comparison cannot yet verify supplementary packages; use the base store")
    assert corpus["archive_sha256"].lower() == manifest["archive_sha256"].lower()
    binary = (ROOT / args.binary).read_bytes()
    rows = {case["id"]: dict(case) for case in corpus["cases"]}
    report = {"recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "edition": manifest["edition"], "archive_sha256": corpus["archive_sha256"],
              "corpus_sha256": hashlib.sha256(corpus_path.read_bytes()).hexdigest(),
              "binary_sha256": hashlib.sha256(binary).hexdigest(), "binary_bytes": len(binary),
              "memory_limit_mib": args.memory_mib,
              "binary_gzip_bytes": len(gzip.compress(binary, mtime=0)),
              "core_index_bytes": manifest["core_bytes"], "samples": args.samples,
              "membership_index_bytes": (manifest.get("membership") or {}).get("bytes", 0),
              "description_index_bytes": (manifest.get("descriptions") or {}).get("bytes", 0),
              "supplements": supplements,
              "core_sha256": manifest["core_sha256"],
              "membership_sha256": (manifest.get("membership") or {}).get("sha256"),
              "index_filesystem": "Docker volume" if args.store_volume else "Windows bind mount",
              "scope": f"One CPU, {args.memory_mib} MiB, persistent Rust process. No result cache. Each measured count request evaluates and materialises the full ordinal set. Five seeded shuffled batches by default; p95 describes this corpus only. Complete enumeration is checked once outside warm count timings. A local-file startup does not measure object-storage download, provider cold start or full-ECL index costs.",
              "results": list(rows.values())}
    if args.snowstorm:
        if bool(args.import_id) == bool(args.import_report):
            parser.error("--snowstorm requires exactly one of --import-id or --import-report")
        prior = None
        if args.import_report:
            evidence = args.import_report.read_bytes()
            prior = json.loads(evidence)
            if (prior["edition"] != manifest["edition"]
                    or prior["archive_sha256"].lower() != manifest["archive_sha256"].lower()):
                raise ValueError("Prior import report is for a different release")
            imports = prior["snowstorm_imports"]
            report["import_evidence_sha256"] = hashlib.sha256(evidence).hexdigest()
        else:
            imports = http(args.snowstorm, "/imports/" + urllib.parse.quote(args.import_id, safe=""), {})
        # Require explicit evidence that a snapshot import completed before using MAIN.
        if imports.get("status") != "COMPLETED" or imports.get("branchPath") != "MAIN" or imports.get("type") != "SNAPSHOT":
            raise ValueError("No completed MAIN import reported")
        report["snowstorm_imports"] = imports
        systems = http(args.snowstorm, "/fhir/CodeSystem", {"url": "http://snomed.info/sct", "_count": 100})
        if any(link.get("relation") == "next" for link in systems.get("link", [])) or manifest["edition"] not in [entry["resource"].get("version") for entry in systems.get("entry", [])]:
            raise ValueError("Snowstorm does not advertise the pinned edition")
        report["snowstorm_branch_before"] = http(args.snowstorm, "/branches/MAIN", {})
        if prior and report["snowstorm_branch_before"] != prior["snowstorm_branch_before"]:
            raise ValueError("MAIN differs from the branch in the completed import report")
        # CLI setup pins the archive. These sentinels additionally reject a wrong or partial release.
        for ecl, expected in [("*", manifest["active_concept_count"]), ("<< 404684003", 137834)]:
            if http(args.snowstorm, "/MAIN/concepts", {"ecl": ecl, "returnIdOnly": "true", "limit": 1})["total"] != expected:
                raise ValueError("Snowstorm release sentinel differs")
    command = ["docker", "run", "--rm", "-i", "--name", "snomed-ecl-corpus", "--cpus", "1", "--memory", f"{args.memory_mib}m", "--memory-swap", f"{args.memory_mib}m", "--mount", f"type=bind,source={ROOT},target=/work,readonly", "-w", "/work"]
    if args.store_volume:
        if any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-" for c in args.store_volume):
            parser.error("Invalid Docker volume name")
        command += ["--mount", f"type=volume,source={args.store_volume},target=/index,readonly"]
        store = "/index"
    command += [IMAGE, args.binary, "batch", store]
    start = time.perf_counter()
    deadline = start + args.timeout_seconds
    comparison_errors = 0
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)

    def query(ecl, count=True):
        if time.perf_counter() >= deadline:
            raise TimeoutError("Corpus run exceeded its time budget")
        process.stdin.write(json.dumps({"ecl": ecl, "count_only": count}) + "\n")
        process.stdin.flush()
        line = process.stdout.readline()
        if not line:
            raise RuntimeError("Rust process exited without a response")
        result = json.loads(line)
        if "error" not in result and result["edition"] != manifest["edition"]:
            raise ValueError("Wrong Rust edition")
        if "error" not in result and result.get("supplements", []) != supplements:
            raise ValueError("Wrong Rust supplements")
        return result

    def save():
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    try:
        assert query("195967001")["total"] == 1
        report["container_start_and_first_request_ms"] = (time.perf_counter() - start) * 1000
        for index, row in enumerate(rows.values()):
            observed = query(row["ecl"], False)
            if "error" in observed:
                row.update(status="unsupported" if "Unsupported" in observed["error"] or "Unsupported" in observed.get("message", "") else "error", error=observed)
                continue
            codes = set(observed["codes"])
            assert len(codes) == len(observed["codes"]) == observed["total"]
            row.update(status="evaluated", total=len(codes), sha256=digest(codes), evaluation_samples_ms=[], parse_samples_ms=[], rust_request_samples_ms=[], snowstorm_count_samples_ms=[])
            if args.snowstorm:
                try:
                    start = time.perf_counter()
                    other = snowstorm(args.snowstorm, row["ecl"])
                    row.update(snowstorm_enumeration_ms=(time.perf_counter() - start) * 1000, matches_snowstorm=codes == other, only_rust=len(codes - other), only_snowstorm=len(other - codes))
                    if codes != other:
                        row["status"] = "mismatch"
                        row["only_rust_codes"] = sorted(codes - other, key=int)
                        row["only_snowstorm_codes"] = sorted(other - codes, key=int)
                except Exception as error:
                    body = error.read(65536).decode("utf-8", errors="replace") if isinstance(error, urllib.error.HTTPError) else ""
                    status = getattr(error, "code", None)
                    if known_language_rejection(row["ecl"], status, body):
                        row.update(status="snowstorm-unsupported", comparison_http_status=status, comparison_error_body=body)
                    else:
                        row.update(status="comparison-error", comparison_error=str(error), comparison_http_status=status, comparison_error_body=body)
                        comparison_errors += 1
                        if comparison_errors >= 5:
                            raise RuntimeError("Stopped after five comparison errors") from error
            if index % 100 == 0:
                print(json.dumps({"enumerated": index + 1}), flush=True)
                save()
        successful = [r for r in rows.values() if r["status"] in ("evaluated", "snowstorm-unsupported")]
        batches = []
        for iteration in range(args.samples):
            order = successful.copy()
            random.Random(20260826 + iteration).shuffle(order)
            start = time.perf_counter()
            for row in order:
                request_start = time.perf_counter()
                result = query(row["ecl"])
                row["rust_request_samples_ms"].append((time.perf_counter() - request_start) * 1000)
                if result.get("total") != row["total"]:
                    raise ValueError(f"Unstable result: {row['id']}")
                row["evaluation_samples_ms"].append(result["eval_ms"])
                row["parse_samples_ms"].append(result["parse_ms"])
                if args.snowstorm and row.get("matches_snowstorm"):
                    snow_start = time.perf_counter()
                    other = http(args.snowstorm, "/MAIN/concepts", {"ecl": row["ecl"], "returnIdOnly": "true", "limit": 1})
                    row["snowstorm_count_samples_ms"].append((time.perf_counter() - snow_start) * 1000)
                    if other["total"] != row["total"]:
                        raise ValueError(f"Unstable Snowstorm result: {row['id']}")
            batches.append((time.perf_counter() - start) * 1000)
            print(json.dumps({"batch": iteration + 1, "expressions": len(successful), "wall_ms": batches[-1]}), flush=True)
            save()
        report["warm_batch_wall"] = summary(batches)
        report["status_counts"] = dict(collections.Counter(r["status"] for r in rows.values()))
        report["categories"] = {}
        for category in sorted({r["category"] for r in rows.values()}):
            group = [r for r in rows.values() if r["category"] == category]
            samples = [v for r in group for v in r.get("evaluation_samples_ms", [])]
            report["categories"][category] = {"status_counts": dict(collections.Counter(r["status"] for r in group)), "evaluation": summary(samples) if samples else None}
            other_samples = [v for r in group for v in r.get("snowstorm_count_samples_ms", [])]
            if other_samples:
                report["categories"][category]["snowstorm_count_http"] = summary(other_samples)
        report["warm_batch_engine_ms"] = [sum(r["evaluation_samples_ms"][i] for r in successful) for i in range(args.samples)]
        report["resources"] = resource_snapshot("snomed-ecl-corpus")
        if args.snowstorm:
            paired = [r for r in successful if r.get("matches_snowstorm")]
            report["paired_expressions"] = len(paired)
            report["paired_rust_engine_batch_ms"] = [sum(r["evaluation_samples_ms"][i] for r in paired) for i in range(args.samples)]
            report["paired_rust_request_batch_ms"] = [sum(r["rust_request_samples_ms"][i] for r in paired) for i in range(args.samples)]
            report["paired_snowstorm_http_batch_ms"] = [sum(r["snowstorm_count_samples_ms"][i] for r in paired) for i in range(args.samples)]
            report["comparison_scope"] = "Only expressions with matching complete code sets receive paired timings. Snowstorm-unsupported cases retain Rust-only timings. Snowstorm warm count HTTP requests return total plus at most one ID, use its default caches, and include HTTP overhead. Rust evaluates complete ordinal sets without a result cache; eval_ms excludes JSONL transport, rust_request_samples_ms includes it. Batch wall time includes both engines. This is not isolated equal-cache engine timing."
            report["snowstorm_resources"] = resource_snapshot("snomed-ecl-snowstorm")
            report["elasticsearch_resources"] = resource_snapshot("snomed-ecl-elasticsearch")
            if http(args.snowstorm, "/branches/MAIN", {}) != report["snowstorm_branch_before"]:
                raise ValueError("Snowstorm branch changed during validation")
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            subprocess.run(["docker", "stop", "snomed-ecl-corpus"], capture_output=True, check=False)
            process.wait(timeout=15)
        report["startup_log"] = process.stderr.read().strip()
        report["exit_code"] = process.returncode
        save()
    print(json.dumps(report.get("status_counts", {})))
    if process.returncode or any(r["status"] not in ("evaluated", "unsupported", "snowstorm-unsupported") for r in rows.values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
