# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Shared architecture, invariants and commands are in `../../CLAUDE.md`; the full spec is `../../docs/SPEC.md`. This crate is the axum web tier (axum on tokio, tower-http, serde). The client it serves is in `../../web/`.

- Stateless. It reports `indexing` from the database, not from its own state, and serves whatever is already indexed during a scan or cold start.
- **Auth split:** only `upload`, `PATCH`/`DELETE /api/photos/{hash}`, `export` and `scan` require HTTP Basic. Favourite, tag and tag-create are deliberately unauthenticated because the frame is a kiosk with no keyboard. Auth is an `AuthProvider` trait with two implementations combined by `AnyOf` (either grants access): `BasicAuth` (constant-time comparison) and `CidrAuth` (`ADMIN_ALLOWED_CIDRS`). `CidrAuth` reads `X-Forwarded-For` only when the TCP peer is in `TRUSTED_PROXY_CIDRS`, walking from the right past trusted hops and failing closed on unparsable entries; the server therefore must be served with `into_make_service_with_connect_info`. `ADMIN_PASSWORD` is optional when CIDRs are set, but at least one is required. Do not implement OIDC. `/manage` itself is served unauthenticated, and the UI turns a 401 into a login form.
- **Manifest** (`GET /api/manifest`): generated per request, `ETag` derived from `generation`, and it includes only photos with `derivatives_ok = 1`. The client polls `/api/status` and refetches the manifest only when the generation moves.
- **`/media/{hash}/{variant}`** is content-addressed, so send `Cache-Control: public, max-age=31536000, immutable`. Support Range requests (the video seam).
- **Upload** streams to a `.part` file in `incoming/` and renames on completion, so the indexer's quiet-period check never sees a partial file. Enforce `MAX_UPLOAD_BYTES` during streaming rather than buffering, validate magic bytes, and respond immediately without waiting for indexing.
- Errors are RFC 9457 problem details and must never leak filesystem paths.
- Static assets from `web/dist` are embedded into the binary with `rust-embed`, so the client must be built before this crate.
- **Web-only env:** `BIND_ADDR`, `PUBLIC_HOSTNAME` (fixes the PWA manifest and service-worker origin; a home-screen install won't follow a change), `ADMIN_USERNAME`, `ADMIN_PASSWORD` (required unless `ADMIN_ALLOWED_CIDRS`), `ADMIN_ALLOWED_CIDRS`, `TRUSTED_PROXY_CIDRS`, `MAX_UPLOAD_BYTES`.
- **Tests:** route-level tests against in-memory SQLite.
