import { defineConfig } from "vite";

// VOLUND_API_PROXY points the dev server at another volundd, e.g. an SSH tunnel
// to a test instance. The default is the local daemon.
const apiTarget = process.env.VOLUND_API_PROXY ?? "http://127.0.0.1:8080";
// Set VOLUND_API_SECURE_COOKIES=true when that instance runs with secure cookies:
// it then expects an https Origin, so the dev server's own http Origin is
// rewritten to https. Foreign origins pass through unchanged and stay rejected.
const secureCookieTarget = process.env.VOLUND_API_SECURE_COOKIES === "true";

export default defineConfig({
  build: {
    target: "es2022",
    sourcemap: true,
  },
  server: {
    proxy: {
      "/api": {
        target: apiTarget,
        configure: (proxy) => {
          if (!secureCookieTarget) return;
          proxy.on("proxyReq", (proxyRequest, request) => {
            const { origin, host } = request.headers;
            if (origin && host && origin === `http://${host}`) proxyRequest.setHeader("origin", `https://${host}`);
          });
        },
      },
    },
  },
});
