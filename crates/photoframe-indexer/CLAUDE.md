# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Shared architecture, invariants and commands are in `../../CLAUDE.md`; the full spec is `../../docs/SPEC.md`. This crate is the `photoframe-indexer` binary. Decoding and resizing live in `../imagepipe/`.

- **Must be safe to interrupt at any point;** the next scan reconciles whatever state was reached. On `SIGTERM`, finish or cancel in-flight derivative jobs, release the advisory lock and exit. It needs a generous `terminationGracePeriodSeconds`.
- **Polls, never watches:** `inotify` is unreliable on NFS. `SCAN_INTERVAL_SECS` defaults to 60, and the `meta` scan-request flag set by `POST /api/scan` starts a scan early.
- **Reconciliation:** recursively walk `library/`. A path already in the DB with matching size and mtime is skipped, and anything else is re-hashed. If the hash is already known, update `rel_path`; this is how a moved file keeps its tags and favourite. If the hash is new, extract metadata, insert the row and queue derivatives. A row whose path vanished has its row and derivatives deleted.
- **An empty database is a normal state** (fresh deploy, restore, backend migration). Do a full walk and rebuild; derivatives already on disk are detected by filename and not regenerated. Curation is not restored automatically.
- **Promotion from `incoming/`** (only when `INGEST_REQUIRE_SCAN=false`; when true, treat `incoming/` as read-only and leave it to the external scanner):
  1. Skip files younger than `INGEST_QUIET_SECS`.
  2. Validate by magic bytes, not extension or MIME.
  3. Enforce `MAX_UPLOAD_BYTES` and the pixel limits before decoding.
  4. Hash, and delete the incoming file if the hash is a duplicate.
  5. Move into `library/YYYY/MM/` by effective date, resolving collisions with `-1`, `-2`.
  6. Index and queue derivatives.

  A file that fails validation goes to `quarantine/` with a sibling `.reason.txt`, and is never silently deleted.
- **Derivative jobs:** a bounded queue with `DERIVATIVE_WORKERS` (default 2). Jobs are idempotent, keyed by hash plus variant, and a job whose output exists is a no-op. Failures are recorded and retried on later scans with exponential backoff, and a photo that keeps failing is excluded from the manifest rather than served broken.
- **Deletion order:** remove derivatives, remove the original, delete the row. See the root file on who triggers it.
- **Curation CLI:** `photoframe-indexer export --out <path>` (suitable for a CronJob) and `import --in <path>`. Import is a merge, never a replace: it adds tags and sets favourites and overrides, never clears anything, and skips hashes absent from the library without erroring.
- **Indexer-only env:** `SCAN_INTERVAL_SECS`, `INGEST_REQUIRE_SCAN`, `INGEST_QUIET_SECS`, `DERIVATIVE_WORKERS`, `MAX_DECODE_PIXELS` (80M), `MAX_DECODE_BYTES` (128 MiB), `DECODE_TIMEOUT_SECS` (30), `ENABLE_AVIF`.

## Tests the spec expects

- Scanner: integration tests over a temp directory covering new, moved, deleted and duplicate-content files and the empty-database rebuild.
- Curation export: a round-trip test, and a test that an empty database plus an export reconstructs tags and favourites exactly.
