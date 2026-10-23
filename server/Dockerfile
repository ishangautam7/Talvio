# Pinned to the Bun the server is tested with.
FROM oven/bun:1.4.3-alpine

WORKDIR /app
# No runtime dependencies, so there is nothing to install.
COPY package.json ./
COPY src ./src

RUN mkdir -p /data && chown bun:bun /data
USER bun

ENV DATABASE_PATH=/data/zynveliq.db
VOLUME /data
EXPOSE 8787
CMD ["bun", "src/main.ts"]
