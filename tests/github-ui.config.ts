import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Scoped UI checks only: mounting Svelte requires its browser entry, not SSR.
export default defineConfig({
  plugins: [svelte()],
  resolve: { conditions: ['browser'] },
  test: { environment: 'jsdom', include: ['tests/github-ui.test.ts'], globals: true },
});
