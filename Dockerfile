FROM node:20-slim AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json* ./
RUN npm install
COPY frontend/ ./
RUN npm run build

FROM rust:slim AS backend-builder
WORKDIR /app
# openssl-sys needs pkg-config + OpenSSL dev headers to build on Debian-based rust:slim
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
# Copy the built frontend into the rust-embed directory before compiling
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist
RUN cargo build --release --workspace

FROM debian:bookworm-slim
WORKDIR /app
# Install common languages for savecodex to run, plus libssl3 for the dynamically-linked binary
RUN apt-get update && apt-get install -y \
    python3 \
    nodejs \
    gcc \
    g++ \
    clang \
    make \
    curl \
    default-jdk \
    golang \
    php-cli \
    ruby \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend-builder /app/target/release/savecodex /usr/local/bin/savecodex

# Render provides the PORT environment variable
ENV PORT=10000
EXPOSE ${PORT}

CMD ["sh", "-c", "savecodex serve --host 0.0.0.0 --port ${PORT}"]