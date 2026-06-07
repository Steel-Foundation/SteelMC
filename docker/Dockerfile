FROM rustlang/rust:nightly-alpine3.23 AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY .. .
RUN cargo build --release --locked
#FROM alpine:3.23
FROM scratch

COPY --chmod=777 --from=builder /steel/target/release/steel /

EXPOSE 25565

ENTRYPOINT ["/steel"]