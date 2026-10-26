<script lang="ts">
	import { Button } from '@fuber/ui';
</script>

<svelte:head>
	<title>FUber</title>
</svelte:head>

<main>
	<h1>FUber</h1>
	<p class="tagline">Rides by women, for women.</p>
	<Button disabled>Where to?</Button>
	<p class="note">Rider app shell (Phase 0). Sign-in arrives in Phase 1.</p>
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
