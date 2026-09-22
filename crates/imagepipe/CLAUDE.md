# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Shared architecture, invariants and commands are in `../../CLAUDE.md`; the full spec is `../../docs/SPEC.md`. The scanner that drives this crate is `../photoframe-indexer/`.

This crate decodes untrusted bytes, so it is **entirely safe Rust with no C dependencies**. This is a security property; do not add a C-backed decoder. It has no I/O and no dependency on axum, sqlx or the filesystem: bytes in, bytes out. That keeps it fuzzable, and the weekly `cargo fuzz` job targets its decode entry points.

- **`Decoder` trait** so implementations can be swapped. Crates: `zune-jpeg` (JPEG), `image` (PNG, WebP decode-only), `heic-rs` (HEIC), `rav1d` via `image` (AVIF). **Avoid Imazen's `heic` crate (AGPL).** A `libheif` fallback goes behind a Cargo feature only if a real photo refuses to decode; do not add it pre-emptively.
- **Limits are enforced before allocation:** pixels (bounds the decoded buffer), input bytes (bounds the file; big originals are expected) and a per-job wall-clock timeout. A file over any limit is quarantined, not decoded.
- **Metadata:** `kamadak-exif` for `DateTimeOriginal`, `Orientation` and dimensions. Apply orientation while generating derivatives so every derivative is upright and the client never rotates. **Strip all metadata from derivatives** (GPS especially). Originals stay untouched.
- **Derivatives:** `fast_image_resize` with Lanczos3 in linear light. Variants are `display` (2560 px, AVIF plus JPEG fallback), `preview` (1280 px), `thumb` (400 px, JPEG) and `blur` (64 px, JPEG). Never upscale; a smaller source is copied at native size. Encoders are `rav1e` and `jpeg-encoder`. Dropping AVIF for JPEG-only is an accepted retreat if encoding is too slow. Derivative paths (`derivatives/<h0h1>/<h2h3>/<hash>-<variant>.<ext>`) are the indexer's concern, not this crate's.
- **Tests:** unit tests per format including a corpus of real iPhone HEIC files, and tests that oversized input is rejected before allocating.
