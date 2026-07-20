import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: "autoUpdate",
      manifest: {
        name: "__PRODUCT_NAME__ Web Wrapper",
        short_name: "__PRODUCT_NAME__",
        theme_color: "#111827",
        background_color: "#111827",
        display: "standalone"
      }
    })
  ]
});
