/// <reference types="vitest/config" />
import { fileURLToPath, URL } from "node:url";
import { lingui } from "@lingui/vite-plugin";
import babel from "@rolldown/plugin-babel";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

// The API runs on :12000. In development it is proxied under /api so the
// session cookie and OAuth callbacks stay on the dashboard's origin, the same
// way production serves the dashboard and /api from one domain.
const apiTarget = process.env.NOALBS_API_URL ?? "http://localhost:12000";
// obs-link, proxied under /link (and /moblin) like production, so the browser source
// finds it on the page's own host.
const obsLinkTarget = process.env.NOALBS_OBS_LINK_URL ?? "http://localhost:11000";

/**
 * The same short URLs as Caddy: the browser source at /source and /source/, and
 * the privacy policy, the terms and the licenses at /privacy, /terms and /licenses.
 */
function shortUrls(): Plugin {
  return {
    name: "short-urls",
    configureServer(server) {
      server.middlewares.use((req, _res, next) => {
        if (req.url && /^\/source\/?(\?|$)/.test(req.url)) {
          req.url = req.url.replace(/^\/source\/?/, "/source/index.html");
        } else if (req.url && /^\/privacy\/?(\?|$)/.test(req.url)) {
          req.url = req.url.replace(/^\/privacy\/?/, "/privacy.html");
        } else if (req.url && /^\/terms\/?(\?|$)/.test(req.url)) {
          req.url = req.url.replace(/^\/terms\/?/, "/terms.html");
        } else if (req.url && /^\/licenses\/?(\?|$)/.test(req.url)) {
          req.url = req.url.replace(/^\/licenses\/?/, "/licenses.html");
        }
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [
    shortUrls(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
    // Compiles the `t`/`<Trans>` macros; `lingui()` loads .po catalogs on import.
    babel({ plugins: ["@lingui/babel-plugin-lingui-macro"] }),
    lingui(),
    tailwindcss(),
  ],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  // The dashboard, the static landing page Caddy serves at `/` to visitors
  // without a session, the privacy policy (`/privacy`), the terms (`/terms`) and the licenses (`/licenses`).
  build: {
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        landing: fileURLToPath(new URL("./landing.html", import.meta.url)),
        privacy: fileURLToPath(new URL("./privacy.html", import.meta.url)),
        terms: fileURLToPath(new URL("./terms.html", import.meta.url)),
        licenses: fileURLToPath(new URL("./licenses.html", import.meta.url)),
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: apiTarget,
        rewrite: (path) => path.replace(/^\/api/, ""),
      },
      "/link": { target: obsLinkTarget, ws: true },
      // Moblin's remote control connects to obs-link too.
      "/moblin": { target: obsLinkTarget, ws: true },
    },
    allowedHosts: ["localhost", "obelisk"],
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
