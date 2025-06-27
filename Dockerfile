# syntax=docker/dockerfile:1
ARG BUILDKIT_SBOM_SCAN_CONTEXT=true
ARG BUILDKIT_SBOM_SCAN_STAGE=planner,builder

# Cross-compiling using Docker multi-platform builds/images and `xx`.
#
# https://docs.docker.com/build/building/multi-platform/
# https://github.com/tonistiigi/xx
FROM --platform=${BUILDPLATFORM:-linux/amd64} tonistiigi/xx AS xx

# Utilizing Docker layer caching with `cargo-chef`.
#
# https://www.lpalmieri.com/posts/fast-rust-docker-builds/
FROM --platform=${BUILDPLATFORM:-linux/amd64} lukemathwalker/cargo-chef:latest-rust-1.83.0 AS chef


FROM chef AS planner
WORKDIR /smoldb
COPY . .
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder
WORKDIR /smoldb

COPY --from=xx / /

# Relative order of `ARG` and `RUN` commands in the Dockerfile matters.
#
# If you pass a different `ARG` to `docker build`, it would invalidate Docker layer cache
# for the next steps. (E.g., the following steps may depend on a new `ARG` value, so Docker would
# have to re-execute them instead of using a cached layer from a previous run.)
#
# Steps in this stage are ordered in a way that should maximize Docker layer cache utilization,
# so, please, don't reorder them without prior consideration. 🥲

RUN apt-get update \
    && apt-get install --no-install-recommends -y clang lld \
    && rustup component add rustfmt

# `ARG`/`ENV` pair is a workaround for `docker build` backward-compatibility.
#
# https://github.com/docker/buildx/issues/510
ARG BUILDPLATFORM
ENV BUILDPLATFORM=${BUILDPLATFORM:-linux/amd64}

ARG MOLD_VERSION=2.3.2

RUN case "$BUILDPLATFORM" in \
        */amd64 ) PLATFORM=x86_64 ;; \
        */arm64 | */arm64/* ) PLATFORM=aarch64 ;; \
        * ) echo "Unexpected BUILDPLATFORM '$BUILDPLATFORM'" >&2; exit 1 ;; \
    esac; \
    \
    mkdir -p /opt/mold; \
    \
    TARBALL="mold-$MOLD_VERSION-$PLATFORM-linux.tar.gz"; \
    curl -sSL -o "/opt/mold/$TARBALL" "https://github.com/rui314/mold/releases/download/v$MOLD_VERSION/$TARBALL"; \
    tar -xf "/opt/mold/$TARBALL" --strip-components 1 -C /opt/mold; \
    rm "/opt/mold/$TARBALL"

# `ARG`/`ENV` pair is a workaround for `docker build` backward-compatibility.
#
# https://github.com/docker/buildx/issues/510
ARG TARGETPLATFORM
ENV TARGETPLATFORM=${TARGETPLATFORM:-linux/amd64}

RUN xx-apt-get install -y pkg-config gcc g++ libssl-dev

# Select Cargo profile (e.g., `release` or `dev`)
ARG PROFILE=release

# Enable crate features
ARG FEATURES

# Pass custom `RUSTFLAGS` (e.g., `--cfg tokio_unstable` to enable Tokio tracing/`tokio-console`)
ARG RUSTFLAGS

# Select linker (e.g., `mold`, `lld` or an empty string for the default linker)
ARG LINKER=mold

COPY --from=planner /smoldb/recipe.json recipe.json
# `PKG_CONFIG=...` is a workaround for `xx-cargo` bug for crates based on `pkg-config`!
#
# https://github.com/tonistiigi/xx/issues/107
# https://github.com/tonistiigi/xx/pull/108
RUN PKG_CONFIG="/usr/bin/$(xx-info)-pkg-config" \
    PATH="$PATH:/opt/mold/bin" \
    RUSTFLAGS="${LINKER:+-C link-arg=-fuse-ld=}$LINKER $RUSTFLAGS" \
    xx-cargo chef cook --profile $PROFILE ${FEATURES:+--features} $FEATURES --recipe-path recipe.json

COPY . .
# `PKG_CONFIG=...` is a workaround for `xx-cargo` bug for crates based on `pkg-config`!
#
# https://github.com/tonistiigi/xx/issues/107
# https://github.com/tonistiigi/xx/pull/108
RUN PKG_CONFIG="/usr/bin/$(xx-info)-pkg-config" \
    PATH="$PATH:/opt/mold/bin" \
    RUSTFLAGS="${LINKER:+-C link-arg=-fuse-ld=}$LINKER $RUSTFLAGS" \
    xx-cargo build --profile $PROFILE ${FEATURES:+--features} $FEATURES --bin smoldb \
    && PROFILE_DIR=$(if [ "$PROFILE" = dev ]; then echo debug; else echo $PROFILE; fi) \
    && mv "target/$(xx-cargo --print-target-triple)/$PROFILE_DIR/smoldb" /smoldb/smoldb

FROM gcr.io/distroless/cc-debian12 AS smoldb

COPY --from=builder /smoldb/smoldb /smoldb

USER 0

ENV TZ=Etc/UTC \
    RUN_MODE=production

EXPOSE 9900
EXPOSE 9910

LABEL org.opencontainers.image.title="Smol DB"
LABEL org.opencontainers.image.description="A smol database implemented from scratch in Rust"
LABEL org.opencontainers.image.url="https://github.com/kshivendu/smoldb"
LABEL org.opencontainers.image.documentation="https://github.com/kshivendu/smoldb"
LABEL org.opencontainers.image.source="https://github.com/kshivendu/smoldb"
LABEL org.opencontainers.image.vendor="kshivendu"

ENTRYPOINT ["/smoldb"]

# docker build --network=host -t kshivendu/smoldb:latest .
# docker run -p 9900:9900 -p 9910:9910 kshivendu/smoldb:latest
# docker run -p 9900:9900 -p 9910:9910 kshivendu/smoldb:latest
