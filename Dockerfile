# syntax=docker/dockerfile:1

ARG RUST_IMAGE=rust:1.92-bookworm
FROM ${RUST_IMAGE} AS kernel-build

WORKDIR /src

COPY apps/kernel/Cargo.toml apps/kernel/Cargo.lock apps/kernel/
COPY apps/kernel/src apps/kernel/src

RUN cargo build --locked --release --manifest-path apps/kernel/Cargo.toml --bin qingyu-kernel

# Phase P3 intentionally exports only a build artifact. The current
# qingyu-kernel "serve" command is a native-child protocol and is not a
# container/server entrypoint. A runnable runtime stage must be added only
# after P2 supplies the server bootstrap and initialization-token lifecycle.
FROM scratch AS kernel-artifact

LABEL dev.qingyu.image.kind="kernel-artifact-only"

COPY --from=kernel-build /src/apps/kernel/target/release/qingyu-kernel /qingyu-kernel
