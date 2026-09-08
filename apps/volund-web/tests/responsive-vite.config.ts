import { defineConfig } from "vite";

// Explizit ohne Proxy: der Prüfer darf keinen Produktionsport kontaktieren.
export default defineConfig({ server: { host: "127.0.0.1", port: 4191, strictPort: true } });
