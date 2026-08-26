import preact from '@preact/preset-vite';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

const apiTarget = `http://localhost:${process.env.VITE_API_PORT ?? '8787'}`;
const frontendRoot = fileURLToPath(new URL('.', import.meta.url));
const packageVersion = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')).version as string;

export default defineConfig({
  root: frontendRoot,
  define: { __APP_VERSION__: JSON.stringify(packageVersion) },
  plugins: [preact()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (/[\\/]node_modules[\\/](@tiptap|prosemirror-)/.test(id)) return 'editor-vendor';
          if (/[\\/]node_modules[\\/]preact[\\/]/.test(id)) return 'preact-vendor';
          return undefined;
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
