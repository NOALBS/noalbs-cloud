# The dashboard (a static SPA) and the OBS browser source, served by Caddy.
# Caddy also proxies /api to the API and /link to obs-link, so one domain is
# enough; see deploy/README.md. Build context: web/dashboard.

FROM node:24-slim AS build
WORKDIR /app
RUN npm install --global pnpm@12.3.4
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
RUN pnpm run build

FROM caddy:2-alpine
COPY Caddyfile /etc/caddy/Caddyfile
COPY --from=build /app/dist /srv

# SITE_ADDRESS: ":80" behind your own TLS proxy, or a domain for automatic HTTPS.
ENV SITE_ADDRESS=":80" \
    API_UPSTREAM="api:12000" \
    OBS_LINK_UPSTREAM="obs-link:11000" \
    CHAT_UPSTREAM="chat:13000"
