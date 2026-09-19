"""Measure one command in a fresh Linux container; keep generated reports in data/."""

import argparse
import json
from pathlib import Path
import resource
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--stdout", required=True, type=Path)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not args.command or args.report.exists() or args.stdout.exists():
        parser.error("Supply a command and new output paths")
    start = time.perf_counter()
    with args.stdout.open("x", encoding="utf-8") as output:
        result = subprocess.run(args.command, stdout=output, check=False)
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    peak_file = next((path for path in (
        Path("/sys/fs/cgroup/memory.peak"),
        Path("/sys/fs/cgroup/memory/memory.max_usage_in_bytes"),
    ) if path.exists()), None)
    report = {
        "exit_code": result.returncode,
        "elapsed_seconds": time.perf_counter() - start,
        "child_peak_rss_kib": usage.ru_maxrss,
        "child_user_seconds": usage.ru_utime,
        "child_system_seconds": usage.ru_stime,
        "container_peak_bytes": int(peak_file.read_text()) if peak_file else None,
        "scope": "Linux child RSS excludes page cache. Container peak includes the measurement wrapper and charged filesystem cache. Use a fresh container for each run.",
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report), flush=True)
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
