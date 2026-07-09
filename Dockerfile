# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM rust:alpine AS chef
# TARGETARCH: auto-set by buildkit/buildah from host arch unless --platform is passed.
ARG TARGETARCH
RUN apk add --no-cache build-base bash pkgconf python3 py3-pip
RUN pip install ziglang --break-system-packages
RUN cargo install cargo-chef cargo-zigbuild
# cook must use --zigbuild too, or its fingerprint won't match the final build and everything recompiles twice.
RUN case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-musl; zigtriple=x86_64-linux-musl ;; \
      arm64) triple=aarch64-unknown-linux-musl; zigtriple=aarch64-linux-musl ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$triple" > /rust_target.txt; \
    rustup target add "$triple"; \
    echo "$zigtriple" > /zig_target.txt
# cc-rs detects zig cc/c++ as clang and appends its own --target=<rust-triple>,
# which breaks zig's -target parser. Strip those flags via a wrapper so any
# future C dependency's buildsystem links correctly against musl.
#
# One wrapper PER triple (not a single script reading a shared /zig_target.txt):
# build scripts and proc-macro crates (e.g. sqlx-macros) always compile for the
# HOST triple (x86_64, since BUILDPLATFORM pins this stage to the native runner)
# regardless of the cross --target passed for the final binary. A single global
# target file can't distinguish "CC invoked because it's the host triple" from
# "CC invoked because it's the cross target" -- when cross-compiling to arm64 it
# wrongly compiled host-triple C code (e.g. libsqlite3-sys for sqlx-macros) for
# aarch64 too, which then failed to link into the host-native x86_64 proc-macro
# artifact ("file in wrong format").
RUN <<'SETUP'
set -eu
for pair in "x86_64:x86_64-linux-musl" "aarch64:aarch64-linux-musl"; do
  rt="${pair%%:*}"; zt="${pair##*:}"
  for tool in cc:cc g++:c++; do
    name="musl-${tool%%:*}-${rt}"; zigcmd="${tool##*:}"
    cat > "/usr/local/bin/${name}" <<WRAP
#!/bin/bash
args=()
skip=0
for a in "\$@"; do
  if [ "\$skip" = 1 ]; then skip=0; continue; fi
  case "\$a" in
    --target=*) continue ;;
    -target) skip=1; continue ;;
  esac
  args+=("\$a")
done
exec python3 -m ziglang ${zigcmd} -target ${zt} "\${args[@]}"
WRAP
    chmod +x "/usr/local/bin/${name}"
  done
done
SETUP
ENV CC_x86_64_unknown_linux_musl=musl-cc-x86_64
ENV CXX_x86_64_unknown_linux_musl=musl-g++-x86_64
ENV AR_x86_64_unknown_linux_musl=ar
ENV CC_aarch64_unknown_linux_musl=musl-cc-aarch64
ENV CXX_aarch64_unknown_linux_musl=musl-g++-aarch64
ENV AR_aarch64_unknown_linux_musl=ar
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --bin uc-server --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Own target/ cache id (separate Cargo.lock from aispecs/operator); registry cache id is
# shared globally. The "-alpine" suffix ties this cache to the builder base image: compiled
# .rlib/.so artifacts are toolchain/ABI-specific, so reusing a cache built under a different
# base (e.g. glibc rust:latest) causes relocation failures like a missing __ubsan_handle_*
# symbol. Bump this suffix again if the builder base image changes.
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/app/target,id=cargo-target-uc-alpine \
    cargo chef cook --zigbuild --profile docker --bin uc-server --target "$(cat /rust_target.txt)" --recipe-path recipe.json
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry \
    --mount=type=cache,target=/app/target,id=cargo-target-uc-alpine \
    cargo zigbuild --profile docker --target "$(cat /rust_target.txt)" -p uc-server && \
    cp "/app/target/$(cat /rust_target.txt)/docker/uc-server" /uc-server-bin

FROM scratch
COPY --from=builder /uc-server-bin /uc-server
ENTRYPOINT ["/uc-server"]
