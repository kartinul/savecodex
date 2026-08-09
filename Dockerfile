FROM node:20-slim AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json* ./
# Since bun.lock is present, we can just use npm to install using package.json, or bun if we install it
RUN npm install
COPY frontend/ ./
RUN npm run build

FROM rust:1.80-slim AS backend-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
# Copy the built frontend into the rust-embed directory if needed
# Wait, rust-embed looks in the filesystem at compile time.
# We need to build the frontend first, then copy it to the rust builder BEFORE compiling rust!
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist
RUN cargo build --release --workspace

FROM debian:bookworm-slim
WORKDIR /app
# Install common languages for savecodex to run
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
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend-builder /app/target/release/savecodex /usr/local/bin/savecodex

# Render provides the PORT environment variable
ENV PORT=10000
EXPOSE ${PORT}

CMD ["sh", "-c", "savecodex serve --host 0.0.0.0 --port ${PORT}"]
