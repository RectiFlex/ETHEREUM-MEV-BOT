# Build stage
FROM rust:1.70 as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build for release
RUN cargo build --release

# Runtime stage
FROM debian:bullseye-slim

# Install CA certificates for HTTPS
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/mev-template /app/mev-bot

# Create non-root user
RUN useradd -m -u 1001 mevbot && \
    chown -R mevbot:mevbot /app

USER mevbot

# Run the bot
CMD ["./mev-bot"] 