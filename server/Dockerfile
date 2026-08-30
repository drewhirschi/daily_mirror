FROM rust:1-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends nodejs npm \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .
RUN node .nextrs/ensure-client.mjs \
    && npm ci \
    && npm run client:generate \
    && cargo build --release --bin server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /app/data/photos

WORKDIR /app
COPY --from=builder /build/target/release/server /app/server
COPY --from=builder /build/public /app/public

ENV PORT=3000
ENV NEXTRS_PUBLIC_DIR=/app/public
ENV DAILY_MIRROR_STORAGE_DIR=/app/data/photos
VOLUME ["/app/data"]
EXPOSE 3000

CMD ["/app/server"]
