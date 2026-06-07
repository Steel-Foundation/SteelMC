FROM rustlang/rust:nightly-alpine3.23 AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

RUN apk add --no-cache lld

ENV RUSTFLAGS="-Zthreads=0 -Clink-arg=-fuse-ld=lld"

COPY . .
RUN cargo build --release --locked

FROM alpine:3.23 AS pelican

RUN adduser --disabled-password --home /home/container container
USER container
ENV USER=container HOME=/home/container

COPY --chmod=777 --from=builder /steel/target/release/steel /home/container/steel
WORKDIR /home/container

EXPOSE 25565

FROM alpine:3.23 AS normal

COPY --chmod=777 --from=builder /steel/target/release/steel /

EXPOSE 25565

ENTRYPOINT ["/steel"]
