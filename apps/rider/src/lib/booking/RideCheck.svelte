<!--
	RideCheck (PRD S-3, App Flow §6.2): full screen "Are you okay?" when the car leaves the
	planned route or stops for a while. "Yes, I'm fine" closes it; "Call for help" raises an
	SOS. With no answer in 60 s the safety desk is alerted, and the screen says so. The parent
	keeps it mounted until `onclose`, because her answer clears the question from the ride view
	before the "Help is coming" screen has been read.
-->
<script lang="ts">
	import type { RideCheckView } from '@fuber/api-client';
	import { messageOf, useSession } from '@fuber/account';
	import { Button, Notice } from '@fuber/ui';

	interface Props {
		check: RideCheckView;
		/** She closed it (answered "fine", or read the SOS screen). */
		onclose: () => void;
	}

	let { check, onclose }: Props = $props();
	const session = useSession();

	let now = $state(Date.now());
	let busy = $state(false);
	let error = $state('');
	/** After "Call for help": the SOS message and the police number. */
	let sos = $state<{ message: string; police: string } | null>(null);

	const left = $derived(Math.max(0, Math.ceil((new Date(check.answer_by).getTime() - now) / 1000)));
	const why = $derived(
		check.kind === 'deviation'
			? 'Your car has left the expected route.'
			: 'Your car has stopped for a while.'
	);

	$effect(() => {
		// A new question (another check) shows again.
		void check.id;
		sos = null;
		error = '';
		navigator.vibrate?.([300, 150, 300]);
		const t = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(t);
	});

	/**
	 * Her position, or null after at most 3 s. The browser's own timeout only starts once
	 * location access is granted, so a pending permission prompt must not hold up the SOS.
	 */
	function position(): Promise<{ lat: number; lon: number } | null> {
		return new Promise((resolve) => {
			if (!navigator.geolocation) return resolve(null);
			const giveUp = setTimeout(() => resolve(null), 3000);
			navigator.geolocation.getCurrentPosition(
				(p) => {
					clearTimeout(giveUp);
					resolve({ lat: p.coords.latitude, lon: p.coords.longitude });
				},
				() => {
					clearTimeout(giveUp);
					resolve(null);
				},
				{ enableHighAccuracy: true, timeout: 3000, maximumAge: 30_000 }
			);
		});
	}

	async function answer(a: 'fine' | 'help') {
		busy = true;
		error = '';
		const location = a === 'help' ? await position() : null;
		for (let attempt = 0; attempt < 3; attempt++) {
			const { data, error: e } = await session.api
				.POST('/v1/safety/ridecheck/{id}/respond', {
					params: { path: { id: check.id } },
					body: { answer: a, location }
				})
				.catch(() => ({ data: undefined, error: undefined }));
			if (data) {
				busy = false;
				if (data.sos) sos = { message: data.sos.message, police: data.sos.police_phone };
				else onclose();
				return;
			}
			if (e) {
				error = messageOf(e);
				break;
			}
		}
		busy = false;
		if (!error) error = "We couldn't reach VARA. If you need help, call 10111.";
	}
</script>

<div class="overlay" role="alertdialog" aria-modal="true" aria-labelledby="rc-title">
	<div class="box">
		{#if sos}
			<h2 id="rc-title">Help is coming</h2>
			<p>{sos.message}</p>
			<a class="police" href="tel:{sos.police}">Call the police ({sos.police})</a>
			<Button variant="secondary" onclick={onclose}>Close</Button>
		{:else if left === 0}
			<h2 id="rc-title">We've alerted VARA's safety team</h2>
			<p>You didn't answer, so the safety team will call you now.</p>
			<Button loading={busy} onclick={() => answer('fine')}>I'm fine</Button>
			<Button variant="sos" loading={busy} onclick={() => answer('help')}>Call for help</Button>
		{:else}
			<h2 id="rc-title">Are you okay?</h2>
			<p>{why}</p>
			<p class="muted" aria-live="polite">
				If you don't answer in {left} s, VARA's safety team will be alerted.
			</p>
			<Button loading={busy} onclick={() => answer('fine')}>Yes, I'm fine</Button>
			<Button variant="sos" loading={busy} onclick={() => answer('help')}>Call for help</Button>
		{/if}
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	</div>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		z-index: 50;
		display: grid;
		place-items: center;
		padding: var(--space-4);
		background: color-mix(in srgb, var(--color-bg) 92%, transparent);
	}
	.box {
		width: min(100%, 420px);
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		padding: var(--space-5);
		border-radius: var(--radius-lg);
		border: 1px solid var(--color-border);
		background: var(--color-surface);
	}
	h2,
	p {
		margin: 0;
	}
	h2 {
		font-size: var(--text-2xl);
	}
	.muted {
		color: var(--color-text-muted);
	}
	.police {
		display: block;
		padding: var(--space-3);
		border-radius: var(--radius-md);
		background: var(--button-sos-bg);
		color: var(--button-sos-fg);
		font-weight: 800;
		text-align: center;
		text-decoration: none;
	}
</style>
