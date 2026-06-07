FROM rustlang/rust:nightly-alpine3.23 AS builder
LABEL authors="junkydeveloper"

WORKDIR /steel

COPY . .
RUN cargo build --release --locked

FROM alpine:3.23 AS pelican

RUN adduser --disabled-password --home /home/container container
USER container
ENV USER=container HOME=/home/container

COPY --chmod=777 --from=builder /steel/target/release/steel /bin/steel
COPY docker/entrypoint-pelican.sh /entrypoint-pelican.sh
WORKDIR /home/container

EXPOSE 25565

CMD ["/bin/ash", "/entrypoint-pelican.sh"]

FROM alpine:3.23 AS normal

COPY --chmod=777 --from=builder /steel/target/release/steel /

EXPOSE 25565

ENTRYPOINT ["/steel"]
