<!--
	Booking (App Flow §3): home → Where to? → ride options → the ride. Owns the live connection:
	ride updates and the driver's position arrive over the socket, and after every (re)connect
	the active ride is refetched, so a reload or a dropped connection resumes where it was.
-->
<script lang="ts">
	import {
		connectLive,
		type ChatMessage,
		type LiveMessage,
		type Me,
		type RiderRide
	} from '@fuber/api-client';
	import { Chat, ShareTrip, Sos, useSession } from '@fuber/account';
	import { Button, Notice, Screen } from '@fuber/ui';
	import { onMount } from 'svelte';
	import { SAFETY_DESK_PHONE } from '../session';
	import Options from './Options.svelte';
	import History from './History.svelte';
	import RideCheck from './RideCheck.svelte';
	import RideStatus from './RideStatus.svelte';
	import Search from './Search.svelte';
	import { DEV_LOCATION, inServiceArea, type Stop } from './types';

	interface Props {
		me: Me;
		onsettings: () => void;
	}

	let { me, onsettings }: Props = $props();
	const session = useSession();

	type Stage = 'loading' | 'home' | 'where' | 'pickup' | 'options' | 'ride' | 'history';
	/** SOS is on screen from the moment a driver is assigned (PRD S-1). */
	const WITH_DRIVER = [
		'DRIVER_ASSIGNED',
		'DRIVER_EN_ROUTE',
		'DRIVER_ARRIVED',
		'PIN_VERIFIED',
		'IN_TRIP',
		'BLOCKED'
	];
	let stage = $state<Stage>('loading');
	let ride = $state<RiderRide | null>(null);
	let current = $state<Stop | null>(null);
	let pickup = $state<Stop | null>(null);
	let dropoff = $state<Stop | null>(null);

	const FINISHED = ['COMPLETED', 'RATED', 'CANCELLED', 'BLOCKED'];
	/** The driver and rider can message each other from acceptance until the trip ends (PRD T-8). */
	const CHAT_OPEN = WITH_DRIVER.filter((s) => s !== 'BLOCKED');
	let chat = $state<Chat>();
	/** An SOS countdown or its result is on screen: it stays even if the trip ends meanwhile. */
	let sosActive = $state(false);

	async function loadActive() {
		const { data, response } = await session.api
			.GET('/v1/rides/active')
			.catch(() => ({ data: undefined, response: undefined }));
		if (data) {
			ride = data;
			stage = 'ride';
		} else if (response?.status === 204) {
			// No ride on the server: keep a finished ride on screen until the rider closes it.
			if (stage === 'ride' && ride && FINISHED.includes(ride.state)) return;
			ride = null;
			if (stage === 'loading' || stage === 'ride') stage = 'home';
		} else if (stage === 'loading') {
			stage = 'home';
		}
	}

	function onmessage(m: LiveMessage) {
		if (m.type === 'ride.updated') {
			const next = m.data as RiderRide;
			if (ride && next.id !== ride.id && !FINISHED.includes(ride.state)) return;
			ride = next;
			stage = 'ride';
		} else if (m.type === 'chat.message') {
			chat?.receive(m.data as ChatMessage);
		} else if (m.type === 'driver.location' && ride?.driver) {
			const d = m.data as { ride_id: string; lat: number; lon: number };
			if (d.ride_id === ride.id) ride.driver = { ...ride.driver, lat: d.lat, lon: d.lon };
		}
	}

	/** The phone's position named by the nearest street or landmark; null if unavailable. */
	async function locate(): Promise<Stop | null> {
		const fix = await new Promise<GeolocationPosition | null>((resolve) => {
			if (!navigator.geolocation) return resolve(null);
			navigator.geolocation.getCurrentPosition(resolve, () => resolve(null), {
				enableHighAccuracy: true,
				timeout: 8000,
				maximumAge: 60_000
			});
		});
		const { latitude: lat, longitude: lon } = fix?.coords ?? {};
		if (lat == null || lon == null || !inServiceArea(lat, lon)) {
			return import.meta.env.DEV ? DEV_LOCATION : null;
		}
		const { data } = await session.api
			.GET('/v1/places/reverse', { params: { query: { lat, lon } } })
			.catch(() => ({ data: undefined }));
		return { address: data?.address ?? 'Current location', lat, lon };
	}

	onMount(() => {
		const live = connectLive({
			baseUrl: session.baseUrl,
			token: session.accessToken,
			onmessage,
			onconnect: () => {
				void loadActive();
				void chat?.refresh();
			},
			onrevoked: () => session.signOut()
		});
		void loadActive();
		void locate().then((s) => {
			current = s;
			pickup ??= s;
		});
		return () => live.close();
	});

	function whereTo() {
		pickup ??= current;
		stage = 'where';
	}

	function done() {
		ride = null;
		dropoff = null;
		pickup = current;
		stage = 'home';
	}
</script>

{#if stage === 'loading'}
	<div class="splash" aria-label="Loading"><span>VARA</span></div>
{:else if stage === 'home'}
	<Screen title="Hi {me.display_name}" lead="Where are you going today?">
		{#if me.emergency_contacts === 0}
			<Notice tone="attention" title="Add an emergency contact">
				<p>We’ll alert them if you press SOS during a ride.</p>
			</Notice>
		{/if}
		<p class="from">
			<small>Pickup</small>
			<span>{current?.address ?? 'Finding your location…'}</span>
		</p>
		{#snippet footer()}
			<Button onclick={whereTo}>Where to?</Button>
			<Button variant="secondary" onclick={() => (stage = 'history')}>Your trips</Button>
			<Button variant="ghost" onclick={onsettings}>Settings</Button>
		{/snippet}
	</Screen>
{:else if stage === 'history'}
	<History onback={() => (stage = 'home')} />
{:else if stage === 'where'}
	<Search
		title="Where to?"
		label="Destination"
		onpick={(s) => {
			dropoff = s;
			stage = pickup ? 'options' : 'pickup';
		}}
		onback={() => (stage = 'home')}
	/>
{:else if stage === 'pickup'}
	<Search
		title="Pickup"
		label="Where should she pick you up?"
		{current}
		onpick={(s) => {
			pickup = s;
			stage = dropoff ? 'options' : 'where';
		}}
		onback={() => (stage = dropoff ? 'options' : 'home')}
	/>
{:else if stage === 'options' && pickup && dropoff}
	{#key `${pickup.lat},${pickup.lon}→${dropoff.lat},${dropoff.lon}`}
		<Options
			{pickup}
			{dropoff}
			onrequested={(r) => {
				ride = r;
				stage = 'ride';
			}}
			onchangepickup={() => (stage = 'pickup')}
			onback={() => (stage = 'where')}
		/>
	{/key}
{:else if stage === 'ride' && ride}
	<RideStatus {ride} onchange={(r) => (ride = r)} ondone={done} />
	{#if ride.ride_check}
		<!-- RideCheck (PRD S-3): asked by the server, shown over everything until she answers. -->
		<RideCheck check={ride.ride_check} />
	{/if}
	{#if WITH_DRIVER.includes(ride.state) || sosActive}
		<Sos rideId={ride.id} safetyDeskPhone={SAFETY_DESK_PHONE} bind:active={sosActive} />
	{/if}
	{#if CHAT_OPEN.includes(ride.state) && ride.driver}
		<!-- Shareable for as long as chat is open: from acceptance to drop-off (PRD S-2). -->
		<ShareTrip rideId={ride.id} />
		<Chat bind:this={chat} rideId={ride.id} party="rider" name={ride.driver.first_name} />
	{/if}
{/if}

<style>
	.splash {
		min-height: 100dvh;
		display: grid;
		place-content: center;
		font-size: var(--text-3xl);
		font-weight: 800;
		color: var(--color-brand);
	}
	.from {
		margin: 0;
		display: grid;
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.from small {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
</style>
