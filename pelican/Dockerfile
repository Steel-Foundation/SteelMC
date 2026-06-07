FROM rustlang/rust:nightly-alpine3.23 AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY .. .

RUN cargo build --release --target x86_64-unknown-linux-musl --locked

FROM alpine:3.23
RUN adduser --disabled-password --home /home/container container
USER container
ENV  USER=container HOME=/home/container

# Podman don't support +x here
COPY --chmod=777 --from=builder /steel/target/x86_64-unknown-linux-musl/release/steel /home/container/steel
WORKDIR /home/container

EXPOSE 25565

ENTRYPOINT ["/home/container/steel"]