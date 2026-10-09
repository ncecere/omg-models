# syntax=docker/dockerfile:1

# Build: compile the CLI (which embeds the Topcoat web app), bundle its
# assets (Tailwind CSS, Inter, logo) next to it, and check the data. The
# Tailwind build script downloads Topcoat's pinned Tailwind CLI release, so
# the build stage needs network access to github.com.
FROM rust:1.99.0-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS build
WORKDIR /build
ARG CARGO_BUILD_JOBS=2
# The asset bundler must match the topcoat crate version (=0.10.0).
RUN cargo install topcoat-cli --version =0.10.0 --locked --jobs "$CARGO_BUILD_JOBS"
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY data/ data/
RUN cargo build --locked --release --jobs "$CARGO_BUILD_JOBS" -p omg-models \
 && topcoat asset bundle --release --bin omg-models --out /build/assets \
 && ./target/release/omg-models validate --data data \
 && ./target/release/omg-models build --data data --out /build/dist

# Runtime: distroless (no shell, package manager or curl). The binary links
# glibc and libgcc_s, so `cc`. Same pinned index digest as the gateway image;
# refresh deliberately with
#   docker buildx imagetools inspect gcr.io/distroless/cc-debian12:nonroot
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f AS runtime
WORKDIR /app
COPY --from=build --chmod=0555 /build/target/release/omg-models /usr/local/bin/omg-models
# The asset bundle must come from the same build as the binary.
COPY --from=build /build/assets/ /app/assets/
# The data is baked in at build time; the server validates it and builds the
# JSON API in memory at startup (byte-identical to /app/dist from `build`).
COPY --from=build /build/data/ /app/data/
COPY --from=build /build/dist/ /app/dist/
COPY LICENSE NOTICE.md /usr/share/doc/omg-models/
COPY crates/web/assets/fonts/OFL.txt /usr/share/doc/omg-models/inter-OFL.txt
ENV OMG_MODELS_DATA=/app/data \
    OMG_MODELS_ASSETS=/app/assets \
    OMG_MODELS_LISTEN=0.0.0.0:8080 \
    HOME=/nonexistent
# UID/GID 10001, like the Open Model Gateway image. Nothing is written at
# runtime, so the root filesystem can be mounted read-only.
USER 10001:10001
EXPOSE 8080
STOPSIGNAL SIGTERM
# Exec form; probes /healthz over loopback on OMG_MODELS_LISTEN's port.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/omg-models", "healthcheck", "--timeout", "3"]
ENTRYPOINT ["/usr/local/bin/omg-models"]
CMD ["serve"]
