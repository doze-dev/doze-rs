import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// The gallery is a static page: `bun run build` writes dist/, `bun run preview` serves it (the Playwright run starts that).
// The same build settings as Kafka's console (no inline assets, es2022), so what the gallery proves holds for the consoles.
export default defineConfig({
  base: "/",
  plugins: [react(), tailwindcss()],
  // The AWS-specific kit (apps/aws/src/kit) is shown here too: the gallery is where every component is seen in every state.
  resolve: { alias: { "@aws-kit": fileURLToPath(new URL("../aws/src/kit/index.ts", import.meta.url)) } },
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
