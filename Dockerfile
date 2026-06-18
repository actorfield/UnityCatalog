FROM rust:latest AS chef
RUN apt-get update && apt-get install -y pkg-config python3-pip && rm -rf /var/lib/apt/lists/*
RUN pip install ziglang --break-system-packages
RUN cargo install cargo-chef cargo-zigbuild
RUN rustup target add aarch64-unknown-linux-musl
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --profile docker --target aarch64-unknown-linux-musl --recipe-path recipe.json
COPY . .
RUN cargo zigbuild --profile docker --target aarch64-unknown-linux-musl -p uc-server

FROM scratch
COPY --from=builder /app/target/aarch64-unknown-linux-musl/docker/uc-server /uc-server
ENTRYPOINT ["/uc-server"]
