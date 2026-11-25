import { defineConfig } from 'vitest/config';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';

// The app calls the API over HTTPS and its WebSocket, and shows photos from it (signed links).
const API = new URL(process.env.VITE_API_URL ?? 'http://127.0.0.1:8080');
const API_ORIGIN = API.origin as `${string}://${string}.${string}:${number}`;
const WS_ORIGIN = API_ORIGIN.replace(/^http/, 'ws') as `${string}://${string}.${string}:${number}`;

// With Cloud Storage the files go to and from the documents bucket itself: allow only that
// bucket's path, never all of Cloud Storage (infra/terraform/README.md, "Known gaps").
const STORAGE_URL = process.env.VITE_STORAGE_URL;
if (STORAGE_URL && !/^https:\/\/storage\.googleapis\.com\/[a-z0-9_-]+\/$/.test(STORAGE_URL)) {
	throw new Error(`VITE_STORAGE_URL must be https://storage.googleapis.com/<bucket>/ (got ${STORAGE_URL})`);
}
const STORAGE = (STORAGE_URL ? [STORAGE_URL] : []) as `${string}://${string}.${string}:${number}`[];

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			adapter: adapter({ fallback: 'index.html' }),
			// Strict CSP (TRD §7.5), as a <meta> tag in the built page: the app runs from files in
			// the phone's WebView, so there is no server to send it as a header. SvelteKit hashes its
			// own start-up script. A script that slips into the page through any text can't run.
			csp: {
				mode: 'hash',
				directives: {
					'default-src': ['self'],
					'script-src': ['self'],
					'style-src': ['self', 'unsafe-inline'],
					'img-src': ['self', 'data:', 'blob:', API_ORIGIN],
					'connect-src': ['self', API_ORIGIN, WS_ORIGIN, ...STORAGE],
					'font-src': ['self'],
					'object-src': ['none'],
					'base-uri': ['self'],
					'form-action': ['self']
				}
			}
		})
	],
	test: {
		expect: { requireAssertions: true },
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'server',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
