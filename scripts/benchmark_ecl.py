"""Compare complete basic-ECL sets using a persistent Rust process and local Snowstorm Lite."""
import argparse
import datetime
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess
import time
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
IMAGE = "rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31"


def digest(codes):
    return hashlib.sha256("".join(f"{code}\n" for code in sorted(codes, key=int)).encode()).hexdigest()


def http(base, path, params):
    with urllib.request.urlopen(base + path + "?" + urllib.parse.urlencode(params), timeout=120) as response:
        return json.load(response)


def snowstorm(base, edition, ecl, page_size):
    codes = set()
    offset = 0
    expected_total = None
    while True:
        expansion = http(base, "/ValueSet/$expand", {
            "url": edition + "?fhir_vs=ecl/" + ecl, "count": page_size, "offset": offset,
            "includeDesignations": "false",
        })["expansion"]
        total = expansion["total"]
        if expected_total is not None and expected_total != total:
            raise ValueError("Total changed between pages")
        expected_total = total
        page = expansion.get("contains", [])
        for entry in page:
            if entry.get("contains") or entry.get("system") != "http://snomed.info/sct" or entry.get("version", edition) != edition:
                raise ValueError("Unexpected expansion system, version or nesting")
            codes.add(entry["code"])
        offset += len(page)
        if offset >= total:
            break
        if not page or offset > 2_000_000:
            raise ValueError("Incomplete or unexpectedly large expansion")
    if len(codes) != total or offset != total:
        raise ValueError("Duplicate or missing codes across pages")
    return codes


def resource_snapshot(container):
    command = ["docker", "exec", container, "sh", "-c",
               "cat /sys/fs/cgroup/memory.peak 2>/dev/null || cat /sys/fs/cgroup/memory/memory.max_usage_in_bytes"]
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    inspect = json.loads(subprocess.check_output(["docker", "inspect", container], text=True))[0]
    return {"container_charged_peak_bytes": int(result.stdout.strip()) if result.returncode == 0 else None,
            "peak_scope": "Container lifetime; includes any earlier queries since this container was created",
            "memory_limit_bytes": inspect["HostConfig"]["Memory"], "memory_and_swap_limit_bytes": inspect["HostConfig"]["MemorySwap"],
            "nano_cpus": inspect["HostConfig"]["NanoCpus"], "image": inspect["Config"]["Image"]}


def summary(samples):
    return {"samples_ms": samples, "median_ms": statistics.median(samples), "p95_ms": sorted(samples)[math.ceil(len(samples) * 0.95) - 1]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="http://127.0.0.1:18081/fhir")
    parser.add_argument("--snowstorm-container", default="snomed-ecl-serving")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--page-size", type=int, default=50000)
    parser.add_argument("--large-samples", type=int, default=1)
    parser.add_argument("--output", type=Path, default=ROOT / "data/validation/basic-ecl-benchmark.json")
    args = parser.parse_args()
    if urllib.parse.urlparse(args.base).hostname not in ("127.0.0.1", "localhost", "::1"):
        parser.error("Only loopback comparison servers are permitted")
    if args.samples < 1 or args.large_samples < 1 or args.page_size < 1 or args.output.exists():
        parser.error("Choose positive sample/page counts and a new output file")
    baseline = json.loads((ROOT / "validation/ontoserver-basic-ecl.json").read_text(encoding="utf-8-sig"))
    edition = baseline["edition"]
    expected = {row["id"]: row for row in baseline["results"]}
    cases = json.loads((ROOT / "validation/basic-ecl-queries.json").read_text())
    systems = http(args.base, "/CodeSystem", {"url": "http://snomed.info/sct", "_count": 100})
    if any(link.get("relation") == "next" for link in systems.get("link", [])):
        raise ValueError("Paginated CodeSystem discovery needs explicit handling")
    if edition not in [entry["resource"].get("version") for entry in systems.get("entry", [])]:
        raise ValueError("Comparison server does not advertise pinned edition")
    command = ["docker", "run", "--rm", "-i", "--name", "snomed-ecl-query", "--cpus", "1", "--memory", "256m", "--memory-swap", "256m",
               "--mount", f"type=bind,source={ROOT},target=/work", "-w", "/work", IMAGE,
               "target/linux/release/snomed-ecl-engine", "batch", "data/compact-store/v1"]
    start = time.perf_counter()
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)

    def rust(ecl, count_only=False):
        process.stdin.write(json.dumps({"ecl": ecl, "count_only": count_only}) + "\n")
        process.stdin.flush()
        line = process.stdout.readline()
        if not line:
            raise RuntimeError("Rust process exited without a response")
        result = json.loads(line)
        if "error" in result or result.get("edition") != edition:
            raise ValueError("Rust query failed or returned wrong edition")
        if not count_only and (len(result["codes"]) != result["total"] or len(set(result["codes"])) != result["total"]):
            raise ValueError("Rust codes are duplicate or incomplete")
        return result

    results = []
    report = {"edition": edition, "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "rust_binary_sha256": hashlib.sha256((ROOT / "target/linux/release/snomed-ecl-engine").read_bytes()).hexdigest(),
              "samples": args.samples, "large_samples": args.large_samples, "page_size": args.page_size,
              "scope": "Complete code sets checked first, then alternating warm transport measurements. Rust uses JSONL through docker stdin/stdout; Snowstorm Lite uses paginated FHIR HTTP and also materialises displays. Transport timings are not an isolated engine speed comparison. Rust eval_ms excludes parsing, transport and display lookup. Small sample p95 is descriptive only. Host controller memory and Docker VM overhead are excluded from cgroup figures.",
              "results": results}
    try:
        rust("195967001", count_only=True)
        report["rust_container_start_and_first_request_ms"] = (time.perf_counter() - start) * 1000
        for case in cases:
            row = {"id": case["id"], "ecl": case["ecl"]}
            results.append(row)
            try:
                result = rust(case["ecl"])
                codes = set(result["codes"])
                signature = digest(codes)
                row.update(total=len(codes), sha256=signature)
                # Different totals already prove a mismatch. Do not enumerate or time it as a match.
                preflight = http(args.base, "/ValueSet/$expand", {
                    "url": edition + "?fhir_vs=ecl/" + case["ecl"], "count": 0,
                })["expansion"]["total"]
                row["snowstorm_total"] = preflight
                if preflight != len(codes):
                    row.update(matches_snowstorm=False, comparison="Count mismatch; no speed comparison or claim of complete-set agreement")
                    raise ValueError("Result totals differ")
                other = snowstorm(args.base, edition, case["ecl"], args.page_size)
                row.update(total=len(codes), sha256=signature, matches_snowstorm=codes == other,
                           only_rust=len(codes - other), only_snowstorm=len(other - codes))
                if case["id"] in expected:
                    reference = expected[case["id"]]
                    row["matches_ontoserver"] = reference["complete"] and len(codes) == reference["total"] and signature == reference["sha256"]
                if not row["matches_snowstorm"] or not row.get("matches_ontoserver", True):
                    raise ValueError("Complete result sets differ")
                rust_samples, snow_samples, eval_samples, parse_samples = [], [], [], []
                sample_count = args.samples if case.get("probe") else args.large_samples
                row["sample_count"] = sample_count
                for iteration in range(sample_count):
                    for engine in (("rust", "snowstorm") if iteration % 2 == 0 else ("snowstorm", "rust")):
                        start = time.perf_counter()
                        if engine == "rust":
                            observed = rust(case["ecl"])
                            rust_samples.append((time.perf_counter() - start) * 1000)
                            eval_samples.append(observed["eval_ms"])
                            parse_samples.append(observed["parse_ms"])
                            observed_codes = set(observed["codes"])
                        else:
                            observed_codes = snowstorm(args.base, edition, case["ecl"], args.page_size)
                            snow_samples.append((time.perf_counter() - start) * 1000)
                        if observed_codes != codes:
                            raise ValueError("Result changed during timing")
                row.update(rust_transport=summary(rust_samples), snowstorm_http=summary(snow_samples), rust_evaluation=summary(eval_samples), rust_parse=summary(parse_samples))
            except Exception as error:
                row["error"] = type(error).__name__ + ": " + str(error)
            print(json.dumps({"id": case["id"], "total": row.get("total"), "matched": row.get("matches_snowstorm"), "error": row.get("error")}), flush=True)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        report["rust_resources"] = resource_snapshot("snomed-ecl-query")
        report["snowstorm_resources"] = resource_snapshot(args.snowstorm_container)
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            subprocess.run(["docker", "stop", "snomed-ecl-query"], check=False, capture_output=True)
            process.wait(timeout=15)
        report["rust_exit_code"] = process.returncode
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if process.returncode != 0 or len(results) != len(cases) or any("error" in row for row in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
