"""Small local FHIR smoke benchmark, with complete-set checks against OneLondon."""

import argparse
import datetime
import hashlib
import json
import math
import pathlib
import statistics
import time
import urllib.error
import urllib.parse
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument("--base", default="http://127.0.0.1:18080/fhir")
parser.add_argument("--samples", type=int, default=20)
args = parser.parse_args()
if urllib.parse.urlparse(args.base).hostname not in ("127.0.0.1", "localhost", "::1"):
    parser.error("This smoke benchmark only permits loopback servers.")
if args.samples < 1:
    parser.error("--samples must be positive")
root = pathlib.Path(__file__).resolve().parent.parent
baseline = json.loads((root / "validation/ontoserver-baseline.json").read_text(encoding="utf-8-sig"))
version = baseline["edition"]


def get(path, params):
    url = args.base.rstrip("/") + path + "?" + urllib.parse.urlencode(params)
    with urllib.request.urlopen(url, timeout=60) as response:
        return json.load(response)


systems = get("/CodeSystem", {"url": "http://snomed.info/sct"})
if version not in [e["resource"].get("version") for e in systems.get("entry", [])]:
    raise SystemExit("Local server does not advertise the pinned edition.")


def expand(ecl):
    codes = set()
    offset = 0
    while True:
        result = get("/ValueSet/$expand", {
            "url": version + "?fhir_vs=ecl/" + ecl, "count": 500, "offset": offset
        })["expansion"]
        total = result["total"]
        page = result.get("contains", [])
        for entry in page:
            if entry.get("contains"):
                raise ValueError("Nested expansion is unsupported by the comparison script")
            if entry.get("system") != "http://snomed.info/sct":
                raise ValueError("Unexpected code system")
            if entry.get("version", version) != version:
                raise ValueError("Unexpected edition")
            codes.add(entry["code"])
        offset += len(page)
        if offset >= total:
            break
        if not page or offset > 100000:
            raise ValueError("Incomplete or unexpectedly large expansion")
    if len(codes) != total:
        raise ValueError("Duplicate codes or incomplete expansion")
    digest = hashlib.sha256(("\n".join(sorted(codes, key=int)) + "\n").encode()).hexdigest()
    return total, digest


results = []
for case in baseline["results"]:
    row = {"id": case["id"], "ecl": case["ecl"]}
    try:
        total, digest = expand(case["ecl"])
        row.update(total=total, sha256=digest, matches_ontoserver=digest == case["sha256"] and total == case["total"])
        if row["matches_ontoserver"]:
            for _ in range(3):
                expand(case["ecl"])
            samples = []
            for _ in range(args.samples):
                start = time.perf_counter()
                observed = expand(case["ecl"])
                samples.append((time.perf_counter() - start) * 1000)
                if observed != (total, digest):
                    raise ValueError("Result changed during timing")
            row.update(samples_ms=samples, median_ms=statistics.median(samples),
                       p95_ms=sorted(samples)[math.ceil(0.95 * len(samples)) - 1])
    except urllib.error.HTTPError as exc:
        row.update(error="HTTP " + str(exc.code))
    except (ValueError, KeyError, urllib.error.URLError, TimeoutError) as exc:
        row.update(error=type(exc).__name__)
    results.append(row)
report = {"checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
          "edition": version, "base": args.base, "samples_per_case": args.samples,
          "measurement": "Warm local HTTP plus JSON parsing, pagination, sorting and digest; new urllib connection per request; sequential case order; smoke only.",
          "results": results}
output = root / "data/validation/snowstorm-lite-smoke.json"
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"output": str(output), "cases": len(results),
                  "matched": sum(r.get("matches_ontoserver", False) for r in results),
                  "errors": sum("error" in r for r in results)}))
if any(not r.get("matches_ontoserver", False) or "error" in r for r in results):
    raise SystemExit(1)
