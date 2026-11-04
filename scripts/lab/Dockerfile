FROM rust:slim-bookworm AS build
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY frontend/dist frontend/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target \
    cargo build --release && cp target/release/savecodex /savecodex

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y gcc g++ make vim bash gawk sed grep findutils procps util-linux cron bc \
    poppler-utils graphviz libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /savecodex /usr/local/bin/savecodex
WORKDIR /work
ENTRYPOINT ["savecodex"]
