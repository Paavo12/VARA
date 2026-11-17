<!--
	Share trip (PRD S-2, App Flow §6.3): what someone she sent the link to sees. Polls every 10 s
	while the trip is live and the tab is visible; stops when it ends. No sign-in: the token is
	the key, so the page is kept out of search engines and sends no referrer.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { Notice, Plate } from '@fuber/ui';
	import {
		POLL_MS,
		ageText,
		etaText,
		headline,
		live,
		loadTrip,
		mapLink,
		type Loaded
	} from '#lib/share.ts';

	let loaded = $state<Loaded | null>(null);
	let now = $state(new Date());
	// The last trip seen, kept on screen while a refresh fails.
	let last = $state<Extract<Loaded, { kind: 'trip' }> | null>(null);

	$effect(() => {
		const token = page.params.token ?? '';
		let timer: ReturnType<typeof setTimeout> | undefined;
		let stopped = false;

		async function tick() {
			if (document.visibilityState === 'visible') {
				const l = await loadTrip(token);
				if (stopped) return;
				loaded = l;
				if (l.kind === 'trip') last = l;
				now = new Date();
				if (!live(l)) return;
			}
			timer = setTimeout(tick, POLL_MS);
		}
		function onVisible() {
			if (document.visibilityState === 'visible' && loaded && live(loaded)) {
				clearTimeout(timer);
				tick();
			}
		}
		tick();
		document.addEventListener('visibilitychange', onVisible);
		const clock = setInterval(() => (now = new Date()), 5_000);
		return () => {
			stopped = true;
			clearTimeout(timer);
			clearInterval(clock);
			document.removeEventListener('visibilitychange', onVisible);
		};
	});

	let trip = $derived(last?.trip ?? null);
</script>

<svelte:head>
	<title>VARA — shared trip</title>
	<meta name="robots" content="noindex, nofollow" />
	<meta name="referrer" content="no-referrer" />
</svelte:head>

<main>
	<p class="brand">VARA</p>

	{#if loaded === null}
		<p class="muted" role="status">Loading the trip…</p>
	{:else if loaded.kind === 'not_found'}
		<h1>This link isn't working</h1>
		<p>It may have been typed wrong, or the trip moved to another driver.</p>
	{:else if loaded.kind === 'expired'}
		<h1>This trip has ended</h1>
		<p>Shared trip links stop working 30 minutes after the trip ends.</p>
	{:else if trip}
		<h1>{headline(trip)}</h1>

		{#if loaded.kind === 'retry'}
			<Notice tone="attention">
				<p>We can't refresh the trip right now. Still trying…</p>
			</Notice>
		{/if}

		{#if trip.eta}
			<p class="eta">
				{trip.status === 'in_trip' ? 'Arriving' : 'Driver arriving'}
				{etaText(trip.eta, now)}
			</p>
		{/if}

		{#if trip.car}
			<section class="card" aria-label="The car">
				<p class="driver">{trip.driver_first_name ?? 'Her driver'}</p>
				<p>{trip.car.colour} {trip.car.make} {trip.car.model}</p>
				<Plate plate={trip.car.plate} />
			</section>
		{/if}

		{#if trip.position}
			<p>
				Car last seen {ageText(trip.position.at, now)} ·
				<!-- An external map, not a route of this site. -->
				<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
				<a href={mapLink(trip.position)} target="_blank" rel="noopener noreferrer">Open the map</a>
			</p>
		{/if}

		<p class="muted">Going to: {trip.destination_area}</p>

		{#if trip.status === 'ended' && trip.expires_at}
			<p class="muted">
				This link stops working at {new Date(trip.expires_at).toLocaleTimeString('en-NA', {
					hour: '2-digit',
					minute: '2-digit',
					hour12: false
				})}.
			</p>
		{/if}

		<section class="help">
			<p>If you think she is in danger, call the police.</p>
			<a class="police" href="tel:{trip.police_phone}">Call {trip.police_phone}</a>
		</section>
	{:else}
		<h1>We can't load this trip right now</h1>
		<p>Check your connection. We'll keep trying.</p>
	{/if}
</main>

<style>
	main {
		max-width: 560px;
		margin: 0 auto;
		padding: var(--space-6) var(--space-4);
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}
	main :global(p) {
		margin: 0;
	}
	.brand {
		font-weight: 800;
		color: var(--color-brand);
		letter-spacing: 0.08em;
	}
	h1 {
		margin: 0;
		font-size: var(--text-2xl);
	}
	.eta {
		font-size: var(--text-lg);
		font-weight: 700;
	}
	.card {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		align-items: flex-start;
		padding: var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.driver {
		font-weight: 700;
	}
	.muted {
		color: var(--color-text-muted);
	}
	.help {
		margin-top: var(--space-4);
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	.police {
		display: block;
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-md);
		background: var(--button-sos-bg);
		color: var(--button-sos-fg);
		font-weight: 800;
		text-align: center;
		text-decoration: none;
	}
	.police:focus-visible {
		outline: 3px solid var(--color-focus-ring);
		outline-offset: 2px;
	}
</style>
