# CPU and concurrency scaling

Four CPUs and four workers completed the 10,000-expression batch in 5.87 seconds,
compared with 23.60 seconds for one CPU and one worker. Throughput increased
about fourfold, from 424 to 1,703 expressions per second. All complete result
sets matched in every configuration.

The scaling benchmark runs the same 10,000 expressions against one shared
`NumericStore`. Each worker parses and evaluates one expression at a time.
The index is loaded once, and workers take expressions from a shared queue.
There is no result cache. The CLI's JSONL loop remains sequential.

## Results

The [recorded results](../validation/ecl-10000-scaling-results.json) use the
complete current UK packed index and Unicode-enabled library. Batch times are
medians of five runs. Request statistics cover 50,000 evaluations per configuration.

| CPU / RAM limit | Workers | 10,000-query batch | Median query | Query p95 | Charged peak |
|---|---:|---:|---:|---:|---:|
| 1 CPU / 512 MiB | 1 | 23.60 s | 0.92 ms | 10.01 ms | 283.89 MiB |
| 4 CPUs / 1 GiB | 1 | 23.16 s | 0.96 ms | 9.94 ms | 274.19 MiB |
| 2 CPUs / 512 MiB | 2 | 12.30 s | 1.02 ms | 10.72 ms | 285.49 MiB |
| 4 CPUs / 1 GiB | 4 | 5.87 s | 0.95 ms | 9.46 ms | 297.33 MiB |

Two workers achieved 1.92 times the baseline throughput; four achieved 4.02
times. The slight excess over fourfold is within run variation. Four CPUs with
one worker improved batch time by only 1.9%, also within run variation. The
gain comes from evaluating separate expressions concurrently. Individual
expressions still run sequentially.

Sharing the index kept the four-worker peak only 13.44 MiB above the one-worker
baseline. These peaks are observations, not minimum memory limits. The worker
example demonstrates library concurrency; a consuming application must provide
its own bounded worker pool. This change adds no parallel mode to the CLI.

## Measurement conditions

Each configuration runs in a separate container, one after another. CPU and
memory limits apply to the whole container, with no additional swap. The host
has an AMD Ryzen 9 5900X with 12 physical cores and 24 logical processors.
Docker Desktop uses WSL2. Filesystem caches remain warm on the Windows bind
mount. The host is not exclusively reserved, but no other task containers run
during the measurements.

Before timing, every configuration checks all 10,000 complete result sets
against the pinned regression digests. Five shuffled batches then evaluate
every expression exactly once per batch. Worker counts use the same shuffle
seeds. Results still materialise in full, although timed requests do not
convert ordinals to SCTIDs or serialise code lists.

Request latency includes parsing and evaluation. It excludes time waiting in
the batch queue, transport and result destruction. Batch time includes work
assignment, worker creation, collection and destruction. These are library
measurements and must not be compared directly with the earlier JSONL request
times. The peak memory measurement includes verification, stored timing samples
and container-charged OS cache through the final timed batch. It excludes
subsequent report serialisation and host controller memory.

The first version of this probe exited with code 137 at 256 MiB before finishing
verification. A diagnostic run passed at 512 MiB with a 276.07 MiB charged peak.
The measured baseline therefore has a 512 MiB limit. The earlier CLI corpus
passed at 256 MiB; the probe retains corpus records, expected digests and timings
in the measured process and uses worker threads. This experiment does not
establish the minimum memory for an application or every ECL query.

## Reproduce

Build the example with the same Unicode support as the corpus executable:

```sh
CARGO_TARGET_DIR=target/linux-unicode cargo build --locked --release --features unicode --example benchmark_scaling
python scripts/benchmark_scaling.py --output validation/new-scaling-results.json
```

Run the build in the pinned Rust Linux image with ICU4C development libraries
available. The Python runner starts that image with the recorded CPU and RAM
limits. It requires the licensed packed index and the pinned corpus baseline.
It records executable, source, index, corpus and baseline checksums. Detailed
timing samples remain under ignored `data/validation/`.

Use repeated `--configuration CPUS,MEMORY_MIB,WORKERS` arguments to change the
matrix. The default is one worker at one CPU and 512 MiB, one worker at four
CPUs and 1 GiB, two workers at two CPUs and 512 MiB, and four workers at four
CPUs and 1 GiB. Changing the number of workers changes concurrency between
expressions. It does not parallelise an individual expression.
