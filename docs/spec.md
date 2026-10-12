---

NOTE: this is AI generated, I used it as intermediate step to review the work that the agent will perform and it allowed me to fine-tune the end goal.

---

# Spec: Iconator Web Server

A network-facing HTTP service that exposes the `iconator` icon-lookup library, so
clients (IDEs, editor plugins, browsers) can resolve file/folder paths to icon IDs
and fetch the corresponding SVG assets.

## Objective

- **Outcome:** A single axum binary that wraps the existing library and serves
  three things: a JSON lookup API, the SVG icon assets, and a small HTML page
  with a form to test lookups manually plus a view-only grid of the icon set.
- **Users:** (a) programmatic clients — IDEs/editor plugins that resolve paths to
  icon IDs and download the SVGs; (b) humans — anyone wanting to try the service
  in a browser.
- **Why:** The library only works in-process; the assignment asks for network
  access.
- **Success:** `cargo run` starts a server; the API returns correct icon IDs for
  the library's own test vectors; unknown paths yield clean 404s (never a panic
  or a 500); the SVG assets are downloadable; tests pass.
- **Out of scope:** authentication, caching headers beyond the basics, changing
  the library's behavior, database anything, CDN/deployment infrastructure.

## Confirmed Decisions (from interview)

| Question | Decision |
|---|---|
| Response contract | JSON icon ID from lookup endpoints, plus SVGs served as static assets |
| HTML UI | Yes — maud index page; revised per user: lookup form + view-only icon grid |
| No icon found | 404 with a JSON error body |
| Path encoding | `path` is always base64-encoded (standard or URL-safe, padding optional); plain-text paths were removed |
| Scope | Assessment-polished, fits the ~1h timebox |

## Tech Stack

Pinned by the user. Rust edition 2024, existing workspace.

```toml
tokio = { version = "1.53.1", features = ["signal", "macros", "rt-multi-thread"] }
axum = { version = "0.8.9", features = [] }
base64 = "0.22"                                               # decoding/encoding of the `path` param
maud = { version = "0.27.0", features = ["axum"] }
serde = { version = "1", features = ["derive"] }             # JSON request/response types
serde_json = "1"                                             # serializer used by axum::Json
tower = { version = "0.5.3", features = ["util"] }            # +util: ServiceExt::oneshot for tests
tower-http = { version = "0.7.0", features = [
    "catch-panic", "cors", "sensitive-headers", "timeout", "trace", "fs"
] }
dotenvy = "0.15.7"
tracing = "0.1.44"
log = "0.4.33"
env_logger = "0.11.11"
anyhow = "1.0.104"
thiserror = "2.0.20"
```

Deviation notes (the only ones):

1. **`tower` gains the `util` feature** — needed for `tower::ServiceExt::oneshot`
   in integration tests. No new crate.
2. **`serde` + `serde_json` added** (user-approved addition to the original dep
   list). Responses are serialized through `axum::Json` with
   `#[derive(Serialize)]` response structs — `axum::Json` sets
   `Content-Type: application/json` and serde guarantees correct escaping.
   Query params are parsed with a typed `#[derive(Deserialize)]`
   `LookupParams { path: Option<String> }` via axum's `Query`
   extractor (percent-decoding handled by axum). Handlers take
   `Result<Query<LookupParams>, QueryRejection>` and map a rejection to the
   JSON 400, so malformed input never produces axum's default plain-text
   rejection body. Serde is already a transitive dependency of axum's
   default `json`/`query` features.
3. **Logging bridge:** `tracing` needs its `log` feature enabled
   (`tracing = { version = "0.1.44", features = ["log"] }`) so that
   `TraceLayer`'s tracing events flow into `env_logger`, which is the logging
   backend (there is no `tracing-subscriber` in the dep list).
4. **`sensitive-headers`** is enabled in the feature list but this service sets
   no sensitive headers; the feature stays enabled per the given list but no
   redaction layer is required.
5. **`base64 = "0.22"` added** (user-requested feature, see API Contract):
   the `path` query parameter is base64-encoded, so path values never contain
   characters that are hostile in URLs.
6. **`TimeoutLayer::with_status_code(408, 10s)`** instead of the deprecated
   `TimeoutLayer::new(10s)` (deprecated in tower-http 0.7): a timed-out
   request receives `408 Request Timeout` rather than a 500.
7. **`icons/lookup_table.rs`:** the three array declarations changed from
   `const` to `static` to satisfy clippy's `large_const_arrays` lint
   (user-requested "fix clippy issues"). Purely a declaration-keyword change —
   no lookup data was altered and `build.rs` consumes them unchanged.
8. **CORS** (user-requested after spec approval): the `cors` feature is
   enabled on `tower-http` (no new crate) and `CorsLayer::permissive()` is
   the outermost middleware layer. Any origin may call the service — it is
   public, read-only, and unauthenticated — and because CORS wraps
   everything, error and timeout responses carry the access-control headers
   too. The layer also answers browser preflight `OPTIONS` requests.

## API Contract

All responses set explicit `Content-Type`. Bodies are serialized with
`serde_json` via `axum::Json`, so escaping is always correct. Error bodies are
always JSON: `{"error": "<static message>"}` — messages are static strings (no
user input is reflected into bodies). The offending input is recorded in a
`log::warn!` line instead of in the response.

### JSON lookup endpoints

```
GET /api/icon/file?path=<base64-encoded path>
GET /api/icon/folder?path=<base64-encoded path>
```

| Status | Condition | Body |
|---|---|---|
| 200 | Icon found | `{"icon_id": 525}` |
| 400 | `path` param missing or empty (incl. base64 of an empty string) | `{"error": "missing required query parameter 'path'"}` |
| 400 | `path` is not decodable base64, or decodes to non-UTF-8 bytes | `{"error": "invalid base64 in query parameter 'path'"}` |
| 404 | No icon matches, or the path cannot be resolved (incl. library panic cases, see below) | `{"error": "no icon found"}` |
| 405 | Non-GET method | axum default |
| 408 | Request exceeded the 10s timeout | empty body (TimeoutLayer) |
| 500 | Unexpected panic (backstop only) | `{"error": "internal server error"}` |

The `path` value is base64-encoded (user decision, replacing the earlier
percent-encoding contract). Standard and URL-safe alphabets are both
accepted, padding is optional, and ASCII whitespace inside the value is
ignored (copy-paste artifacts). Base64 keeps values free of characters that
are hostile in URLs (`/`, `#`, `?`, spaces, non-ASCII), so clients never need
percent-encoding. An earlier plain-text `path` parameter was removed; a
plain (non-base64) path now yields the 400 invalid-base64 error. The
offending input is recorded in a `log::warn!` line instead of in the response.

### SVG assets

```
GET /icons/{id}.svg   →  200, Content-Type: image/svg+xml
```

Served by `tower_http::services::ServeDir` nested at `/icons` from the `./icons`
directory. `ServeDir` handles path-traversal safety and content-type detection.
A nonexistent ID yields ServeDir's default 404.

### HTML UI

```
GET /   →  200, Content-Type: text/html; charset=utf-8
```

A maud-rendered page with two parts:

**Lookup form** (top) — for testing lookups manually in the browser:

- a GET form (so queries are bookmarkable) with a `path` text input and a `kind`
  selector (file / folder), submitting to `/`
- the form itself submits the plain typed path (the browser percent-encodes
  form submissions; this is the page's own input, not the API contract)
- when `?path=&kind=` are present: the result — the resolved icon rendered
  inline via `<img src="/icons/{id}.svg">`, its numeric ID, and a copyable
  API URL using the base64 encoding: `/api/icon/{file|folder}?path=<URL-safe
  unpadded base64>`
- on miss: a friendly "no icon found" message (the page still returns 200 with
  the message rendered; only the API endpoints use 404)

**Icon grid** (below) — a view-only gallery of the full icon set:

- the handler lists `./icons/*.svg` once (cached in a `OnceLock`, sorted
  numerically by ID) — the same directory `ServeDir` reads
- each cell shows the icon via `<img src="/icons/{id}.svg">` and its numeric ID
- if `./icons` cannot be read, respond 500 (deployment error — the SVG route
  would be broken too)

### Health

```
GET /health   →  200, {"status":"ok"}
```

For future liveness probes; returns immediately without touching the lookup.

## Library Sharp Edges (must be handled by the server)

`src/lib.rs` panics — it does not return `None` — in these cases:

1. `get_icon_for_file`: path has no `file_name()` (e.g. `""`, `"foo/.."`) — the
   `.unwrap()` on line 26 panics.
2. `get_icon_for_file`: filename not in the filename map **and** the path has no
   extension — the `.unwrap()` on line 30 panics (e.g. `LICENSE`, which is
   absent from the table; note that `.gitignore` **is** in the filename table
   with icon 206 and resolves normally).
3. `get_icon_for_folder`: path has no `file_name()`.

Mitigation (in order of preference):

- **Handlers pre-validate:** reject a missing/empty `path` (or one that decodes
  to empty) with 400 before touching the library; reject undecodable base64
  with 400.
- **`std::panic::catch_unwind`** (with `AssertUnwindSafe`) wraps each library
  call. A caught panic is mapped to 404 `{"error": "no icon found"}` — this is
  semantically what the library would have returned if it could — plus a
  `warn!` log line recording the input.
- **`CatchPanicLayer`** (with a custom JSON responder) stays in the middleware
  stack as a defense-in-depth backstop for any other panic, responding 500
  with a JSON body.

The library itself is **not modified** (its public API, build script, and lookup
tables are off-limits; the existing library tests must keep passing). The panic
behavior is a known issue to document in the design note (see Design Note
below for its current status).

## Architecture

```
src/main.rs        → bootstrap: dotenvy, env_logger init, config, build router,
                     bind TcpListener, axum::serve(...).with_graceful_shutdown(...)
src/config.rs      → read HOST / PORT from env (defaults 0.0.0.0 / 3000)
src/error.rs       → ApiError enum (thiserror) + IntoResponse impl
src/api.rs         → /api/icon/{file,folder} + /health handlers; base64 path
                     decoding + panic-safe lookup wrappers; build_router()
src/pages.rs       → maud index page (lookup form + icon grid)
```

The binary consumes the crate's own library (`iconator::get_icon_for_file` /
`get_icon_for_folder`) — `src/main.rs` alongside `src/lib.rs` in the same crate.

**Middleware stack** (outermost first):

```
CorsLayer::permissive()       → any origin/method/headers (public read-only
                                service); preflight OPTIONS answered here;
                                outermost so error/timeout responses keep
                                the access-control headers
TimeoutLayer(10s)             → bound worst-case request latency; 408 on timeout
TraceLayer                    → request/method/status/duration via tracing → log
CatchPanicLayer               → panic backstop → 500 JSON
```

**Graceful shutdown:** `tokio::signal::ctrl_c()` plus SIGTERM (Unix
`signal(SignalKind::terminate())`), whichever comes first, drives
`axum::serve(...).with_graceful_shutdown(...)`. In-flight requests drain; the
shutdown reason is logged.

**Config:** environment loading via dotenvy — `.env.local` first (personal
overrides), then `.env`; dotenvy never overwrites already-set variables, so
`.env.local` wins. Missing files are fine — ignore. Variables read: `HOST`
(default `0.0.0.0` — the point of the task is network access), `PORT` (default
`3000`), and `RUST_LOG` (log filter, e.g. `info`). Empty values are treated as
unset; an invalid `PORT` falls back to `3000` with a `warn!` line. A committed
`.env.example` documents every variable; `.env` and `.env.local` are
gitignored. `Config::from_lookup` takes an injected lookup function so the
unit tests exercise defaults/overrides without touching the shared process
environment.

**Logging:** `env_logger` reads `RUST_LOG` (settable via `.env` / `.env.local`);
`tracing`'s `log` feature — enabled by default — routes `TraceLayer` events
into it.

## Code Style

- `rustfmt` defaults; no custom config.
- Handlers return `Result<impl IntoResponse, ApiError>`; error conversion lives
  in one `IntoResponse` impl.
- No `unwrap()` in server code paths that touch request input.

```rust
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("missing required query parameter 'path'")]
    MissingPath,
    #[error("invalid base64 in query parameter 'path'")]
    InvalidBase64,
    #[error("no icon found")]
    NoIcon,
    #[error("internal server error")]
    Internal,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    error: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::MissingPath | ApiError::InvalidBase64 => StatusCode::BAD_REQUEST,
            ApiError::NoIcon => StatusCode::NOT_FOUND,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(ErrorResponse { error: self.to_string().as_str() })).into_response()
    }
}
```

## Testing Strategy

Framework: `cargo test` (built-in) with `tokio::test` for async tests.

**Unit tests** (in-module, `#[cfg(test)]`):

- config defaults, env overrides, invalid/empty values, injected lookup
- panic-safety wrapper: extensionless unknown filename (`"LICENSE"`) and path
  without a file name (`"foo/.."`) map to `ApiError::NoIcon`, never panic
- base64 decoding: all common variants (URL-safe/standard, padded/unpadded,
  stray whitespace), URL-hostile characters (`"weird name #?.rs"`), invalid
  base64 and non-UTF-8 payloads → `ApiError::InvalidBase64`
- `ApiError` variants map to the specified statuses

**Integration tests** (`tests/api.rs`): build the full production router (same
function as `main` uses — no duplicated stack) and drive it with
`tower::ServiceExt::oneshot`. Because a test crate cannot link a binary's
modules and `src/lib.rs` is off-limits to modify, the tests pull in the
production modules with `#[path = "../src/..."]` includes (`api.rs`,
`error.rs`, `pages.rs`; `config.rs` is covered by its in-module unit tests):

- file lookup: `?path=<base64 of src/main.rs>` → 200 `{"icon_id": 525}`
  (library test vector)
- folder lookup: `?path=<base64 of .github>` → 200 `{"icon_id": 862}`
- unknown extension → 404 with the JSON error body
- extensionless unknown path (`LICENSE`) and path without a file name
  (`foo/..`) → 404 (panic-safety through the full stack)
- base64 path carrying URL-hostile characters (`weird name #?.rs`) → 200 525
- standard (non-URL-safe) base64 alphabet also decodes
- invalid base64 → 400 with the invalid-base64 error body
- plain (non-base64) path → 400 invalid base64 (the old contract is gone)
- missing/empty `path` → 400 with the missing-path error body
- `/health` → 200 `{"status":"ok"}`
- `/` → 200 `text/html`, contains the lookup form and the grid (multiple
  `<img src="/icons/...">` cells with ID labels)
- `/` with `?path=src/main.rs&kind=file` → 200, result section shows icon 525
  and the base64 API URL (`path=c3JjL21haW4ucnM`)
- `/` with an unknown path → 200, "no icon found" message rendered (not 404)
- `/icons/0.svg` → 200 `image/svg+xml` (ServeDir reads the real `./icons` dir;
  cargo runs tests with the package root as cwd); unknown ID → 404
- method not allowed on the API endpoints → 405
- every response (including 404s) carries
  `Access-Control-Allow-Origin: *`; a preflight `OPTIONS` request with
  `Origin` + `Access-Control-Request-Method` is answered 200 by the CORS
  layer with the wildcard allow-methods/allow-headers

**Existing library tests** (`src/lib.rs`) must keep passing untouched.

## Design Note (assignment item 2)

**Status: not currently appended to `README.md`.** A design note covering the
choices below was written during implementation, but the README was reverted
to its original text during review; it now carries the project's own
"Setup and usage" documentation (prerequisites, run instructions, `.env`
configuration, curl examples, development gates). If the note is wanted for
the assessment submission it needs to be re-appended; intended coverage:

- design choices and tradeoffs: axum (tokio-native, extractor ergonomics),
  serde/serde_json for typed JSON payloads (compile-checked shapes, correct
  escaping, `axum::Json` integration), base64 path encoding (URL-hostile
  characters never reach the URL layer), static
  `ServeDir` for assets vs. embedding, `catch_unwind` over library panics vs.
  fixing the library (out of scope), maud for type-checked HTML
- productionization: containerized deployment, `RUST_LOG`-based observability
  → structured logs/metrics (Prometheus), health/readiness probes, reverse
  proxy with TLS, horizontal scaling statelessness, CI (fmt/clippy/test), and
  the library fixes worth upstreaming (replace `unwrap`s with `Option` returns)

## Commands

```
cargo run                 # start the server (reads .env.local / .env if present)
cargo build               # build library + binary
cargo test                # unit + integration tests
cargo fmt --check         # formatting gate
cargo clippy              # lint gate
cp .env.example .env.local   # optional: local overrides (gitignored)
curl "localhost:3000/api/icon/file?path=$(printf 'src/main.rs' | base64)"
curl "localhost:3000/api/icon/folder?path=$(printf '.github' | base64)"
```

## Boundaries

- **Always:** keep the existing library tests passing; run `cargo test` before
  considering work done; never let a request panic reach the client as a 500
  when it is a lookup miss; JSON errors machine-parseable.
- **Ask first:** modifying `src/lib.rs`, `build.rs`, or `icons/` (including
  adding any dependency or feature beyond the noted deviations above);
  changing the route names after this spec is approved.
- **Never:** commit `.env`, `.env.local`, or secrets; alter the lookup tables or
  icon data; change the library's public API.

## Success Criteria

1. `cargo run` starts a server bound to `HOST:PORT` (default `0.0.0.0:3000`) and
   logs its listen address.
2. The two lookup endpoints return the library's own test-vector icon IDs
   (`main.rs` → 525, `.github` → 862) as `{"icon_id": N}` with
   `application/json`, with the path supplied base64-encoded.
3. A miss (unknown extension, extensionless file, unknown folder) returns
   404 `{"error": "no icon found"}` — never a 500, never a connection reset.
4. Missing `path` returns 400 with `{"error": "missing required query parameter
   'path'"}`; an undecodable base64 value returns 400 with
   `{"error": "invalid base64 in query parameter 'path'"}`.
5. `GET /icons/525.svg` returns the SVG with `Content-Type: image/svg+xml`.
6. `GET /` renders an HTML page with the lookup form and the view-only icon
   grid; submitting the form shows the matched icon inline with its ID and a
   copyable base64 API URL.
7. `GET /health` returns `{"status":"ok"}`.
8. Ctrl-C / SIGTERM shuts down gracefully (in-flight requests drain, shutdown
   logged).
9. `cargo test` passes (unit + integration + untouched library tests);
   `cargo clippy` is clean.
10. The design note (assignment item 2) — **currently not appended**; the
   README carries setup/usage documentation instead (see Design Note above).

## Resolved Questions

Formerly open questions, answered by the user:

1. **`tracing` `log` feature** — enabled by default; log level is configured
   through `.env` / `.env.local` via `RUST_LOG`.
2. **Route naming** — confirmed: `/api/icon/file`, `/api/icon/folder`.
3. **Bind config** — `HOST` / `PORT` come from `.env` / `.env.local` (defaults
   `0.0.0.0:3000`); a `.env.example` is committed; `.env.local` is read in
   addition to `.env` and takes precedence.
4. **HTML UI** — the index page pairs a form for testing lookups manually with
   a view-only grid of the icon set (final revision: form restored alongside
   the grid).
5. **Path encoding** — the `path` parameter is base64-encoded (decision taken
   after implementation of a dual `path`/`path_b64` contract: the plain-text
   `path` parameter was removed and `path_b64` was renamed to `path`). This
   required the `base64` crate (deviation note 5).
6. **Clippy issues** — the user requested clippy warnings be fixed; the
   `large_const_arrays` lints in `icons/lookup_table.rs` were resolved with a
   `const` → `static` declaration change (deviation note 7).
