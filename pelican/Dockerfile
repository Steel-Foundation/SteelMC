FROM rustlang/rust:nightly-alpine3.23 AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY .. .

RUN cargo build --target x86_64-unknown-linux-musl --locked
RUN ls -lah && cd target && ls -lah && cd x86_64-unknown-linux-musl && ls -lah
#FROM gcr.io/distroless/cc-debian13
FROM alpine:3.23
RUN adduser --disabled-password --home /home/container container
USER container
ENV  USER=container HOME=/home/container
# COPY --from=builder /steel/target/release/steel /home/container/
# Podman don't support +x here
COPY --chmod=777 --from=builder /steel/target/x86_64-unknown-linux-musl/debug/steel /home/container/steel
WORKDIR /home/container

EXPOSE 25565

ENTRYPOINT ["/home/container/steel"]