<!--
	Driving (App Flow §4, PRD D-3 – D-5): go online and offline, the request card with its 15 s
	countdown (earnings largest; no destination, only the pickup area), and the accepted trip.
	After a PIN lockout she stays online but requests pause until the safety desk closes it
	(App Flow §7); closing cancels the blocked trip, so that update refetches the status.
	While online the position goes over the socket every 4 s. After every (re)connect the
	status is refetched, so a reload or a dropped connection resumes where it was.
-->
<script lang="ts">
	import {
		connectLive,
		type ChatMessage,
		type DriverRide,
		type DriverStatus,
		type LiveMessage,
		type OfferCard
	} from '@fuber/api-client';
	import { Chat, messageOf, Sos, useSession } from '@fuber/account';
	import { Button, Notice, Screen, formatDistance, formatMinutes, formatNad } from '@fuber/ui';
	import { onMount } from 'svelte';
	import Earnings from './Earnings.svelte';
	import { SAFETY_DESK_PHONE } from './session';
	import Trip from './Trip.svelte';

	interface Props {
		name: string;
		tiers: string[];
		ondocuments: () => void;
		onsettings: () => void;
	}

	let { name, tiers, ondocuments, onsettings }: Props = $props();
	const session = useSession();

	const LOCATION_EVERY_MS = 4000;
	/**
	 * Local development only: where the driver is when the computer running the app is not in
	 * Windhoek (Independence Avenue, a few hundred metres from the rider's Post Street Mall).
	 */
	const DEV_POSITION = { lat: -22.5703, lon: 17.0838 };
	const NAMES: Record<string, string> = {
		classic: 'Classic',
		comfort: 'Comfort',
		softlife: 'Soft Life'
	};

	/** Driver and passenger can message each other from acceptance until the trip ends (PRD T-8). */
	const CHAT_OPEN = [
		'DRIVER_ASSIGNED',
		'DRIVER_EN_ROUTE',
		'DRIVER_ARRIVED',
		'PIN_VERIFIED',
		'IN_TRIP'
	];
	let chat = $state<Chat>();

	let status = $state<DriverStatus | null>(null);
	let offer = $state<OfferCard | null>(null);
	let deadline = $state(0);
	let now = $state(Date.now());
	let ride = $state<DriverRide | null>(null);
	let busy = $state(false);
	let showEarnings = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let position: { lat: number; lon: number; heading?: number; speed_kmh?: number } | null = null;
	let live: ReturnType<typeof connectLive> | null = null;

	function show(s: DriverStatus) {
		status = s;
		// A trip that just ended stays on screen (summary) until she taps Done.
		const ended = ride && ['COMPLETED', 'RATED', 'CANCELLED', 'BLOCKED'].includes(ride.state);
		ride = s.ride ?? (ended ? ride : null);
		setOffer(s.offer ?? null);
	}

	function setOffer(o: OfferCard | null) {
		offer = o;
		deadline = o ? Date.now() + o.seconds_left * 1000 : 0;
	}

	async function load() {
		const { data } = await session.api.GET('/v1/driver/status').catch(() => ({ data: undefined }));
		if (data) show(data);
	}

	function onmessage(m: LiveMessage) {
		if (m.type === 'offer.created') {
			setOffer(m.data as OfferCard);
			notice = null;
		} else if (m.type === 'offer.expired') {
			const d = m.data as { offer_id: string; response: string };
			if (offer?.offer_id === d.offer_id) {
				setOffer(null);
				if (d.response === 'expired') notice = 'The request timed out and went to another driver.';
				if (d.response === 'withdrawn') notice = 'The passenger cancelled the request.';
			}
		} else if (m.type === 'ride.updated') {
			// Keep the finished trip on screen until she taps Done.
			const next = m.data as DriverRide;
			if (ride?.id === next.id || !ride) ride = next;
			if (status?.hold && next.state === 'CANCELLED') void load();
		} else if (m.type === 'chat.message') {
			chat?.receive(m.data as ChatMessage);
		} else if (m.type === 'ride.removed') {
			ride = null;
			notice = 'The passenger cancelled the trip. No fee for you.';
		}
	}

	/** The phone's position; in development, a fixed point in Windhoek when there is no fix. */
	function locate(): Promise<typeof position> {
		return new Promise((resolve) => {
			const fallback = () => resolve(import.meta.env.DEV ? DEV_POSITION : null);
			if (!navigator.geolocation) return fallback();
			navigator.geolocation.getCurrentPosition(
				(p) => {
					const { latitude: lat, longitude: lon } = p.coords;
					const inWindhoek = lat >= -22.8 && lat <= -22.35 && lon >= 16.85 && lon <= 17.35;
					if (!inWindhoek) return fallback();
					resolve({
						lat,
						lon,
						heading: p.coords.heading == null ? undefined : Math.round(p.coords.heading) % 360,
						speed_kmh: p.coords.speed == null ? undefined : p.coords.speed * 3.6
					});
				},
				fallback,
				{ enableHighAccuracy: true, timeout: 8000, maximumAge: 5000 }
			);
		});
	}

	const body = (p: NonNullable<typeof position>) => ({
		at: { lat: p.lat, lon: p.lon },
		heading: p.heading ?? null,
		speed_kmh: p.speed_kmh ?? null
	});

	async function goOnline() {
		error = null;
		busy = true;
		position = await locate();
		if (!position) {
			busy = false;
			return void (error = 'Turn on location so passengers nearby can find you.');
		}
		const { data, error: err } = await session.api
			.POST('/v1/driver/online', { body: body(position) })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) show(data);
		else error = messageOf(err, 'We couldn’t take you online. Check your connection.');
	}

	async function goOffline() {
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST('/v1/driver/offline')
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) show(data);
		else error = messageOf(err, 'We couldn’t take you offline. Check your connection.');
	}

	async function answer(accept: boolean) {
		if (!offer) return;
		const id = offer.offer_id;
		error = null;
		busy = true;
		if (accept) {
			const {
				data,
				error: err,
				response
			} = await session.api
				.POST('/v1/offers/{id}/accept', { params: { path: { id } } })
				.catch(() => ({ data: undefined, error: undefined, response: undefined }));
			if (response?.status === 503) {
				// Too busy to answer, but the accept may have landed: ask where things stand.
				notice = messageOf(err);
				await load();
				if (ride) notice = null;
				busy = false;
				return;
			}
			if (data) ride = data;
			else notice = messageOf(err, 'This request is no longer available.');
		} else {
			await session.api
				.POST('/v1/offers/{id}/decline', { params: { path: { id } } })
				.catch(() => undefined);
		}
		busy = false;
		if (offer?.offer_id === id) setOffer(null);
	}

	async function sendPosition() {
		if (!status?.online) return;
		position = (await locate()) ?? position;
		if (!position) return;
		if (live?.send('location.update', body(position))) return;
		await session.api.PUT('/v1/driver/location', { body: body(position) }).catch(() => undefined);
	}

	onMount(() => {
		live = connectLive({
			baseUrl: session.baseUrl,
			token: session.accessToken,
			onmessage,
			onconnect: () => {
				void load();
				void chat?.refresh();
			},
			onrevoked: () => session.signOut()
		});
		void load();
		const tick = setInterval(() => (now = Date.now()), 250);
		const report = setInterval(sendPosition, LOCATION_EVERY_MS);
		return () => {
			clearInterval(tick);
			clearInterval(report);
			live?.close();
		};
	});

	const left = $derived(Math.max(0, Math.ceil((deadline - now) / 1000)));
	const progress = $derived(offer ? Math.max(0, (deadline - now) / 15_000) : 0);
	const RING = 2 * Math.PI * 44;
</script>

<!-- SOS on every screen while she is online or on a trip (PRD S-1). -->
{#if ride || status?.online}
	<Sos rideId={ride?.id ?? null} safetyDeskPhone={SAFETY_DESK_PHONE} />
{/if}
{#if ride && CHAT_OPEN.includes(ride.state)}
	<Chat bind:this={chat} rideId={ride.id} party="driver" name={ride.passenger.name} />
{/if}

{#if ride}
	<Trip
		{ride}
		onchange={(r) => (ride = r)}
		onreleased={() => {
			ride = null;
			notice = 'You cancelled. The request went to another driver.';
		}}
		ondone={() => {
			ride = null;
			void load();
		}}
	/>
{:else if offer}
	<Screen title="New request" lead="{NAMES[offer.tier] ?? offer.tier} · {offer.pickup_area.label}">
		<div class="offer" role="alert">
			<svg class="ring" viewBox="0 0 100 100" aria-hidden="true">
				<circle cx="50" cy="50" r="44" class="track" />
				<circle
					cx="50"
					cy="50"
					r="44"
					class="bar"
					stroke-dasharray={RING}
					stroke-dashoffset={RING * (1 - progress)}
				/>
			</svg>
			<span class="left" aria-label="{left} seconds left">{left}</span>
		</div>
		<p class="earning">{formatNad(offer.earning)}</p>
		<div class="card">
			<p>
				<small>Pickup</small>{formatMinutes(offer.eta_s)} away · {formatDistance(offer.distance_m)}
			</p>
			<p>
				<small>Passenger</small><strong>{offer.passenger.name}</strong>
				{offer.passenger.booked_by ??
					(offer.passenger.rating != null ? `★ ${offer.passenger.rating.toFixed(2)}` : 'New rider')}
			</p>
			<p>
				<small>Passengers</small>{offer.passengers}{offer.wheelchair ? ' · wheelchair' : ''} ·
				{offer.payment_method === 'cash' ? 'Cash' : 'Card'}
			</p>
		</div>
		{#snippet footer()}
			<Button size="driver" onclick={() => answer(true)} loading={busy}>Accept</Button>
			<Button size="driver" variant="secondary" onclick={() => answer(false)} disabled={busy}
				>Decline</Button
			>
		{/snippet}
	</Screen>
{:else if showEarnings}
	<Earnings onback={() => (showEarnings = false)} />
{:else if status?.online}
	<Screen
		title="You’re online"
		lead={status.hold ? 'New requests are paused.' : 'Looking for passengers near you…'}
	>
		{#if status.hold}
			<Notice tone="attention">
				<p>{status.hold.message}</p>
			</Notice>
		{:else}
			<div class="radar" aria-hidden="true"><span class="ring-pulse"></span></div>
		{/if}
		<p class="hint">Ride types: {status.tiers.map((t) => NAMES[t] ?? t).join(', ')}</p>
		{#if notice}<Notice tone="info"><p>{notice}</p></Notice>{/if}
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		<Button variant="ghost" onclick={() => (showEarnings = true)}>Earnings and trips</Button>
		{#snippet footer()}
			<Button size="driver" variant="secondary" onclick={goOffline} loading={busy}
				>Go offline</Button
			>
		{/snippet}
	</Screen>
{:else}
	<Screen title="Hi {name}" lead="You’re offline.">
		<Notice tone="success">
			<p>Approved ride types: {tiers.map((t) => NAMES[t] ?? t).join(', ') || 'none yet'}.</p>
		</Notice>
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		<Button variant="secondary" onclick={() => (showEarnings = true)}>Earnings and trips</Button>
		<Button variant="secondary" onclick={ondocuments}>Documents</Button>
		{#snippet footer()}
			<Button size="driver" onclick={goOnline} loading={busy} disabled={!status}>Go online</Button>
			<Button variant="ghost" onclick={onsettings}>Settings</Button>
		{/snippet}
	</Screen>
{/if}

<style>
	.card {
		display: grid;
		gap: var(--space-2);
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.card p {
		margin: 0;
		display: grid;
	}
	small {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.earning {
		margin: 0;
		text-align: center;
		font-size: var(--text-3xl);
		font-weight: 800;
		font-variant-numeric: tabular-nums;
	}
	.offer {
		position: relative;
		width: 120px;
		height: 120px;
		margin: 0 auto;
		display: grid;
		place-items: center;
	}
	.ring {
		position: absolute;
		inset: 0;
		transform: rotate(-90deg);
	}
	.ring circle {
		fill: none;
		stroke-width: 8;
	}
	.track {
		stroke: var(--color-border);
	}
	.bar {
		stroke: var(--color-brand);
		stroke-linecap: round;
		transition: stroke-dashoffset 0.25s linear;
	}
	.left {
		font-size: var(--text-2xl);
		font-weight: 800;
		font-variant-numeric: tabular-nums;
	}
	.radar {
		height: 160px;
		display: grid;
		place-items: center;
	}
	.ring-pulse {
		width: 140px;
		height: 140px;
		border-radius: 50%;
		border: 2px solid var(--color-brand);
		animation: pulse 2.4s ease-out infinite;
	}
	@keyframes pulse {
		from {
			transform: scale(0.1);
			opacity: 0.9;
		}
		to {
			transform: scale(1);
			opacity: 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.ring-pulse {
			animation: none;
			opacity: 0.4;
		}
		.bar {
			transition: none;
		}
	}
	.hint {
		margin: 0;
		text-align: center;
		color: var(--color-text-muted);
	}
</style>
