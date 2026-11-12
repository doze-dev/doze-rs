import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The console is served by the doze-kafka binary from `crates/kafka-console/web-dist/` (see scripts/build.ts, which runs this
// build and then gzips the files and writes the Rust manifest). `base: "./"` keeps every asset URL relative, so the same build
// works at `/` and under `--console-prefix`; the server rewrites the two references in index.html to absolute ones.
export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: {
    outDir: "../crates/kafka-console/web-dist",
    emptyOutDir: true,
    assetsDir: "assets",
    sourcemap: false,
    // No inline scripts or data: URLs for scripts: the CSP is `default-src 'self'`. Fonts stay files.
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    target: "es2022",
  },
  server: {
    proxy: { "/api": { target: process.env.DOZE_CONSOLE ?? "http://127.0.0.1:9093", headers: { "X-Console": "1" } } },
  },
});
