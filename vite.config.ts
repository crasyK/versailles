import { defineConfig } from "vite";
import { resolve } from "node:path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
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
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        launcher: resolve(__dirname, "launcher.html"),
        "launcher-dim": resolve(__dirname, "launcher-dim.html"),
        canvas: resolve(__dirname, "canvas.html"),
        desktop: resolve(__dirname, "desktop.html"),
        anywhere: resolve(__dirname, "anywhere.html"),
        guides: resolve(__dirname, "guides.html"),
      },
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
