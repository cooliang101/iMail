FROM node:24-bookworm-slim AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build:remote

FROM node:24-bookworm-slim AS runtime
ENV NODE_ENV=production \
    HOST=0.0.0.0 \
    PORT=8787 \
    IMAIL_DATA_DIR=/data \
    IMAIL_BACKUP_DIR=/backups \
    IMAIL_WEB_DIST=/app/dist \
    IMAIL_WORKER_ENTRY=/app/server-runtime/imail-worker.cjs
WORKDIR /app
COPY --from=build /app/dist ./dist
COPY --from=build /app/server-runtime ./server-runtime
RUN useradd --system --uid 10001 --create-home imail && mkdir -p /data /backups && chown imail:imail /data /backups
USER imail
VOLUME ["/data", "/backups"]
EXPOSE 8787
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 CMD ["node", "-e", "fetch('http://127.0.0.1:8787/api/system/info').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))"]
CMD ["node", "server-runtime/imail-server.cjs"]
