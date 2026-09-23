"""Helpers shared by the benchmark and RF2 check scripts."""
import hashlib
import json
import math
from pathlib import Path
import statistics
import subprocess
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
IMAGE = "rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31"


def digest(codes):
    return hashlib.sha256("".join(f"{code}\n" for code in sorted(codes, key=int)).encode()).hexdigest()


def http(base, path, params):
    with urllib.request.urlopen(base + path + "?" + urllib.parse.urlencode(params), timeout=120) as response:
        return json.load(response)


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
