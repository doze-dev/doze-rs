import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The console is served by the doze-aws binary from `crates/aws-console/web-dist/` (scripts/embed.ts runs this build, then
// gzips the files and writes the Rust manifest and the source hash). `base: "./"` keeps every asset URL relative; the server
// rewrites the references in index.html to absolute ones, so a deep link finds them (crates/aws-console/src/web.rs).
//
// `bun run dev` serves the pages with hot reload and sends /api to a running doze-aws console:
//   DOZE_AWS_CONSOLE=http://127.0.0.1:4567 bun run dev
export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: {
    outDir: "../../../crates/aws-console/web-dist",
    emptyOutDir: true,
    assetsDir: "assets",
    sourcemap: false,
    // No inline scripts and no data: URLs: the console's CSP is `default-src 'self'`. Fonts and the mark stay files.
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    target: "es2022",
    chunkSizeWarningLimit: 700,
  },
  server: {
    proxy: { "/api": { target: process.env.DOZE_AWS_CONSOLE ?? "http://127.0.0.1:4567", headers: { "X-Console": "1" } } },
  },
});
