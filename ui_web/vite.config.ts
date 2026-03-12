import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  plugins: [preact()],
  base: "./",
  build: {
    outDir: "../android/app/src/main/assets/ui",
    emptyOutDir: false,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: "./src/vitest.setup.ts",
  },
});
