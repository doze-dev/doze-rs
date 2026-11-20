import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The gallery is a static page: `bun run build` writes dist/, `bun run preview` serves it (the Playwright run starts that).
// The same build settings as Kafka's console (no inline assets, es2022), so what the gallery proves holds for the consoles.
export default defineConfig({
  base: "/",
  plugins: [react(), tailwindcss()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    target: "es2022",
    chunkSizeWarningLimit: 900,
  },
});
