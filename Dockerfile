FROM rust:latest AS chef
# TARGETARCH is auto-populated by buildkit/buildah from the build host's
# native platform unless --platform is passed explicitly -- so this resolves
# to amd64 on our Ubuntu dev/CI/prod boxes with zero change to how they
# invoke `docker build`/`podman build`, and to arm64 on Apple Silicon, and
# also supports genuine cross-builds via `--platform linux/amd64` etc.
ARG TARGETARCH
RUN apt-get update && apt-get install -y pkg-config python3-pip && rm -rf /var/lib/apt/lists/*
RUN pip install ziglang --break-system-packages
RUN cargo install cargo-chef cargo-zigbuild
# cargo-chef's "cook" step runs a plain `cargo build`, not `cargo zigbuild` --
# crates with C build scripts (e.g. ring) need a real cross compiler on PATH
# for that step; zigbuild's own compiler setup only applies to the final
# build below. CC goes through zig (not musl-tools' host-native musl-gcc) so
# this is a real cross compiler for either target regardless of the host's
# own arch, not just "host happens to match target".
RUN case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-musl; zigtriple=x86_64-linux-musl ;; \
      arm64) triple=aarch64-unknown-linux-musl; zigtriple=aarch64-linux-musl ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$triple" > /rust_target.txt; \
    rustup target add "$triple"; \
    echo "$zigtriple" > /zig_target.txt
# zig cc is clang-based, so cc-rs (e.g. building aws-lc-sys) detects it as
# clang and appends its own `--target=<rust-4-part-triple>` after our args --
# zig's own `-target` flag parser only understands its 3-part format and
# chokes on the 4-part one ("UnknownOperatingSystem"). Strip any
# caller-supplied -target/--target before forwarding so ours is the only one
# zig ever sees.
RUN cat > /usr/local/bin/musl-cc <<'WRAP'
#!/bin/bash
zt=$(cat /zig_target.txt)
args=()
skip=0
for a in "$@"; do
  if [ "$skip" = 1 ]; then skip=0; continue; fi
  case "$a" in
    --target=*) continue ;;
    -target) skip=1; continue ;;
  esac
  args+=("$a")
done
exec python3 -m ziglang cc -target "$zt" "${args[@]}"
WRAP
RUN chmod +x /usr/local/bin/musl-cc
ENV CC_x86_64_unknown_linux_musl=musl-cc
ENV AR_x86_64_unknown_linux_musl=ar
ENV CC_aarch64_unknown_linux_musl=musl-cc
ENV AR_aarch64_unknown_linux_musl=ar
WORKDIR /app

FROM chef AS planner
COPY . .
# --bin scopes the recipe to only what uc-server actually needs. Today
# uc-server pulls in nearly every workspace member anyway, but this keeps
# the build correctly scoped as the workspace grows instead of silently
# building unrelated crates.
RUN cargo chef prepare --bin uc-server --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --profile docker --bin uc-server --target "$(cat /rust_target.txt)" --recipe-path recipe.json
COPY . .
RUN cargo zigbuild --profile docker --target "$(cat /rust_target.txt)" -p uc-server
RUN cp "/app/target/$(cat /rust_target.txt)/docker/uc-server" /uc-server-bin

FROM scratch
COPY --from=builder /uc-server-bin /uc-server
ENTRYPOINT ["/uc-server"]