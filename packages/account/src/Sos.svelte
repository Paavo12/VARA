<!--
	SOS (PRD S-1, App Flow §6.1): a button on every screen during a ride, and in the driver app
	while online. Tapping starts a 3-second countdown she can cancel; then the alert goes to
	VARA's safety desk and her emergency contacts, and she is offered a one-tap call to the
	police. The API call is retried; if it still fails, she can text the safety desk from her
	own phone with her position filled in.
-->
<script lang="ts">
	import { Button } from '@fuber/ui';
	import { onDestroy } from 'svelte';
	import { useSession } from './context';

	interface Props {
		/** The ride on screen, if any (the API finds her current one otherwise). */
		rideId?: string | null;
		/** The safety desk's SMS number for the fallback; without it only 10111 is offered. */
		safetyDeskPhone?: string;
	}

	let { rideId = null, safetyDeskPhone }: Props = $props();
	const session = useSession();

	const COUNTDOWN_S = 3;
	const ATTEMPTS = 3;
	const POLICE = '10111';

	type Step = 'idle' | 'counting' | 'sending' | 'sent' | 'failed';
	let step = $state<Step>('idle');
	let left = $state(COUNTDOWN_S);
	let message = $state('');
	let police = $state(POLICE);
	let position: { lat: number; lon: number } | null = null;
	let timer: ReturnType<typeof setInterval> | undefined;

	function locate() {
		position = null;
		navigator.geolocation?.getCurrentPosition(
			(p) => (position = { lat: p.coords.latitude, lon: p.coords.longitude }),
			() => undefined,
			{ enableHighAccuracy: true, timeout: COUNTDOWN_S * 1000, maximumAge: 30_000 }
		);
	}

	function start() {
		step = 'counting';
		left = COUNTDOWN_S;
		// The position is fetched during the countdown so it is ready when the alert goes.
		locate();
		timer = setInterval(() => {
			left -= 1;
			if (left <= 0) {
				clearInterval(timer);
				void send();
			}
		}, 1000);
	}

	function cancel() {
		clearInterval(timer);
		step = 'idle';
	}

	async function send() {
		step = 'sending';
		for (let attempt = 1; attempt <= ATTEMPTS; attempt++) {
			const { data } = await session.api
				.POST('/v1/safety/sos', { body: { ride_id: rideId, location: position } })
				.catch(() => ({ data: undefined }));
			if (data) {
				message = data.message;
				police = data.police_phone;
				step = 'sent';
				return;
			}
			if (attempt < ATTEMPTS) await new Promise((r) => setTimeout(r, 1000));
		}
		step = 'failed';
	}

	const fallbackSms = $derived.by(() => {
		if (!safetyDeskPhone) return null;
		const where = position ? ` I'm at ${position.lat.toFixed(5)},${position.lon.toFixed(5)}.` : '';
		const body = `VARA SOS: I need help.${where}${rideId ? ` Ride ${rideId}.` : ''}`;
		return `sms:${safetyDeskPhone}?body=${encodeURIComponent(body)}`;
	});

	onDestroy(() => clearInterval(timer));
</script>

{#if step === 'idle'}
	<button class="sos-fab" type="button" onclick={start} aria-label="SOS: get help now">SOS</button>
{:else}
	<div class="overlay" role="alertdialog" aria-modal="true" aria-labelledby="sos-title">
		{#if step === 'counting'}
			<h2 id="sos-title">Sending SOS in</h2>
			<p class="count" aria-live="assertive">{left}</p>
			<p>VARA’s safety team and your emergency contacts will be alerted.</p>
			<Button size="driver" variant="secondary" onclick={cancel}>Cancel</Button>
		{:else if step === 'sending'}
			<h2 id="sos-title">Sending your alert…</h2>
			<a class="call" href="tel:{POLICE}">Call police {POLICE}</a>
		{:else if step === 'sent'}
			<h2 id="sos-title">Alert sent</h2>
			<p>{message}</p>
			<a class="call" href="tel:{police}">Call police {police}</a>
			<Button variant="ghost" onclick={() => (step = 'idle')}>Close</Button>
		{:else}
			<h2 id="sos-title">We couldn’t reach VARA</h2>
			<p>Call the police, or text the safety desk from your phone.</p>
			<a class="call" href="tel:{POLICE}">Call police {POLICE}</a>
			{#if fallbackSms}<a class="text" href={fallbackSms}>Text the safety desk</a>{/if}
			<Button variant="secondary" onclick={send}>Try again</Button>
			<Button variant="ghost" onclick={() => (step = 'idle')}>Close</Button>
		{/if}
	</div>
{/if}

<style>
	.sos-fab {
		position: fixed;
		top: calc(env(safe-area-inset-top, 0px) + var(--space-3));
		right: var(--space-3);
		z-index: 50;
		width: 56px;
		height: 56px;
		border: 0;
		border-radius: 50%;
		background: var(--button-sos-bg);
		color: var(--button-sos-fg);
		font: inherit;
		font-weight: 800;
		letter-spacing: 0.04em;
		box-shadow: 0 4px 12px rgb(0 0 0 / 0.25);
		cursor: pointer;
	}
	.sos-fab:focus-visible {
		outline: 3px solid var(--color-focus-ring);
		outline-offset: 2px;
	}
	.overlay {
		position: fixed;
		inset: 0;
		z-index: 60;
		display: grid;
		align-content: center;
		justify-items: stretch;
		gap: var(--space-4);
		padding: var(--space-6) var(--space-5);
		background: var(--color-sos-fill);
		color: var(--color-text);
		text-align: center;
	}
	h2 {
		margin: 0;
		font-size: var(--text-xl);
		color: var(--color-sos);
	}
	p {
		margin: 0;
	}
	.count {
		font-size: 5rem;
		font-weight: 800;
		line-height: 1;
		color: var(--color-sos);
		font-variant-numeric: tabular-nums;
	}
	.call,
	.text {
		display: flex;
		align-items: center;
		justify-content: center;
		min-height: 56px;
		border-radius: var(--radius-pill);
		font-weight: 700;
		text-decoration: none;
	}
	.call {
		background: var(--button-sos-bg);
		color: var(--button-sos-fg);
	}
	.text {
		border: 2px solid var(--color-sos);
		color: var(--color-sos);
	}
	@media (prefers-color-scheme: dark) {
		.overlay {
			background: var(--color-bg);
		}
	}
</style>
