# Serverless query runtime

The engine must run without an always-on API, Redis or database server. Import and index construction run offline. A provider-specific HTTP wrapper can live in another repository and depend on this library.

## Deployment model

Publish immutable indexes under an edition and checksum. A new instance obtains the required index files from external object storage, verifies them and opens the store. Keep the opened store for subsequent requests while the instance stays warm. Displays remain a separate lookup after expansion.

Use local files for graph traversal. A remote object request for every relationship would replace inexpensive memory reads with network round trips. Future semantic indexes should have independent files so hierarchy queries do not have to acquire description text or history data. Queries that need those files must acquire them before returning a complete result.

The current reader loads the whole numeric store into memory. It does not yet download indexes, load semantic files on demand, or provide a cloud runtime. Consider memory mapping or bounded block reads only after the full semantic data layout exists and measurements show a benefit. Avoid a provider choice based on the incomplete core's footprint.

## Query-only build

```sh
cargo build --locked --release --no-default-features --bin snomed-rust-ecl-engine
```

The default `import` feature includes RF2 ZIP support. Excluding it removes the importer and decompression dependencies. Release builds strip symbols. A dependent service can also disable default features on the library dependency.

The query library has no HTTP framework, cloud SDK or asynchronous runtime dependency. The measured executable is a Linux x86-64 CLI linked against the standard platform libraries. Its size is not a measurement of a future deployable function with its runtime adapter.

## Measurements and remaining work

The current query-only executable is 740,464 bytes, or 348,775 bytes gzip-compressed. The numeric index is 90,263,179 bytes. These figures exclude the semantic indexes still required for full ECL.

A fresh process opened an already-local index on a Docker Linux volume in 0.382 seconds. Starting the container and obtaining its first answer took 1.216 seconds. The corpus run peaked at 113,324,032 bytes of container-charged memory, under a one-CPU, 256 MiB limit. Snowstorm was importing elsewhere on the host, so these are diagnostic measurements. They do not measure cold storage, network transfer or a cloud provider's cold start.

The same buffered reader took 3.327 seconds through a Windows bind mount. Filesystem choice matters. A larger read buffer reduces filesystem calls while retaining checksum and structural validation.

The [recorded run](refinement-results.json) contains the binary checksum, index size, startup log, complete-set digests and raw warm-query samples.

Before choosing a host, measure:

- The complete runtime package and all required semantic index files.
- Empty-cache acquisition, checksum verification and first expansion over object storage.
- Fresh-process startup with locally cached files, then warm sequential and concurrent queries.
- Peak memory including file cache, temporary result sets and optional semantic indexes.
- Costs at the expected request frequency, including storage reads and data transfer.

Keep index versions immutable and pin each request batch to one version. Warm-instance reuse is an optimisation; correctness must not depend on an instance surviving between requests.
