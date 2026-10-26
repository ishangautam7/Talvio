# Pinned to the Bun the server is tested with.
FROM oven/bun:1.4.3-alpine

WORKDIR /app
# No runtime dependencies, so there is nothing to install.
COPY package.json ./
COPY src ./src
# Reads the free credit ledger: `docker exec talvio-server bun scripts/free-usage.ts`.
COPY scripts/free-usage.ts ./scripts/

RUN mkdir -p /data && chown bun:bun /data
USER bun

# A volume from before the rename to Talvio holds zynveliq.db; the server
# opens that one while no talvio.db exists (src/databasePath.ts).
ENV DATABASE_PATH=/data/talvio.db
VOLUME /data
EXPOSE 8787
CMD ["bun", "src/main.ts"]
