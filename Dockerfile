# syntax=docker/dockerfile:1

# --- Rust binaries ---
FROM rust:1.86-bookworm AS builder-rust
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY apps apps
COPY crates crates
COPY packages packages
RUN cargo build --release -p bunny-server -p bunny-discord-bridge

# --- Web UI ---
FROM node:20-bookworm AS builder-web
WORKDIR /build
COPY apps/web/package.json apps/web/package-lock.json ./apps/web/
COPY packages/i18n ./packages/i18n
COPY apps/web ./apps/web
RUN cd apps/web && npm ci --no-fund --no-audit && npm run build

# --- Node sidecars ---
FROM node:20-bookworm AS builder-sidecars
WORKDIR /build
COPY apps/server/webrtc-sidecar/package.json apps/server/webrtc-sidecar/package-lock.json ./apps/server/webrtc-sidecar/
COPY apps/server/cdp-sidecar/package.json apps/server/cdp-sidecar/package-lock.json ./apps/server/cdp-sidecar/
RUN cd apps/server/webrtc-sidecar && npm ci --omit=dev --no-fund --no-audit \
 && cd ../cdp-sidecar && npm ci --omit=dev --no-fund --no-audit
COPY apps/server/webrtc-sidecar ./apps/server/webrtc-sidecar
COPY apps/server/cdp-sidecar ./apps/server/cdp-sidecar

# --- Runtime ---
FROM ubuntu:24.04 AS runtime
ENV DEBIAN_FRONTEND=noninteractive
ENV BUNNY_INSTALL_DIR=/opt/bunny

RUN apt-get update -qq \
 && apt-get install -y --no-install-recommends curl ca-certificates git tmux neovim \
    xvfb x11vnc websockify novnc \
 && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
 && apt-get install -y nodejs \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/bunny

COPY --from=builder-rust /build/target/release/bunny ./bin/bunny
COPY --from=builder-rust /build/target/release/bunny-discord-bridge ./bin/bunny-discord-bridge
COPY --from=builder-web /build/apps/web/dist ./share/bunny/web/dist
COPY --from=builder-sidecars /build/apps/server/webrtc-sidecar ./share/bunny/webrtc-sidecar
COPY --from=builder-sidecars /build/apps/server/cdp-sidecar ./share/bunny/cdp-sidecar
COPY scripts/docker-entrypoint.sh scripts/install-runtime.sh ./scripts/
RUN chmod +x ./scripts/docker-entrypoint.sh ./scripts/install-runtime.sh ./bin/*

# Playwright Chromium for browser tab
RUN cd share/bunny/webrtc-sidecar \
 && npx playwright install chromium \
 && npx playwright install-deps chromium \
 && playwright_chrome="$(find /root/.cache/ms-playwright -path '*/chromium-*/chrome-linux*/chrome' -type f 2>/dev/null | sort -V | tail -1)" \
 && ln -sf "$playwright_chrome" /usr/local/bin/chromium

ENV PATH="/opt/bunny/bin:/usr/local/bin:${PATH}"

EXPOSE 7681
ENTRYPOINT ["/opt/bunny/scripts/docker-entrypoint.sh"]
CMD ["sleep", "infinity"]
