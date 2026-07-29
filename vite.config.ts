import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const apiTarget = `http://localhost:${process.env.VITE_API_PORT ?? '8787'}`;

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': apiTarget,
      '/gateway': { target: apiTarget, ws: true },
    },
  },
});
