# Two images from one multi-stage build:
#   docker build --target web     -t photoframe-web .
#   docker build --target indexer -t photoframe-indexer .

FROM node:lts-bookworm-slim AS client
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY migrations migrations
# rust-embed picks the client up from web/dist at compile time.
COPY --from=client /web/dist web/dist
RUN cargo build --release -p photoframe-web -p photoframe-indexer

# Debian 12 base matches the builder's glibc. Non-root; the only writable
# paths are the mounts.
FROM gcr.io/distroless/cc-debian12:nonroot AS web
COPY --from=build /src/target/release/photoframe-web /photoframe-web
EXPOSE 8080
ENTRYPOINT ["/photoframe-web"]

FROM gcr.io/distroless/cc-debian12:nonroot AS indexer
COPY --from=build /src/target/release/photoframe-indexer /photoframe-indexer
ENTRYPOINT ["/photoframe-indexer"]
