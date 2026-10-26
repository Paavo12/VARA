<script lang="ts">
	import { Button } from '@fuber/ui';
</script>

<svelte:head>
	<title>FUber Driver</title>
</svelte:head>

<main>
	<h1>FUber Driver</h1>
	<p class="tagline">Drive on your terms. Only women passengers.</p>
	<Button size="driver" disabled>Go online</Button>
	<p class="note">Driver app shell (Phase 0). Applications arrive in Phase 1.</p>
</main>

<style>
	main {
		min-height: 100dvh;
		display: grid;
		place-content: center;
		gap: var(--space-4);
		padding: var(--space-4);
		text-align: center;
	}
	h1 {
		margin: 0;
		font-size: var(--text-3xl);
		color: var(--color-brand);
	}
	.tagline {
		margin: 0;
		color: var(--color-text-secondary);
	}
	.note {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
</style>
