import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { fileURLToPath, URL } from 'node:url'

const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [vue()],
  root: 'frontend',
  publicDir: 'public',
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./frontend/src', import.meta.url)),
    },
  },
  // Tauri serves the dev server itself; failing loudly beats silently
  // falling back to another port the Rust side is not pointed at.
  clearScreen: false,
  server: {
    port: 5183,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 5184 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'esnext',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    minify: process.env.TAURI_ENV_DEBUG ? false : 'esbuild',
  },
})
