/// <reference types="vitest/config" />
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// In development the page is served by Vite and the API by a hub running next to it (HUB_URL, default http://localhost:8080).
const hub = process.env.HUB_URL ?? 'http://localhost:8080';

export default defineConfig({
  plugins: [react()],
  server: { proxy: { '/api': { target: hub, changeOrigin: false } } },
  build: { outDir: 'dist', sourcemap: true },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    // only measured with `vitest run --coverage` (CI does, and posts it on the pull request)
    coverage: { provider: 'v8', include: ['src/**'], exclude: ['src/test/**', 'src/generated/**'], reporter: ['json-summary', 'text-summary'] },
  },
});
