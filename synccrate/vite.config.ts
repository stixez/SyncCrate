import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  build: {
    // The CSP has no font-src, so fonts fall back to default-src 'self' and a
    // font inlined as a data: URI would be blocked. Always emit them as files.
    assetsInlineLimit: (file: string) => (/\.(woff2?|ttf|otf)$/.test(file) ? false : undefined),
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
