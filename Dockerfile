# Build stage
FROM rust:1-bookworm AS builder
WORKDIR /app

# Copy workspace and crate manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY crates ./crates

# Build release binary (root package = k-paper)
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/k-paper /app/k-paper

ENV PAPER_EXCHANGE_PORT=19999
ENV PAPER_EXCHANGE_DUMMY_OB=1

EXPOSE 19999

CMD ["/app/k-paper"]
