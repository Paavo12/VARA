<!--
	The rider's ride after Request (App Flow §3, design brief §5.1 items 7–8): Finding your
	driver… with the 3 km radius pulse, No drivers accepted → Try again, Scheduled, Driver en
	route with the driver card, plate and PIN, arrived (PIN card amber, free-wait time), PIN
	confirmed, in trip, You've arrived (fare to pay), blocked after wrong PINs, and Cancelled.
	Updates arrive over the socket.
-->
<script lang="ts">
	import type { RiderRide } from '@fuber/api-client';
	import { messageOf, ReportIssue, ShareTrip, useSession } from '@fuber/account';
	import { Button, Notice, Plate, Screen, Stars, formatMinutes, formatNad } from '@fuber/ui';

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
	let stars = $state(0);
	let lowReasons = $state<string[]>([]);
	let favourite = $state(false);

	type Reason =
		'unsafe_driving' | 'car_condition' | 'route' | 'late' | 'rude' | 'felt_unsafe' | 'other';
	const LOW_REASONS: { id: Reason; label: string }[] = [
		{ id: 'unsafe_driving', label: 'Unsafe driving' },
		{ id: 'felt_unsafe', label: 'I didn’t feel safe' },
		{ id: 'late', label: 'Late' },
		{ id: 'rude', label: 'Rude' },
		{ id: 'car_condition', label: 'Car not clean or not as shown' },
		{ id: 'route', label: 'Took a strange route' },
		{ id: 'other', label: 'Something else' }
	];

	async function rate() {
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST('/v1/rides/{id}/rating', {
				params: { path: { id: ride.id } },
				body: {
					stars,
					reasons: stars < 3 ? (lowReasons as Reason[]) : [],
					comment: null,
					favourite: stars >= 4 && favourite
				}
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) onchange({ ...ride, rated: true, state: data.state as RiderRide['state'] });
		else error = messageOf(err, 'We couldn’t send your rating. Check your connection.');
	}

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
	const beforePin = $derived(
		['DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED'].includes(ride.state)
	);
	const clock = (iso: string) =>
		new Date(iso).toLocaleTimeString('en-NA', { hour: '2-digit', minute: '2-digit' });

	function heading(driver: string): string {
		if (ride.state === 'DRIVER_ARRIVED') return `${driver} has arrived`;
		if (ride.state === 'PIN_VERIFIED') return 'You’re all set';
		if (ride.state === 'IN_TRIP') return `On your way to ${ride.dropoff.address}`;
		return `${driver} is on her way`;
	}

	function lead(etaS: number | null | undefined): string {
		if (ride.state === 'DRIVER_ARRIVED') {
			if (!ride.wait_started_at) return 'Meet her at the pickup.';
			const ends = Date.parse(ride.wait_started_at) + 5 * 60_000;
			return `She’s waiting for you. Free wait until ${clock(new Date(ends).toISOString())}.`;
		}
		if (ride.state === 'PIN_VERIFIED') return 'Your driver will start the trip.';
		if (ride.state === 'IN_TRIP')
			return `The trip takes about ${formatMinutes(ride.trip_duration_s)}.`;
		return etaS != null ? `Arriving in about ${formatMinutes(etaS)}` : 'Arriving soon';
	}

	const cancelledLead = $derived(
		ride.cancelled_by === 'rider'
			? 'You cancelled this ride.'
			: ride.cancelled_by === 'driver'
				? 'Your driver waited 5 minutes at the pickup and then cancelled.'
				: 'This ride was cancelled.'
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
	<Screen title={heading(d.first_name)} lead={lead(d.eta_s)}>
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
		{#if beforePin}
			<div class="pin" class:arrived={ride.state === 'DRIVER_ARRIVED'}>
				<small>{ride.state === 'DRIVER_ARRIVED' ? 'Show your driver this PIN' : 'Your PIN'}</small>
				<strong
					><span aria-hidden="true">{ride.pin}</span><span class="sr-only"
						>{ride.pin.split('').join(' ')}</span
					></strong
				>
				<small>Only get in once she enters it. Check the plate first.</small>
			</div>
		{:else}
			<Notice tone="success" title="PIN confirmed">
				<p>Have a safe trip. Share your trip with someone you trust.</p>
			</Notice>
		{/if}
		{@render trip()}
		<!-- Share trip (PRD S-2) from acceptance to drop-off, in view under the trip details. -->
		<ShareTrip rideId={ride.id} />
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		{#snippet footer()}
			{#if beforePin}{@render cancelButton('Cancel ride')}{/if}
		{/snippet}
	</Screen>
{:else if ride.state === 'COMPLETED' || ride.state === 'RATED'}
	<Screen title="You’ve arrived" lead="Trip {ride.code} to {ride.dropoff.address}">
		<p class="total">{formatNad(ride.fare.total)}</p>
		<Notice tone="success">
			<p>
				{ride.payment_method === 'card'
					? 'Paid by card. Your receipt is on its way.'
					: ride.payment_status === 'cash_confirmed'
						? 'Paid in cash. Thank you.'
						: 'Please pay your driver this amount in cash.'}
			</p>
		</Notice>
		<dl class="breakdown">
			<div>
				<dt>Base</dt>
				<dd>{formatNad(ride.fare.base)}</dd>
			</div>
			<div>
				<dt>Distance</dt>
				<dd>{formatNad(ride.fare.distance)}</dd>
			</div>
			<div>
				<dt>Time</dt>
				<dd>{formatNad(ride.fare.time)}</dd>
			</div>
			{#if ride.fare.minimum_adjustment > 0}
				<div>
					<dt>Minimum fare</dt>
					<dd>{formatNad(ride.fare.minimum_adjustment)}</dd>
				</div>
			{/if}
			{#if (ride.fare.kids ?? 0) > 0}<div>
					<dt>Child seat</dt>
					<dd>{formatNad(ride.fare.kids ?? 0)}</dd>
				</div>{/if}
			{#if ride.fare.surge > 0}<div>
					<dt>Busy time</dt>
					<dd>{formatNad(ride.fare.surge)}</dd>
				</div>{/if}
			<div class="sum">
				<dt>Total (locked at booking)</dt>
				<dd>{formatNad(ride.fare.total)}</dd>
			</div>
		</dl>
		{#if !ride.rated && ride.driver}
			<Stars label="How was your trip with {ride.driver.first_name}?" bind:value={stars} />
			{#if stars > 0 && stars < 3}
				<fieldset class="reasons">
					<legend>What went wrong?</legend>
					{#each LOW_REASONS as r (r.id)}
						<label><input type="checkbox" value={r.id} bind:group={lowReasons} /> {r.label}</label>
					{/each}
				</fieldset>
			{/if}
			{#if stars >= 4}
				<label class="fav"
					><input type="checkbox" bind:checked={favourite} /> Add {ride.driver.first_name} to my favourite
					drivers</label
				>
			{/if}
		{:else if ride.rated}
			<Notice tone="info"><p>Thanks for rating your trip.</p></Notice>
		{/if}
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		<ReportIssue rideId={ride.id} party="rider" />
		{#snippet footer()}
			{#if !ride.rated && ride.driver}
				<Button
					onclick={rate}
					loading={busy}
					disabled={stars === 0 || (stars < 3 && lowReasons.length === 0)}>Send rating</Button
				>
				<Button variant="ghost" onclick={ondone}>Not now</Button>
			{:else}
				<Button onclick={ondone}>Done</Button>
			{/if}
		{/snippet}
	</Screen>
{:else if ride.state === 'BLOCKED'}
	<Screen title="Trip stopped for your safety" lead="Your driver entered the wrong PIN too often.">
		<Notice tone="attention" title="Don’t get in the car">
			<p>VARA’s safety team has been alerted and will call you shortly.</p>
		</Notice>
		{#snippet footer()}<Button variant="secondary" onclick={ondone}>Back to home</Button>{/snippet}
	</Screen>
{:else if ride.state === 'CANCELLED'}
	<Screen title="Ride cancelled" lead={cancelledLead}>
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
	.breakdown {
		display: grid;
		gap: var(--space-1);
		margin: 0;
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-md);
		background: var(--color-surface);
		font-variant-numeric: tabular-nums;
	}
	.breakdown div {
		display: flex;
		justify-content: space-between;
	}
	.breakdown dt {
		color: var(--color-text-muted);
	}
	.breakdown dd {
		margin: 0;
	}
	.breakdown .sum {
		padding-top: var(--space-1);
		border-top: 1px solid var(--color-border);
		font-weight: 700;
	}
	.reasons {
		display: grid;
		gap: var(--space-1);
		border: 0;
		margin: 0;
		padding: 0;
	}
	.reasons label,
	.fav {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--touch-target);
	}
	.total {
		margin: 0;
		text-align: center;
		font-size: var(--text-3xl);
		font-weight: 800;
		font-variant-numeric: tabular-nums;
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
