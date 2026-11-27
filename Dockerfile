# Build stage — pin matches rust-toolchain.toml
FROM rust:1.98.1-bookworm AS builder
WORKDIR /app

COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY src ./src
COPY crates ./crates
COPY config ./config

RUN cargo build --release --bin k-paper

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/k-paper /app/k-paper
COPY config ./config

ENV PAPER_EXCHANGE_PORT=19999
ENV PAPER_EXCHANGE_HOST=127.0.0.1
ENV PAPER_EXCHANGE_DUMMY_OB=1
ENV PAPER_EXCHANGE_CONFIG=/app/config/instruments.yaml

EXPOSE 19999

CMD ["/app/k-paper"]
