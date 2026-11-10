import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [svelte()],
	// Svelte's browser build, so component tests can mount (they opt into jsdom per file).
	resolve: { conditions: ['browser'] }
});
