# Photo Frame

A self-hosted digital photo frame. A Rust backend indexes a folder of photographs, generates display-ready derivatives and serves them to browser clients on your LAN. The main client is an iPad installed to the home screen as a PWA, but the frame view also runs in any desktop browser.

The built-in iPad slideshow gives no control over pacing or ordering. Here each client picks its own dwell time, ordering mode and tag filter, and keeps those settings in the browser.

- **Frame view (`/`):** full-screen and gesture-driven, with keyboard equivalents on desktop (arrows, space to pause, `f` to favourite).
- **Management view (`/manage`):** thumbnail grid, multi-select, tags, date correction, rotation, upload, delete and a status panel.
- **Your files stay yours.** Photographs live in a mounted folder and originals are never modified. Tags, favourites, date overrides and rotation are stored in the database and can be exported to a JSON file and restored after a rebuild.

## How it works

Two binaries share nothing but a database and a library volume:

| Binary | Job | Replicas |
| --- | --- | --- |
| `photoframe-web` | HTTP API, static client, media serving, uploads | 1..n (n > 1 needs Postgres) |
| `photoframe-indexer` | Scans the library, generates derivatives, promotes uploads, handles delete and export requests | exactly 1 |

```
$LIBRARY_ROOT/
  incoming/      new arrivals (uploads land here); the indexer promotes them
  quarantine/    for an external malware scanner; never read by the app
  library/       canonical originals
  derivatives/   generated, disposable
  exports/       curation exports
```

Anything that can write files into `incoming/` can feed the frame. The full design is in [docs/SPEC.md](docs/SPEC.md).

The database is SQLite or Postgres, selected by `DATABASE_URL`. SQLite is for one pod (or one machine) running both processes. It must never sit on NFS or be shared across nodes. Use Postgres for anything more.

## Quick start (local)

You need a Rust toolchain and Node. The [devcontainer](.devcontainer/) has both. No database server is required.

```bash
(cd web && npm ci && npm run build)   # do this before cargo build/run: rust-embed's
                                       # dev-mode path check is baked in at compile time

# shell 1: web
DATABASE_URL=sqlite://./state/photoframe.db LIBRARY_ROOT=./library \
  ADMIN_PASSWORD=dev cargo run -p photoframe-web

# shell 2: indexer, same database file
DATABASE_URL=sqlite://./state/photoframe.db LIBRARY_ROOT=./library \
  cargo run -p photoframe-indexer
```

Copy some photos into `./library/incoming/`, or upload them at `http://localhost:8080/manage` (user `admin`, password `dev`). Supported inputs are JPEG, PNG, WebP and HEIC. Files that can't be decoded go to `quarantine/`.

## Deploying to Kubernetes

The Helm chart in [helm-charts/photoframe/](helm-charts/photoframe/) supports both topologies. See its [README](helm-charts/photoframe/README.md) for details.

```bash
helm install frame oci://ghcr.io/<owner>/charts/photoframe --version 0.1.1 -n photoframe --create-namespace \
  --set image.registry=ghcr.io/<owner> \
  --set admin.password=<password> \
  --set ingress.enabled=true --set 'ingress.hosts={frame.example.com}'
```

Images and the chart are built by the `ci` workflow's `image` and `helm` jobs on every push to `main` that passes tests, and published to `ghcr.io/<owner>/photoframe-web`, `photoframe-indexer` and `charts/photoframe`, using the workflow's built-in token. Make the packages public to pull them without a secret, or create an `imagePullSecrets` entry with a `read:packages` token if you keep them private. TLS terminates at your Ingress; the apps speak plain HTTP.

To build the images yourself:

```bash
docker build --target web     -t photoframe-web .
docker build --target indexer -t photoframe-indexer .
```

## Configuration

Environment variables only. Invalid values stop the process at startup rather than falling back silently.

| Variable | Binary | Default | Purpose |
| --- | --- | --- | --- |
| `DATABASE_URL` | both | required | `sqlite://…` or `postgres://…` |
| `LIBRARY_ROOT` | both | required | Library volume mount path |
| `ADMIN_PASSWORD` | web | required unless `ADMIN_ALLOWED_CIDRS` | HTTP Basic password for management actions |
| `ADMIN_USERNAME` | web | `admin` | Management username |
| `ADMIN_ALLOWED_CIDRS` | web | — | Comma-separated CIDRs (or bare IPs) granted management access without credentials. Alternative to the password; either grants access. `ADMIN_PASSWORD` is optional when this is set |
| `TRUSTED_PROXY_CIDRS` | web | — | Peers whose `X-Forwarded-For` is trusted (the Ingress controller). Without it the TCP peer address is used, which behind an Ingress is the proxy itself |
| `BIND_ADDR` | web | `0.0.0.0:8080` | Listen address |
| `PUBLIC_HOSTNAME` | web | unset | Canonical hostname for the PWA manifest. Set once and leave it |
| `MAX_UPLOAD_BYTES` | web | `104857600` | Per-file upload limit |
| `SCAN_INTERVAL_SECS` | indexer | `60` | Polling interval |
| `INGEST_REQUIRE_SCAN` | indexer | `false` | Leave `incoming/` to an external scanner |
| `INGEST_QUIET_SECS` | indexer | `5` | Minimum age before an incoming file is promoted |
| `DERIVATIVE_WORKERS` | indexer | `2` | Concurrent derivative jobs |
| `MAX_DECODE_PIXELS` | indexer | `80000000` | Decoder pixel limit |
| `MAX_DECODE_BYTES` | indexer | `134217728` | Decoder input size limit |
| `DECODE_TIMEOUT_SECS` | indexer | `30` | Per-image time limit |
| `ENABLE_AVIF` | indexer | `true` | Parsed but currently ignored; derivatives are JPEG only |
| `LOG_LEVEL` | both | `info` | `tracing` filter |

A 48 MP original peaks around 450 MB of memory per derivative worker, so size the indexer accordingly.

## Backup and restore

The filesystem decides which photographs exist, so the database can always be rebuilt by rescanning. Only curation (favourites, tags, date overrides, rotation) can't be rebuilt. Export it regularly with the management UI's export action, or from the CLI:

```bash
photoframe-indexer export --out curation.json
photoframe-indexer import --in curation.json   # merges by content hash, into an empty or existing database
```

A photograph's identity is the BLAKE3 hash of its bytes, so moving or renaming a file keeps its tags. Re-encoding one creates a new photograph.

## Development

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test                                      # SQLite; Postgres too when TEST_POSTGRES_URL is set
(cd web && npm run check && npm test && npm run build)
(cd web && npm run e2e)                         # real binaries in real browsers; build everything first
```

| Path | Contents |
| --- | --- |
| `crates/photoframe-web` | axum server and embedded client |
| `crates/photoframe-indexer` | scanner, derivatives, promotion, export/import CLI |
| `crates/photoframe-store` | `Store` trait with SQLite and Postgres implementations |
| `crates/imagepipe` | decode, EXIF, resize, encode; no I/O, fuzzed |
| `web/` | Svelte client |
| `migrations/` | sqlx migrations for both backends |
| `helm-charts/photoframe/` | Helm chart |

Each crate and `web/` has its own `CLAUDE.md` with the rules that apply there.

## Status

Feature-complete for v1 on SQLite. Known gaps:

- **Postgres has never been run.** It compiles and shares its queries with SQLite, but expect small dialect bugs. Set `TEST_POSTGRES_URL` to run the contract tests.
- **No AVIF.** Derivatives are JPEG only, and AVIF input is quarantined.
- **HEIC is lightly tested,** with no corpus of real iPhone files yet.
- **Not tried on real iPad hardware:** touch gestures, Safari, wake lock and the offline service worker path.
- **The Helm chart has not been installed on a cluster.**
