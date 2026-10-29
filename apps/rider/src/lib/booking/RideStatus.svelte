<!--
	The rider's ride after Request (App Flow §3, design brief §5.1 items 7–8): Finding your
	driver… with the 3 km radius pulse, No drivers accepted → Try again, Scheduled, Driver en
	route with the driver card, plate and PIN, and Cancelled. Updates arrive over the socket.
-->
<script lang="ts">
	import type { RiderRide } from '@fuber/api-client';
	import { messageOf, useSession } from '@fuber/account';
	import { Button, Notice, Plate, Screen, formatMinutes, formatNad } from '@fuber/ui';

	interface Props {
		ride: RiderRide;
		onchange: (ride: RiderRide) => void;
		ondone: () => void;
	}

	let { ride, onchange, ondone }: Props = $props();
	const session = useSession();
	let busy = $state(false);
	let error = $state<string | null>(null);
	let confirmCancel = $state(false);

	async function act(path: '/v1/rides/{id}/cancel' | '/v1/rides/{id}/retry') {
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST(path, { params: { path: { id: ride.id } } })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		confirmCancel = false;
		if (data) onchange(data);
		else error = messageOf(err);
	}

	const assigned = $derived(
		['DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED', 'PIN_VERIFIED', 'IN_TRIP'].includes(
			ride.state
		)
	);
	const when = $derived(
		ride.scheduled_for
			? new Date(ride.scheduled_for).toLocaleString('en-NA', {
					weekday: 'short',
					day: 'numeric',
					month: 'short',
					hour: '2-digit',
					minute: '2-digit'
				})
			: ''
	);
</script>

{#snippet trip()}
	<div class="trip">
		<p><small>From</small>{ride.pickup.address}</p>
		<p><small>To</small>{ride.dropoff.address}</p>
		<p>
			<small>Price (locked)</small>{formatNad(ride.fare.total)} · {ride.payment_method === 'cash'
				? 'Cash'
				: 'Card'}
		</p>
		{#if ride.guest_name}<p><small>Passenger</small>{ride.guest_name}</p>{/if}
	</div>
{/snippet}

{#snippet cancelButton(label: string)}
	{#if confirmCancel}
		<Notice tone="attention" title="Cancel this ride?">
			<p>There’s no fee to cancel.</p>
		</Notice>
		<Button variant="secondary" onclick={() => act('/v1/rides/{id}/cancel')} loading={busy}
			>Yes, cancel</Button
		>
		<Button variant="ghost" onclick={() => (confirmCancel = false)}>Keep my ride</Button>
	{:else}
		<Button variant="ghost" onclick={() => (confirmCancel = true)}>{label}</Button>
	{/if}
{/snippet}

{#if ride.state === 'SEARCHING' || ride.state === 'REQUESTED'}
	<Screen title="Finding your driver…" lead="We’re asking the best-matched driver near you.">
		<div class="radar" aria-hidden="true">
			<span class="ring"></span><span class="ring delay"></span><span class="centre"></span>
		</div>
		<p class="radius">Searching within 3 km of your pickup</p>
		{@render trip()}
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		{#snippet footer()}{@render cancelButton('Cancel request')}{/snippet}
	</Screen>
{:else if ride.state === 'NO_DRIVERS'}
	<Screen
		title="No drivers accepted"
		lead="Every nearby driver was busy or passed on this request. Try again, or choose another ride type."
	>
		{@render trip()}
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		{#snippet footer()}
			<Button onclick={() => act('/v1/rides/{id}/retry')} loading={busy}>Try again</Button>
			{@render cancelButton('Change ride')}
		{/snippet}
	</Screen>
{:else if ride.state === 'SCHEDULED'}
	<Screen title="Ride scheduled" lead="We’ll start finding your driver 15 minutes before pickup.">
		<Notice tone="success" title={when}><p>Pickup at {ride.pickup.address}.</p></Notice>
		{@render trip()}
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		{#snippet footer()}
			<Button variant="secondary" onclick={ondone}>Done</Button>
			{@render cancelButton('Cancel scheduled ride')}
		{/snippet}
	</Screen>
{:else if assigned && ride.driver}
	{@const d = ride.driver}
	<Screen
		title="{d.first_name} is on her way"
		lead={d.eta_s != null ? `Arriving in about ${formatMinutes(d.eta_s)}` : 'Arriving soon'}
	>
		<div class="driver">
			<div class="avatar" aria-hidden="true">{d.first_name.slice(0, 1)}</div>
			<div class="who">
				<strong>{d.first_name} <span class="verified">✓ Verified</span></strong>
				<small
					>{d.rating != null
						? `★ ${d.rating.toFixed(2)} · ${d.rating_count} trips`
						: 'New driver'}</small
				>
			</div>
		</div>
		<div class="car">
			<span>{d.vehicle.colour} {d.vehicle.make} {d.vehicle.model}</span>
			<Plate plate={d.vehicle.plate} />
		</div>
		<div class="pin" class:arrived={ride.state === 'DRIVER_ARRIVED'}>
			<small>{ride.state === 'DRIVER_ARRIVED' ? 'Show your driver this PIN' : 'Your PIN'}</small>
			<strong aria-label="PIN {ride.pin.split('').join(' ')}">{ride.pin}</strong>
			<small>Only get in once she enters it. Check the plate first.</small>
		</div>
		{@render trip()}
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		{#snippet footer()}{@render cancelButton('Cancel ride')}{/snippet}
	</Screen>
{:else if ride.state === 'CANCELLED'}
	<Screen
		title="Ride cancelled"
		lead={ride.cancelled_by === 'rider' ? 'You cancelled this ride.' : 'This ride was cancelled.'}
	>
		<Notice tone="info"><p>No fee charged.</p></Notice>
		{#snippet footer()}<Button onclick={ondone}>Back to home</Button>{/snippet}
	</Screen>
{:else}
	<Screen title="Your ride" lead="Status: {ride.state.toLowerCase().replaceAll('_', ' ')}">
		{@render trip()}
		{#snippet footer()}<Button onclick={ondone}>Back to home</Button>{/snippet}
	</Screen>
{/if}

<style>
	.radar {
		position: relative;
		height: 220px;
		display: grid;
		place-items: center;
	}
	.ring {
		position: absolute;
		width: 200px;
		height: 200px;
		border-radius: 50%;
		border: 2px solid var(--color-brand);
		animation: pulse 2.4s ease-out infinite;
		opacity: 0;
	}
	.ring.delay {
		animation-delay: 1.2s;
	}
	.pin {
		display: grid;
		justify-items: center;
		gap: var(--space-1);
		padding: var(--space-4);
		border-radius: var(--radius-md);
		background: var(--color-surface-tinted);
		text-align: center;
	}
	.centre {
		width: 18px;
		height: 18px;
		padding: 0;
		border-radius: 50%;
		background: var(--color-brand);
		box-shadow: 0 0 0 6px var(--color-surface);
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
		.ring {
			animation: none;
			opacity: 0.4;
		}
		.ring.delay {
			display: none;
		}
	}
	.radius {
		margin: 0;
		text-align: center;
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.trip {
		display: grid;
		gap: var(--space-2);
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.trip p {
		margin: 0;
		display: grid;
	}
	small {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.driver {
		display: flex;
		align-items: center;
		gap: var(--space-3);
	}
	.avatar {
		width: 56px;
		height: 56px;
		border-radius: 50%;
		display: grid;
		place-items: center;
		background: var(--color-brand);
		color: var(--color-on-brand);
		font-size: var(--text-xl);
		font-weight: 800;
	}
	.who {
		display: grid;
	}
	.verified {
		color: var(--color-success);
		font-size: var(--text-sm);
	}
	.car {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		font-weight: 600;
	}
	.pin strong {
		font-size: var(--text-3xl);
		letter-spacing: 0.3em;
		font-variant-numeric: tabular-nums;
	}
	.pin.arrived {
		background: var(--color-attention-fill);
		outline: 2px solid var(--color-attention);
	}
</style>
