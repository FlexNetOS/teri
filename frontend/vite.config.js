import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import path from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
      // locales now live inside the teri frontend (self-contained, no MiroFish sibling dir)
      '@locales': path.resolve(__dirname, 'locales')
    }
  },
  server: {
    // 8374 = "TERI" on a phone keypad — deliberately uncommon to dodge the 3000/3001/5173
    // collision zone. strictPort fails loudly on a conflict instead of silently wandering to a
    // neighbouring port (which makes the dev URL unpredictable). Override per-run with --port.
    port: 8374,
    strictPort: true,
    open: true,
    proxy: {
      // Dev proxy: relative /api requests → teri serve (default addr 0.0.0.0:5001)
      '/api': {
        target: 'http://localhost:5001',
        changeOrigin: true,
        secure: false
      }
    }
  },
  build: {
    rollupOptions: {
      output: {
        // Split heavy vendor libs into their own chunks so d3 (only used by GraphPanel) and the
        // Vue framework are cached separately instead of inflating the main entry chunk — resolves
        // the ">500 kB chunk" warning by giving the bundler explicit split points.
        manualChunks: {
          d3: ['d3'],
          vue: ['vue', 'vue-router', 'vue-i18n']
        }
      }
    }
  }
})
