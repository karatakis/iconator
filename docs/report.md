# Brief

Single binary wrapping the existing lookup library: JSON API for lookups, static file serving for the SVG assets, and an HTML page to see example usage

# Advantages

* Graceful shutdown (Ctrl-C / SIGTERM, in-flight requests drain, reason logged)
* Timeout handling, bounds worst-case latency at 10s and returns 408 instead of a 500
* Error tracing and request tracing are built in
* Code is properly tested, the exact production router is reused by the integration tests, so tests cannot drift from what is served
* Custom error type makes code easier to read and maintain, because you can get a quick glimpse of what can go wrong, and static error messages, so no backend logic is leaked to the users
* Sanitization layer over the lib.rs calls to catch panics and convert them to application errors, a caught panic is mapped to 404 "no icon found", which is semantically what the library would have returned if it could, plus a `warn!` log line
* Base64 `path` parameter, so values with URL-hostile characters (`/`, `#`, `?`, spaces, non-ASCII) never need percent-encoding
* Friendly UI to debug the application and get examples of how to use the API, the form result renders the copyable base64 API URL, which doubles as usage documentation
* Maud gives type-checked HTML: no mismatched tags, and interpolated values (paths, IDs) are escaped automatically
* CORS enabled: any origin can call the API (public, read-only, unauthenticated), preflight OPTIONS is answered, and even error/timeout responses carry the access-control headers a browser client needs to read them
* Health endpoint that can help with production integration on the existing microservices observer
* CI ready, with tests, formatting, and linting
* Config is testable by construction: `Config::from_lookup` takes an injected lookup function, so unit tests never touch the shared process environment
* ServeDir handles path-traversal safety and content-type detection for free
* Error bodies are machine-parseable JSON everywhere on the API (`{"error": "<static message>"}`), and offending input goes to logs, not responses

# Limitations

* Uses a static database of icons and paths. The fst lookup maps are generated at compile time by `build.rs` from `icons/lookup_table.rs`, so adding a new icon-to-ID mapping requires a full rebuild. The SVG assets themselves are read from disk and must ship with the executable. The icon grid also caches the directory listing for the lifetime of the process, so icons dropped in at runtime never appear until a restart.
* There is some `unsafe` code in the `src/lib.rs` file, three `unwrap_unchecked()` calls on fst maps. The sharper edge is actually the plain `unwrap() 's on `file_name()`/`extension()`, which panics on paths like `"  "`, `"foo/.."`, or an unknown extensionless filename (`LICENSE`), the server has to defend against its own library.
* File and Folder path are required to be base64 encoded, so it can be HTTP url safe, a plain path is now a 400, which is slightly unfriendly for curl one-liners (tradeoff accepted for URL safety and a single unambiguous contract)
* JSON responses might add overhead, negligible for this use case, but a batch endpoint (see Future Work) would amortize it for IDE clients resolving whole trees
* The caching headers are not properly configured, `ServeDir` sets no `Cache-Control`; icons are immutable per ID, so a `Cache-Control: public, max-age=31536000, immutable` layer would let IDE clients cache them essentially forever
* No proper authentication if required, the service is open to any client that can reach it; fine for an internal tool, not for a public deployment
* No proper CDN setup, if it were a large-scale service, it would cause lots of network load
* `ServeDir`'s 404 body is not JSON, so a miss on `/icons/{id}.svg` breaks the "every error is machine-parseable" contract that the API endpoints keep
* Client-supplied paths are logged in plaintext on misses/invalid input (`log::warn!`), filenames can be sensitive, and log lines are not structured; there is also no request-ID correlation to tie a client report to log lines
* No rate limiting or request-size limits beyond axum's defaults; a malicious client can spam lookups unboundedly
* The timeout and graceful-shutdown paths are not covered by tests (hard to drive through `oneshot`); the 10s timeout and drain behavior are verified manually
* GET-only, one-path-at-a-time API: an IDE resolving a whole project tree needs one request per file, which is chatty over the network

# Future Work

* **Fix the library upstream**, replace the panicking `unwrap()`s in `src/lib.rs` with `Option`-returning logic (e.g. `path.extension().and_then(|ext| EXT_ICONS.get(ext.as_bytes()))`). This removes the need for `catch_unwind`, the panic backstop, and the "pre-validate to avoid the panic path" workarounds entirely, the biggest single simplification available.
* **Batch lookup endpoint**, `POST /api/v1/icons/batch` with a JSON list of paths returning a map of path → icon ID; one round trip for an IDE's whole tree instead of N.
* **Caching layer**, `Cache-Control: immutable` on `/icons/*` (icons never change per ID), plus optional ETag/conditional-request support; this is the highest-value line of code for real IDE clients.
* **Readiness vs liveness**, `/health` stays as liveness; add `/ready` that additionally verifies `./icons` is readable (the only deployment dependency), so a bad image is pulled from a ready pod automatically.
* **Observability**, structured JSON logs, a `/metrics` Prometheus endpoint (request rate, latency histogram, 4xx/5xx counts, panic-backstop hits as an alertable signal), and `x-request-id` propagation for correlating client reports with log lines.
* **Runtime-extensible icon set**, load the fst maps and the SVG directory at startup from a configurable data directory instead of compiling them in, so new icons ship without a rebuild; alternatively, embed everything with `rust-embed` for a single-file binary with no filesystem dependency.
* **Deployment**, multi-stage container build (distroless runtime, `./icons` copied in), TLS terminated at a reverse proxy, horizontal scaling (already stateless), GitHub Actions CI running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` on every push.
* **API hardening**, rate limiting (tower), and a JSON 404 responder for the `/icons` subtree so the error contract is uniform; CORS is already permissive, but a future authenticated API would need to switch to an explicit origin allowlist (the wildcard `*` is incompatible with credentialed requests).
* **API versioning**, prefix with `/api/v1/` before the contract grows (batch, search-by-name, fuzzy matching), so breaking changes stay backward compatible.
