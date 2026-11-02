<!--
	The trip from the driver's side (App Flow §5, PRD T-2 – T-7): to the fuzzy pickup circle, the
	exact pickup within 50 m with the PIN keypad and free-wait timer, destination unlocked after the
	right PIN, in trip with slide-to-complete, and the end states. Every action goes to the API;
	the screen follows the ride the API returns (and the socket pushes).
-->
<script lang="ts">
	import type { DriverRide } from '@fuber/api-client';
	import { messageOf, useSession } from '@fuber/account';
	import { Button, Notice, Screen, formatNad } from '@fuber/ui';
	import { onMount } from 'svelte';

	interface Props {
		ride: DriverRide;
		onchange: (ride: DriverRide) => void;
		/** She cancelled: the request went back to other drivers. */
		onreleased: () => void;
		ondone: () => void;
	}

	let { ride, onchange, onreleased, ondone }: Props = $props();
	const session = useSession();

	const NAMES: Record<string, string> = {
		classic: 'Classic',
		comfort: 'Comfort',
		softlife: 'Soft Life'
	};
	const REASONS = [
		{ id: 'rider_asked_me', label: 'The passenger asked me to cancel' },
		{ id: 'cant_reach_pickup', label: 'I can’t reach the pickup' },
		{ id: 'feel_unsafe', label: 'I don’t feel safe' },
		{ id: 'vehicle_problem', label: 'Problem with my car' },
		{ id: 'other', label: 'Something else' }
	] as const;
	type Reason = (typeof REASONS)[number]['id'];

	let busy = $state(false);
	let error = $state<string | null>(null);
	let pin = $state('');
	let shake = $state(false);
	let cancelling = $state(false);
	let reason = $state<Reason | null>(null);
	let slide = $state(0);
	let now = $state(Date.now());

	onMount(() => {
		const t = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(t);
	});

	type Path =
		| '/v1/rides/{id}/arrive'
		| '/v1/rides/{id}/wait'
		| '/v1/rides/{id}/no-show'
		| '/v1/rides/{id}/start'
		| '/v1/rides/{id}/complete';

	async function act(path: Path) {
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST(path, { params: { path: { id: ride.id } } })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) onchange(data);
		else error = messageOf(err, 'That didn’t go through. Check your connection and try again.');
	}

	async function sendPin() {
		if (pin.length !== 4) return;
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST('/v1/rides/{id}/pin', { params: { path: { id: ride.id } }, body: { pin } })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		pin = '';
		if (data) return onchange(data);
		error = messageOf(err, 'We couldn’t check the PIN. Check your connection.');
		shake = true;
		setTimeout(() => (shake = false), 400);
		// The attempts left (or the blocked trip) come from the API.
		const { data: s } = await session.api
			.GET('/v1/driver/status')
			.catch(() => ({ data: undefined }));
		if (s?.ride) onchange(s.ride);
	}

	function press(d: string) {
		if (pin.length < 4) pin += d;
		if (pin.length === 4) void sendPin();
	}

	async function cancel() {
		if (!reason) return;
		error = null;
		busy = true;
		const { response, error: err } = await session.api
			.POST('/v1/rides/{id}/driver-cancel', {
				params: { path: { id: ride.id } },
				body: { reason }
			})
			.catch(() => ({ response: undefined, error: undefined }));
		busy = false;
		if (response?.ok) onreleased();
		else error = messageOf(err, 'We couldn’t cancel. Check your connection.');
	}

	function onslide() {
		if (slide >= 100 && !busy) void act('/v1/rides/{id}/complete').then(() => (slide = 0));
	}

	const waitLeft = $derived(
		ride.no_show_from ? Math.max(0, Math.ceil((Date.parse(ride.no_show_from) - now) / 1000)) : null
	);
	const mmss = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
	const canCancel = $derived(
		['DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED'].includes(ride.state)
	);
</script>

{#snippet tripCard()}
	<div class="card">
		<p class="earning">{formatNad(ride.earning)}</p>
		<p><small>Passenger</small><strong>{ride.passenger.name}</strong></p>
		{#if ride.passenger.booked_by}<p><small>{ride.passenger.booked_by}</small></p>{/if}
		<p>
			<small>Passengers</small>{ride.passengers}{ride.wheelchair ? ' · wheelchair' : ''} ·
			{ride.payment_method === 'cash' ? 'Cash' : 'Card'}
		</p>
	</div>
{/snippet}

{#snippet cancelPanel()}
	{#if cancelling}
		<fieldset class="reasons">
			<legend>Why are you cancelling? It’s free, but it lowers your priority.</legend>
			{#each REASONS as r (r.id)}
				<label
					><input type="radio" name="reason" value={r.id} bind:group={reason} /> {r.label}</label
				>
			{/each}
		</fieldset>
		<Button variant="secondary" onclick={cancel} loading={busy} disabled={!reason}
			>Cancel this trip</Button
		>
		<Button variant="ghost" onclick={() => (cancelling = false)}>Keep the trip</Button>
	{:else if canCancel}
		<Button variant="ghost" onclick={() => (cancelling = true)}>Cancel trip…</Button>
	{/if}
{/snippet}

{#if ride.state === 'DRIVER_ASSIGNED' || ride.state === 'DRIVER_EN_ROUTE'}
	<Screen title="Head to the pickup" lead="Trip {ride.code} · {NAMES[ride.tier] ?? ride.tier}">
		<div class="area">
			<small>Pickup area</small>
			<strong>{ride.pickup_area.label}</strong>
			<small
				>Drive into the {ride.pickup_area.radius_m} m circle. The exact spot shows within 50 m of it.</small
			>
		</div>
		{@render tripCard()}
		{#if error}<Notice tone="attention"><p>{error}</p></Notice>{/if}
		{#snippet footer()}
			<Button size="driver" onclick={() => act('/v1/rides/{id}/arrive')} loading={busy}
				>I’ve arrived</Button
			>
			{@render cancelPanel()}
		{/snippet}
	</Screen>
{:else if ride.state === 'DRIVER_ARRIVED'}
	<Screen title="You’ve arrived" lead="Ask {ride.passenger.name} for her 4-digit PIN.">
		<div class="area exact">
			<small>Exact pickup</small>
			<strong>{ride.pickup?.address}</strong>
		</div>
		<div class="pin" class:shake>
			<div class="digits" aria-label="PIN entered: {pin.length} of 4 digits">
				{#each [0, 1, 2, 3] as i (i)}<span class:filled={i < pin.length}></span>{/each}
			</div>
			<div class="keys">
				{#each ['1', '2', '3', '4', '5', '6', '7', '8', '9'] as d (d)}
					<button type="button" onclick={() => press(d)} disabled={busy}>{d}</button>
				{/each}
				<button type="button" class="soft" onclick={() => (pin = pin.slice(0, -1))} disabled={busy}
					>⌫ <span class="sr">Delete</span></button
				>
				<button type="button" onclick={() => press('0')} disabled={busy}>0</button>
				<span></span>
			</div>
			<p class="attempts">
				{ride.pin_attempts_left} attempt{ride.pin_attempts_left === 1 ? '' : 's'} left
			</p>
		</div>
		{#if error}<Notice tone="attention"><p>{error}</p></Notice>{/if}
		{#if ride.wait_started_at && waitLeft != null}
			<Notice tone="info" title={waitLeft > 0 ? `Free wait: ${mmss(waitLeft)}` : 'Free wait over'}>
				<p>
					{waitLeft > 0
						? 'If she doesn’t come, you can cancel as a no-show when the timer ends.'
						: 'You can cancel as a no-show. It counts against the passenger, not you.'}
				</p>
			</Notice>
		{/if}
		{#snippet footer()}
			{#if !ride.wait_started_at}
				<Button variant="secondary" onclick={() => act('/v1/rides/{id}/wait')} loading={busy}
					>Start 5-minute wait</Button
				>
			{:else if waitLeft === 0}
				<Button variant="secondary" onclick={() => act('/v1/rides/{id}/no-show')} loading={busy}
					>Passenger didn’t come</Button
				>
			{/if}
			{@render cancelPanel()}
		{/snippet}
	</Screen>
{:else if ride.state === 'PIN_VERIFIED'}
	<Screen title="Destination unlocked" lead="PIN correct. Check she’s seated and buckled in.">
		<div class="area exact">
			<small>Destination</small>
			<strong>{ride.dropoff?.address}</strong>
		</div>
		{@render tripCard()}
		{#if error}<Notice tone="attention"><p>{error}</p></Notice>{/if}
		{#snippet footer()}
			<Button size="driver" onclick={() => act('/v1/rides/{id}/start')} loading={busy}
				>Start trip</Button
			>
		{/snippet}
	</Screen>
{:else if ride.state === 'IN_TRIP'}
	<Screen title="On the way" lead="Drive to {ride.dropoff?.address}.">
		<div class="area exact">
			<small>Destination</small>
			<strong>{ride.dropoff?.address}</strong>
		</div>
		{@render tripCard()}
		{#if error}<Notice tone="attention"><p>{error}</p></Notice>{/if}
		{#snippet footer()}
			<label class="slider">
				<span>{busy ? 'Completing…' : 'Slide to complete the trip'}</span>
				<input
					type="range"
					min="0"
					max="100"
					bind:value={slide}
					onchange={onslide}
					disabled={busy}
					aria-label="Slide to complete the trip"
				/>
			</label>
		{/snippet}
	</Screen>
{:else if ride.state === 'COMPLETED' || ride.state === 'RATED'}
	<Screen title="Trip complete" lead="Trip {ride.code}">
		<p class="earning">{formatNad(ride.earning)}</p>
		<Notice tone="success">
			<p>
				{ride.payment_method === 'cash'
					? 'Collect the fare in cash. FUber’s service fee is added to your balance.'
					: 'Paid by card. Your earnings arrive with the weekly payout.'}
			</p>
		</Notice>
		{#snippet footer()}<Button size="driver" onclick={ondone}>Done</Button>{/snippet}
	</Screen>
{:else if ride.state === 'BLOCKED'}
	<Screen title="Trip blocked" lead="Too many wrong PINs.">
		<Notice tone="attention" title="Don’t start the trip">
			<p>FUber’s safety team has been alerted and will call you. Don’t let anyone in the car.</p>
		</Notice>
		{#snippet footer()}<Button variant="secondary" onclick={ondone}>Back</Button>{/snippet}
	</Screen>
{:else}
	<Screen title="Trip cancelled" lead="Trip {ride.code}">
		<Notice tone="info"><p>No fee for you.</p></Notice>
		{#snippet footer()}<Button onclick={ondone}>Back</Button>{/snippet}
	</Screen>
{/if}

<style>
	.card,
	.area {
		display: grid;
		gap: var(--space-2);
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.area strong {
		font-size: var(--text-lg);
	}
	.exact {
		outline: 2px solid var(--color-brand);
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
	.pin {
		display: grid;
		gap: var(--space-3);
		justify-items: center;
	}
	.digits {
		display: flex;
		gap: var(--space-3);
	}
	.digits span {
		width: 18px;
		height: 18px;
		border-radius: 50%;
		border: 2px solid var(--color-brand);
	}
	.digits span.filled {
		background: var(--color-brand);
	}
	.keys {
		display: grid;
		grid-template-columns: repeat(3, 72px);
		gap: var(--space-2);
	}
	.keys button {
		height: 56px;
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
		color: var(--color-text);
		font: inherit;
		font-size: var(--text-xl);
		font-weight: 700;
		cursor: pointer;
	}
	.keys .soft {
		font-size: var(--text-lg);
		color: var(--color-text-muted);
	}
	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
	.attempts {
		margin: 0;
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.shake {
		animation: shake 0.35s;
	}
	@keyframes shake {
		25% {
			transform: translateX(-8px);
		}
		75% {
			transform: translateX(8px);
		}
	}
	.reasons {
		display: grid;
		gap: var(--space-2);
		border: 0;
		padding: 0;
		margin: 0;
	}
	.reasons legend {
		margin-bottom: var(--space-2);
		color: var(--color-text-secondary);
	}
	.reasons label {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--touch-target);
	}
	.slider {
		display: grid;
		gap: var(--space-2);
		text-align: center;
		font-weight: 700;
	}
	.slider input {
		width: 100%;
		height: var(--touch-target-driver);
		accent-color: var(--color-brand);
	}
	@media (prefers-reduced-motion: reduce) {
		.shake {
			animation: none;
		}
	}
</style>
