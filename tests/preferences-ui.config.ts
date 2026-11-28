import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  resolve: { conditions: ['browser'] },
  test: { environment: 'jsdom', include: ['tests/preferences-ui.test.ts', 'tests/safety-ui.test.ts', 'tests/installation-ui.test.ts', 'tests/catalog-sort-ui.test.ts', 'tests/library-manual/library-manual.test.ts'], globals: true },
});
