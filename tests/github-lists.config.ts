import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  resolve: { conditions: ['browser'] },
  test: { environment: 'jsdom', include: ['tests/github-lists.test.ts', 'tests/github-cache.test.ts'], globals: true },
});
