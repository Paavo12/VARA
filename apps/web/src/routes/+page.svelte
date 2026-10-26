<svelte:head>
	<title>FUber — rides by women, for women</title>
	<meta
		name="description"
		content="FUber is a women-only ride-hailing service in Windhoek: verified riders, verified drivers, safety you can see."
	/>
</svelte:head>

<main>
	<h1>FUber</h1>
	<p class="tagline">Rides by women, for women. Windhoek, coming soon.</p>
	<p class="note">
		Website shell (Phase 0): legal pages, trip-share links and driver applications are added in
		later phases.
	</p>
</main>

<style>
	main {
		max-width: 720px;
		margin: 0 auto;
		padding: var(--space-6) var(--space-4);
	}
	h1 {
		margin: 0;
		font-size: var(--text-3xl);
		color: var(--color-brand);
	}
	.tagline {
		font-size: var(--text-lg);
	}
	.note {
		color: var(--color-text-muted);
	}
</style>
