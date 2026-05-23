FROM rustlang/rust:nightly-bookworm AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY . .

RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /steel/target/release/steel /steel/steel-bin

ENTRYPOINT ["/steel/steel-bin"]