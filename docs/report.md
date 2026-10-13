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
* There is some `unsafe` code in the `src/lib.rs` file, three `unwrap_unchecked()` calls on fst maps. The sharper edge is actually the plain `unwrap()`s on `file_name()`/`extension()`, which panics on paths like `"  "`, `"foo/.."`, or an unknown extensionless filename (`LICENSE`), the server has to defend against its own library.
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
* A malicious actor can iterate all ids 1..2..3.. and download all images from the icon set. 

# Future Work

* **Fix the library upstream**, replace the panicking `unwrap()`s in `src/lib.rs` with `Option`-returning logic. This removes the need for `catch_unwind`, the panic backstop, and the "pre-validate to avoid the panic path" workarounds entirely, the biggest single simplification available. Look for alternatives that can remove unsafe code.
* **Batch lookup endpoint**, `POST /api/v1/icons/batch` with a JSON list of paths returning a map of path → icon ID. One round trip for an IDE's whole tree instead of N.
* **Caching layer**, `Cache-Control: immutable` on `/icons/*` (icons never change per ID), plus optional ETag/conditional-request support.
* **Readiness**, on startup add a check that `./icons` is readable, so a bad configuration of folder permissions shows early.
* **Runtime-extensible icon set**, load the fst maps and the SVG directory at startup from a configurable data directory instead of compiling them in, so new icons ship without a rebuild; alternatively, embed everything with `rust-embed` for a single-file binary with no filesystem dependency.
* **Deployment**, multi-stage container build (distroless runtime, `./icons` copied in), TLS terminated at a reverse proxy, horizontal scaling (already stateless), GitHub Actions CI running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` on every push.
* **API hardening**, rate limiting (tower), and a JSON 404 responder for the `/icons` subtree so the error contract is uniform; CORS is already permissive, but a future authenticated API would need to switch to an explicit origin allowlist (the wildcard `*` is incompatible with credentialed requests).
* **API versioning**, prefix with `/api/v1/` before the contract grows (batch, search-by-name, fuzzy matching), so breaking changes stay backward compatible.

# Assumptions (User Stories)

Assumptions about who uses the service and what they want from it, written as
user stories. Every story is covered by the unit/integration tests or verified
manually. The acceptance criteria are the observable HTTP behavior.

* As an IDE, I want to resolve a file so that I can render its icon. I
  base64-encode `src/main.rs` and call
  `GET /api/icon/file?path=c3JjL21haW4ucnM`. The service returns
  `200 {"icon_id": 525}` as `application/json`. I download `/icons/525.svg`
  and render it.
* As an IDE, I want to resolve a folder so that I can render its icon. The
  flow is the same through `/api/icon/folder`. For example, `.github` returns
  `{"icon_id": 862}`.
* As an IDE, I want to download the asset for a resolved ID so that I render a
  real icon. `GET /icons/525.svg` returns the SVG with
  `Content-Type: image/svg+xml`. I work with IDs and render real icons.
* As an IDE, I want to send real-world paths so that I do not handle special
  characters myself. Paths contain `/`, `#`, `?`, spaces, and non-ASCII text.
  The base64 `path` parameter makes them survivable. `weird name #?.rs`
  resolves to 200 / 525 without any percent-encoding.
* As a client, I want to emit whatever base64 variant my platform produces so
  that I never think about it. The service accepts the standard or URL-safe
  alphabet, padded or unpadded.
* As a browser-embedded client, I want to call the service cross-origin so
  that my page can use it. The service is public, read-only, and
  unauthenticated, so CORS is permissive and any origin can call.
* As a client, I want two distinct errors for malformed input so that I can
  tell a bug from bad input. A missing, empty, or base64-of-empty `path`
  returns 400 `"missing required query parameter 'path'"`. Undecodable base64
  (including a plain path, the old contract is gone) or non-UTF-8 bytes return
  400 `"invalid base64 in query parameter 'path'"`. The offending value goes
  to a `warn!` log, never into the response.
* As a client, I want a miss, not a crash, so that one client-side error path
  handles every miss. An unknown extension, an unknown folder, or a path the
  library panics on (`LICENSE`, `foo/..`) all yield
  `404 {"error": "no icon found"}`. `catch_unwind` catches the library panics
  and returns the 404 in their place. The connection never drops.
* As a client, I want the wrong method rejected so that a misbehaving client
  cannot hold a connection open. The API routes accept GET only, anything
  else is axum's default 405. A request that exceeds 10s is cut off with a
  408 instead of hanging.
* As a client, I want a defined answer for an unknown SVG so that I can fall
  back to a default icon. `/icons/999999.svg` returns status code 404.
  Traversal attempts are rejected.
* As a human user, I want to try a lookup manually so that I learn how the service
  works. I open `GET /`, type a path, pick file or folder, and submit.
* As a human user, I want to browse the icon set so that I discover what exists
  without knowing a path in advance. The grid below the form shows every
  `./icons/*.svg` with its ID.
* As a human user, I want a friendly miss so that the page does not break. An
  unknown path in the form still returns 200 with a friendly "no icon found"
  message rendered in the page.
* As an operator, I want to configure the service via environment so that I
  do not edit code. `HOST` (default `0.0.0.0`, the point of the task is
  network access), `PORT` (default `3000`), and `RUST_LOG` are read from
  `.env.local` then `.env`.
* As an orchestrator, I want to probe liveness so that dead pods are
  restarted automatically. `GET /health` returns `{"status": "ok"}`
  immediately, without touching the lookup or the filesystem.
* As an operator, I want to restart safely so that a rolling update never
  drops requests mid-flight. Ctrl-C / SIGTERM drains in-flight requests
  before exit and logs the shutdown reason.
* As an operator, I want to survive a panic so that a client never sees a
  dropped connection. If any handler panics outside the known library edges,
  the backstop returns `500 {"error": "internal server error"}` as JSON. The
  event is visible in the logs.
