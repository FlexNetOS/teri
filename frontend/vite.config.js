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
    port: 3000,
    open: true,
    proxy: {
      // Dev proxy: relative /api requests → teri serve (default addr 0.0.0.0:5001)
      '/api': {
        target: 'http://localhost:5001',
        changeOrigin: true,
        secure: false
      }
    }
  }
})
