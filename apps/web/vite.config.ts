import path from 'path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@bunny/i18n': path.resolve(__dirname, '../../packages/i18n'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': { target: 'http://127.0.0.1:7681', changeOrigin: true },
      '/s': { target: 'http://127.0.0.1:7681', changeOrigin: true },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // xterm.js is ~500 kB minified; split it from the app shell.
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/@xterm') || id.includes('node_modules/xterm')) {
            return 'xterm';
          }
          if (id.includes('node_modules/react-dom') || id.includes('node_modules/react/')) {
            return 'react';
          }
        },
      },
    },
  },
});
