FROM rustlang/rust:nightly-alpine3.23 AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY .. .
RUN cargo build --target x86_64-unknown-linux-musl --release --locked
FROM alpine:3.23

COPY --chmod=777 --from=builder /steel/target/release/steel /

EXPOSE 25565

ENTRYPOINT ["/steel"]