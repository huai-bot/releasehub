import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Scoped component checks use the browser entry; they do not start a dev server.
export default defineConfig({
  plugins: [svelte()],
  resolve: { conditions: ['browser'] },
  test: { environment: 'jsdom', include: ['src/lib/feed/ui.spec.ts'], globals: true },
});
