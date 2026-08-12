FROM node:24-bookworm-slim AS web-build
ENV NODE_OPTIONS=--max-old-space-size=768
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build:web

FROM rust:1.77.2-bookworm AS rust-build
ENV CARGO_BUILD_JOBS=1 \
    CARGO_INCREMENTAL=0
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY http-service/Cargo.toml ./http-service/Cargo.toml
COPY http-service/src ./http-service/src
COPY contracts ./contracts
RUN cargo build --locked --release -p imail-http-service --bin imail-server \
    && cargo build --locked --release -p imail-storage-sqlite --bin imail-maintenance

FROM debian:bookworm-slim AS runtime
ENV IMAIL_DATA_DIR=/data \
    IMAIL_WEB_DIST=/app/dist \
    IMAIL_SYNC_WORKER=true \
    IMAIL_ALLOWED_HOSTS=localhost,127.0.0.1,::1
WORKDIR /app
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 imail \
    && useradd --system --uid 10001 --gid 10001 --no-create-home imail \
    && mkdir -p /data /backups \
    && chown 10001:10001 /data /backups
COPY --from=web-build /app/frontend/dist ./dist
COPY --from=rust-build /app/target/release/imail-server ./imail-server
COPY --from=rust-build /app/target/release/imail-maintenance ./imail-maintenance
USER 10001:10001
VOLUME ["/data", "/backups"]
EXPOSE 8787
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 CMD ["curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:8787/api/health"]
CMD ["/app/imail-server", "--host", "0.0.0.0", "--port", "8787"]
