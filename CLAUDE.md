# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status: v1 feature-complete on SQLite, Postgres untested

`docs/SPEC.md` is the source of truth. Everything in the spec's v1 scope is built and tested on SQLite: reconciliation, promotion and quarantine, derivative jobs, delete and export through the `requests` table, the curation export/import CLI, every HTTP endpoint, the frame client and the management client. `crates/photoframe-web`, `crates/photoframe-indexer`, `crates/photoframe-store`, `crates/imagepipe` and `web/` each have a CLAUDE.md with the parts of the spec that apply to them; this file holds what they share.

**Not done, and known:**
- **Postgres has run only against the store contract tests.** `postgres:16-alpine` on a throwaway server passes `photoframe-store`'s tests (each Postgres test creates its own `pf_test_*` database). That found one dialect bug (tag ordering needed `lower(name)`). The web and indexer binaries have never been run against Postgres, and no test there uses `TEST_POSTGRES_URL`.
- **AVIF.** Derivatives are JPEG only. `ENABLE_AVIF` is parsed and ignored, and AVIF *input* has no decoder (no pure-Rust one is wired up), so those files are quarantined. Wire `ravif`/`rav1e` behind the flag if the JPEG-only retreat proves too big.
- **HEIC has no test corpus.** The decoder is `heic-rs`, exercised only by its own tests. Add real iPhone files before trusting it (spec calls for a corpus).
- **Not tried on real hardware.** The frame client was driven in desktop Chromium (keyboard, overlay, long press, IndexedDB, reload) but never with real touch input, pinch, Safari, WakeLock or the service worker offline path. Gestures live in `web/src/lib/gestures.ts`; treat them as unverified on an iPad.
- **Helm chart** (`helm-charts/photoframe/`) renders and lints for both topologies but has never been installed on a cluster.

Not in the spec, chosen while building: `meta` is a key/value table (`generation`, `schema_version`, `scan_requested`, `last_scan_at`, `indexing_state`, `scan_total`, `scan_done`), so new keys need no migration; migrations live in `migrations/sqlite/` and `migrations/postgres/`; the Postgres `tags.name` uniqueness uses a `lower(name)` index rather than `citext`; the client test runner is vitest; `GET /api/session`, `GET /api/requests/{id}` and `GET /api/exports/{name}` exist for the management UI's login check and export download; the directory contract lives in `photoframe_store::Layout`, so web and indexer cannot disagree about derivative paths.

`helm-charts/photoframe/` deploys either topology from `database.type` (`sqlite`: one pod, two containers, Recreate; `postgres`: two Deployments). Its constraints come from the spec's *Deployment topology*, *Volumes* and *Startup and shutdown* sections: the indexer is a singleton with `Recreate`, web is 1..n only on Postgres, and the library volume is RWX. Check it with `helm lint` and `helm template` (pass `--set image.registry=x --set admin.password=x`).

## Commands

Develop inside the devcontainer (`.devcontainer/`, Debian 12 with Rust stable and Node LTS); the host VS Code sandbox has no toolchains. Debian 12 matches the `distroless/cc-debian12` runtime image the spec names.

```bash
# Local dev: only a directory and a file path, no Postgres, no k8s
DATABASE_URL=sqlite://./state/photoframe.db LIBRARY_ROOT=./library ADMIN_PASSWORD=dev cargo run -p photoframe-web
DATABASE_URL=sqlite://./state/photoframe.db LIBRARY_ROOT=./library cargo run -p photoframe-indexer   # separate shell, same DB file

# CI gate
cargo fmt --check
cargo clippy -- -D warnings
cargo test                       # SQLite always; Postgres contract tests too when TEST_POSTGRES_URL is set (CI sets it)
npm run check && npm test && npm run build   # in web/
npm run e2e                      # in web/; real binaries in real browsers. Build first: cargo build -p photoframe-web -p photoframe-indexer, then npm run build
#   PW_CHROMIUM=/usr/bin/chromium PW_NO_SANDBOX=1 npm run e2e -- --project=chromium   # container without Playwright's browsers
cargo fuzz                       # weekly, against imagepipe decode entry points

cargo test -p <crate> <test_name>   # single test
```

Running locally on SQLite alone is a hard requirement: "if a change makes this stop working, the change is wrong." Postgres is CI-only for contributors. Tests select Postgres via `TEST_POSTGRES_URL`; without it they skip. Build `web/` before release-building `photoframe-web` so the client is embedded (debug builds and `cargo test` work without it).

## Architecture

Two binaries, two images, two workloads, coupled only through the **database and the library volume**. There is no message queue and no shared memory, and adding one is an explicit anti-requirement.

- **`photoframe-web`** serves the API and static assets, is stateless, and runs 1..n replicas.
- **`photoframe-indexer`** walks the library, generates derivatives, promotes uploads and maintains the index. It must be a singleton: `replicas: 1`, `strategy: Recreate`, plus a startup advisory lock (`pg_try_advisory_lock` on Postgres, an exclusive row in `singleton` on SQLite) so a second instance exits loudly.
- **Change signalling:** a `generation` counter in `meta`. The indexer bumps it on any change to the indexed set, and web bumps it on any shared-state mutation. Web derives the manifest `ETag` from it. `POST /api/scan` only sets a flag in `meta`, which the indexer picks up on its next tick.
- **Database backend** is chosen by `DATABASE_URL` via a `Store` trait with SQLite and Postgres implementations. Queries live inside each implementation because sqlx compile-time macros are backend-specific, so keep the trait narrow. Migrations are embedded in both binaries and must be idempotent and safe to run concurrently. SQLite is supported only for two containers in one pod sharing a volume (WAL). Never put it on NFS or split the containers across nodes, and never "fix" that with `nolock`; switch to Postgres instead.
- **Library volume** (`LIBRARY_ROOT`, RWX, expected NFS): `incoming/`, `quarantine/`, `library/`, `derivatives/`, `exports/`. The directory contract is the integration surface for anything external (scanner, future mail ingest).

### Invariants that span web and indexer

- **Photograph identity is the BLAKE3 hash of the file bytes.** It is the primary key everywhere and is used in derivative filenames and export keys. A move or rename keeps identity; re-encoding creates a new photograph.
- **Filesystem wins on which files exist; the database wins on curation.** The database is never authoritative over the set of photographs. Favourites, `date_override` and tags are the only state that can't be rebuilt from a rescan. Any new column must be classified derived or authoritative, and an authoritative one must be added to the curation export format in the same commit.
- **`library/` is never modified in place.** Only promotion and deletion write to it; rotation, date correction and tagging are database writes. `derivatives/` is disposable and regenerable.
- **`effective_date` = `COALESCE(date_override, taken_at, file_mtime)`.** It is computed in the API, not stored, and every ordering mode sorts by it.
- **Client settings never reach the server.** The server has no client identity, no client table and no settings endpoint. This is deliberate; per-client server-side settings are listed as something not to build.
- **No server-side manifest cache.** Generate on request.
- **Config is environment variables only,** parsed once into a validated struct. Invalid config is a startup failure, never a silent default. TLS is terminated at the Ingress; the apps speak plain HTTP.

## Settled spec gaps

- **Delete and export run in the indexer.** Web inserts a row into `requests` and returns 202; the indexer does the work. Pending-delete photos are hidden from the manifest. Migration `0002`.
- **Status and retry state:** `meta` gained `last_scan_at`, `indexing_state`, `scan_total`, `scan_done`; per-photo backoff is the `derivative_failures` table. Queue depth is computed, not stored.
- **"Sidecar" wording** in the spec now says curation export.

- **`MAX_DECODE_BYTES` caps the input file size; `MAX_DECODE_PIXELS` bounds the decoded buffer** (3 bytes per pixel, so 80 MP is about 240 MB). Large originals such as 48 MP iPhone photos are accepted by design, since deriving manageable sizes is the indexer's job. Budget memory accordingly: a 48 MP job peaks around 450 MB (8-bit buffer, then the 16-bit linear copy), times `DERIVATIVE_WORKERS`.
- **Reconciliation refuses to delete rows on an incomplete listing.** An unreadable subdirectory aborts row deletion for that scan, and an empty `library/` with a populated index is only believed on the second consecutive scan. This guards against an unmounted NFS volume looking like "everything was deleted".
- **Uploads land in `incoming/<unique>/<original name>`** so the client's file name survives into `library/`; the indexer removes the emptied directory. Extensions in `library/` follow the detected format, never the upload name.
- **Rotation is authoritative curation** (`photos.rotation`, in the export), applied after EXIF orientation. `derivatives_rotation` (derived) says what is on disk; the manifest serves `media_rotation` and clients put it in the media URL (`?r=`), so URLs stay immutable. Derivative filenames carry `-r90` etc. The indexer prunes the old rotation's files only after the database points at the new ones. Rotation is relative (`PATCH {"rotate": 90}`).
- **Web writes go through `BEGIN IMMEDIATE` on SQLite.** Deferred transactions that read then write fail instantly with `SQLITE_BUSY_SNAPSHOT` under concurrency and the busy timeout cannot help. New write paths must use `begin_tx()`.

## Spec inconsistencies to settle with the user before building

- **Env var names.** Prose says `SCAN_INTERVAL` and `INGEST_QUIET_SECONDS`, but the config table says `SCAN_INTERVAL_SECS` and `INGEST_QUIET_SECS`; the table wins. `MANIFEST_POLL_INTERVAL` (default 300 s) appears only in prose. The spec's `§N` cross-references don't match any numbered headings, so use heading names.
