import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";

const framePolicy = "frame-ancestors 'none'";

export default defineConfig({
  envDir: "../..",
  plugins: [
    react(),
    VitePWA({
      strategies: "injectManifest",
      srcDir: "src",
      filename: "sw.ts",
      injectRegister: "auto",
      manifest: {
        name: "Leave CLI",
        short_name: "Leave",
        description: "A mobile workspace for the local Devin agent on your computer.",
        display: "standalone",
        start_url: "/",
        background_color: "#121923",
        theme_color: "#121923",
        orientation: "any",
        categories: ["developer", "productivity"],
        icons: [
          {
            src: "/favicon.svg",
            sizes: "any",
            type: "image/svg+xml",
            purpose: "any maskable"
          }
        ]
      },
      injectManifest: {
        globPatterns: ["**/*.{js,css,html,woff2}"]
      }
    })
  ],
  server: {
    headers: { "Content-Security-Policy": framePolicy },
    port: 5173,
    strictPort: true
  },
  preview: {
    headers: { "Content-Security-Policy": framePolicy },
    port: 4173,
    strictPort: true
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: "./src/test-setup.ts"
  }
});
