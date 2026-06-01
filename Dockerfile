FROM rustlang/rust:nightly-bookworm AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY . .
RUN cargo build --release --locked
FROM gcr.io/distroless/cc-debian13

COPY --from=builder /steel/target/release/steel /

EXPOSE 25565

ENTRYPOINT ["/steel"]