FROM rust:1.73-bookworm as builder

WORKDIR /usr/src/app

RUN apt-get update && apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

run protoc --version

COPY . .

RUN cargo install --path .

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 -U -s /bin/sh -d /app app

WORKDIR /app
USER app

COPY --from=builder /usr/src/app/target/release/grpcalc /app/grpcalc

EXPOSE 50051

ENTRYPOINT ["/app/grpcalc"]
