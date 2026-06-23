FROM rust:latest AS chef
RUN apt-get update && apt-get install -y pkg-config python3-pip musl-tools && rm -rf /var/lib/apt/lists/*
RUN pip install ziglang --break-system-packages
RUN cargo install cargo-chef cargo-zigbuild
RUN rustup target add x86_64-unknown-linux-musl
# cargo-chef's "cook" step runs a plain `cargo build`, not `cargo zigbuild` --
# crates with C build scripts (e.g. ring) need a real musl cross-cc on PATH
# for that step, zigbuild's env setup only applies to the final build below.
ENV CC_x86_64_unknown_linux_musl=musl-gcc
ENV AR_x86_64_unknown_linux_musl=ar
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --profile docker --target x86_64-unknown-linux-musl --recipe-path recipe.json
COPY . .
RUN cargo zigbuild --profile docker --target x86_64-unknown-linux-musl -p uc-server

FROM scratch
COPY --from=builder /app/target/x86_64-unknown-linux-musl/docker/uc-server /uc-server
ENTRYPOINT ["/uc-server"]
