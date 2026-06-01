FROM rustlang/rust:nightly-bookworm AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

# COPY . .
# RUN cargo build --release --locked
RUN wget https://github.com/Steel-Foundation/SteelMC/releases/download/v0.8.0%2Bmc26.1/steel-linux && chmod +x ./steel-linux

FROM gcr.io/distroless/cc-debian13

COPY --from=builder /steel/steel-linux /

EXPOSE 25565

ENTRYPOINT ["/steel-linux"]