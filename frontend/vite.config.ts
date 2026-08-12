import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

const apiTarget = `http://localhost:${process.env.VITE_API_PORT ?? '8787'}`;
const frontendRoot = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  root: frontendRoot,
  plugins: [react()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (/[\\/]node_modules[\\/](@tiptap|prosemirror-)/.test(id)) return 'editor-vendor';
          if (/[\\/]node_modules[\\/](@fluentui|@griffel|@floating-ui|tabster)/.test(id)) return 'ui-vendor';
          if (/[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/.test(id)) return 'react-vendor';
          if (/[\\/]node_modules[\\/](@phosphor-icons|simple-icons)[\\/]/.test(id)) return 'icon-vendor';
          return 'vendor';
        },
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': apiTarget,
      '/gateway': { target: apiTarget, ws: true },
    },
  },
  test: {
    include: [
      'src/**/*.{test,spec}.{ts,tsx}',
      '../scripts/**/*.{test,spec}.{ts,mts,mjs}',
    ],
  },
});
