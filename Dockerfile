FROM rust:latest AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev musl-tools && rm -rf /var/lib/apt/lists/*
RUN rustup target add aarch64-unknown-linux-musl
COPY . .
RUN cargo build --release --target aarch64-unknown-linux-musl -p uc-server

FROM scratch
COPY --from=builder /app/target/aarch64-unknown-linux-musl/release/uc-server /uc-server
ENTRYPOINT ["/uc-server"]
