FROM rust:1.82-bookworm AS builder

WORKDIR /app

# Copy dependency manifests first for layer caching
COPY ignite-pay-core/Cargo.toml ignite-pay-core/Cargo.lock ignite-pay-core/
COPY didcomm-router/Cargo.toml didcomm-router/Cargo.lock didcomm-router/

# Create dummy libs to cache dependencies
RUN mkdir -p ignite-pay-core/src && echo "" > ignite-pay-core/src/lib.rs \
 && mkdir -p didcomm-router/src && echo "fn main(){}" > didcomm-router/src/main.rs \
 && cd didcomm-router && cargo build --release 2>/dev/null || true

# Copy actual source code
COPY ignite-pay-core/src ignite-pay-core/src
COPY didcomm-router/src didcomm-router/src

# Touch source to invalidate cached dummy build
RUN find ignite-pay-core/src didcomm-router/src -type f -exec touch {} +

# Build the real binary
RUN cd didcomm-router && cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/didcomm-router/target/release/didcomm-router /app/didcomm-router

# Default config — override via volume mount
COPY didcomm-router/config.toml /app/config.toml

# Create data and certs directories
RUN mkdir -p /app/data /app/certs

EXPOSE 8080

ENV RUST_LOG=didcomm_router=info

ENTRYPOINT ["/app/didcomm-router"]
CMD ["config.toml"]
