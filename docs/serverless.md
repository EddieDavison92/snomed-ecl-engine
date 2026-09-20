# Serverless query runtime

The engine must run without an always-on API, Redis or database server. Import and index construction run offline. Vercel is the owner's preferred hosting platform. Its HTTP wrapper can live in another repository and depend on this library.

## Deployment model

Publish immutable indexes under an edition and checksum. Prefer fetching a pinned, prebuilt index during the application build, verifying it and including it in the function bundle. Application builds reuse the index rather than importing RF2 again. Each instance opens its bundled files once and keeps the store for subsequent requests while warm. Displays remain a separate lookup after expansion.

Bundling removes an application-level startup download, but platform bundle preparation and index loading still contribute to a cold start. Keep startup downloads to local temporary storage as a fallback if the complete index outgrows the chosen deployment format.

Use local files for graph traversal. A remote object request for every relationship would replace inexpensive memory reads with network round trips. Future semantic indexes should have independent files so hierarchy queries do not have to acquire description text or history data. Queries that need those files must acquire them before returning a complete result.

The current reader loads the whole numeric store into memory. It does not yet download indexes, load semantic files on demand, or provide a cloud runtime. Consider memory mapping or bounded block reads only after the full semantic data layout exists and measurements show a benefit. Verify the complete engine against Vercel's limits before committing to the final packaging.

## Vercel deployment target

Use a thin native Rust Function wrapper around the query library, with importer features disabled. Vercel documents an official [Rust runtime](https://vercel.com/docs/functions/runtimes/rust), currently in beta, using `vercel_runtime` and Fluid compute. No deployment has been created yet.

Bundle index files as private function assets, outside public/static routes. Initialise one shared immutable store per process, with bounded query concurrency. Pin the edition and store format in the deployment; release updates produce a new deployment rather than replacing files in a running instance.

Vercel documents a standard [250 MB uncompressed function bundle limit and 4.5 MB request/response limit](https://vercel.com/docs/functions/limitations). The current 90.3 MB numeric index leaves room for the wrapper, but the full semantic index has not been measured. The advertised [large-function beta](https://vercel.com/changelog/vercel-functions-can-now-be-up-to-5-gb-in-package-size) names Node.js and Python; do not assume that allowance applies to the native Rust runtime. Check the actual packaged output in a preview deployment. Paginate large expansions and provide count-only responses.

These platform facts were checked on 20 September 2026. Measure deployment cold starts and concurrency on Vercel; local Linux timings do not predict them directly.

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

Before deploying the complete engine, measure:

- The complete runtime package and all required semantic index files.
- Empty-cache acquisition, checksum verification and first expansion over object storage.
- Fresh-process startup with locally cached files, then warm sequential and concurrent queries.
- Peak memory including file cache, temporary result sets and optional semantic indexes.
- Costs at the expected request frequency, including storage reads and data transfer.

Keep index versions immutable and pin each request batch to one version. Warm-instance reuse is an optimisation; correctness must not depend on an instance surviving between requests.
