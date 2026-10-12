# Mainmatter Rust Assessment

> Please take no more than ~1 hour to complete this assignment. We’ll discuss your solutions in a call in a few days. Feel free to ask questions if the assignment is unclear at any point.

## Context

This repository contains a simple file-icon lookup library as may be used by IDEs for their project tree sidebar. `get_icon_for_file` and `get_icon_for_folder` accept a filepath and return the numeric ID of a corresponding icon if one is found. Each icon ID corresponds to an svg icon in `./icons`.

## Assignment

1. Implement a web server than allows users to query for file/folder icons over the network. 
2. Write a short (<= 1 page) design/implementation note describing:
  - Design choices and tradeoffs you made (high-level architecture, web server choice, libraries you picked etc.)
  - How you would productionize this service (deployment, observability, etc.)
  
Keep your audience in mind. We’re not interested in a perfect solution but one where we see your thought-process and working style in action.
  
You may add these notes at the end of this file if you wish.

## Setup and usage

### Prerequisites

- A stable Rust toolchain (edition 2024, e.g. via [rustup](https://rustup.rs)).
- No other services: the server is a single stateless binary and serves the
  SVGs straight from the `./icons` directory.

### Run

```bash
cargo run            # builds and starts the server, default 0.0.0.0:3000
```

The server logs its listen address, e.g. `listening on 0.0.0.0:3000`. Stop it
with Ctrl-C (or SIGTERM) — in-flight requests drain before shutdown.

### Configuration via .env files

Configuration is read from the environment at startup; `.env` files are a
convenience for local runs:

1. Copy the template: `cp .env.example .env.local`
2. Edit values as needed:

| Variable   | Meaning                              | Default    |
|------------|--------------------------------------|------------|
| `HOST`     | Bind address                        | `0.0.0.0`  |
| `PORT`     | Listen port                         | `3000`     |
| `RUST_LOG` | Log filter for `env_logger`          | `info`     |

Precedence (highest first): variables already set in your shell, `.env.local`
(personal overrides, gitignored), `.env` (project defaults, gitignored), then
the built-in defaults above. dotenvy loads `.env.local` first and never
overwrites variables that are already set, so nothing shadows your shell
environment. Invalid or empty values fall back to the defaults (logged as a
warning for `PORT`).

### Try it

The `path` parameter is base64-encoded (standard or URL-safe alphabet,
padding optional). Base64 keeps values free of characters that are hostile in
URLs — `/`, `#`, `?`, spaces, non-ASCII — so no percent-encoding is needed:

```bash
# Icon ID for a file path (JSON)
curl "localhost:3000/api/icon/file?path=$(printf 'src/main.rs' | base64)"   # {"icon_id":525}
# Icon ID for a folder path (JSON)
curl "localhost:3000/api/icon/folder?path=$(printf '.github' | base64)"    # {"icon_id":862}
# Download an SVG asset
curl -O localhost:3000/icons/525.svg
```

Undecodable values return `400 {"error":"invalid base64 in query parameter 'path'"}`.

Other routes:

- `GET /` — HTML page: a lookup form for manual testing plus a view-only grid
  of the whole icon set. The form takes a plain path (the browser handles the
  URL encoding of its own submission) and the result shows the copyable
  base64 API URL.
- `GET /health` — liveness probe, returns `{"status":"ok"}`.

Misses return `404 {"error":"no icon found"}`; a missing or empty `path`
parameter returns `400 {"error":"missing required query parameter 'path'"}`.

### Development

```bash
cargo test             # unit + integration tests (includes the library's own tests)
cargo fmt --check      # formatting gate
cargo clippy --all-targets
```

Tests run against the same router the server serves and read the real `./icons`
directory, so no extra setup is needed.
