FROM rustlang/rust:nightly-alpine3.23 AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY . .
RUN cargo build --release --locked --features stand-alone

FROM scratch
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --chmod=755 --from=builder /steel/target/release/steel /

EXPOSE 25565

ENTRYPOINT ["/steel"]
