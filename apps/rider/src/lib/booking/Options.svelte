<!--
	Ride options (PRD B-3 – B-7, design brief §5.1 item 6): three ride types with an exact,
	locked price and live pickup ETA; passengers; wheelchair; For me / Someone else; Now /
	Schedule; payment. The price shown is the price charged (PRD P-1): Request sends the signed
	quote, and an expired quote is refreshed before asking the rider to confirm again.
-->
<script lang="ts">
	import type { Quote, RiderRide, Tier } from '@fuber/api-client';
	import { messageOf, normalisePhone, useSession } from '@fuber/account';
	import {
		Button,
		Notice,
		Screen,
		Segmented,
		TextField,
		formatMinutes,
		formatNad
	} from '@fuber/ui';
	import { onMount } from 'svelte';
	import type { Stop } from './types';

	interface Props {
		pickup: Stop;
		dropoff: Stop;
		onrequested: (ride: RiderRide) => void;
		onchangepickup: () => void;
		onback: () => void;
	}

	let { pickup, dropoff, onrequested, onchangepickup, onback }: Props = $props();
	const session = useSession();

	const NAMES: Record<Tier, { name: string; blurb: string }> = {
		classic: { name: 'Classic', blurb: 'Affordable everyday rides' },
		comfort: { name: 'Comfort', blurb: 'Roomier cars · top-rated drivers' },
		softlife: { name: 'Soft Life', blurb: 'Premium cars · the full treatment' }
	};

	let quote = $state<Quote | null>(null);
	let tier = $state<Tier>('classic');
	let passengers = $state(1);
	let wheelchair = $state(false);
	let forWhom = $state<'me' | 'guest'>('me');
	let guestName = $state('');
	let guestPhone = $state('');
	let when = $state<'now' | 'later'>('now');
	let at = $state('');
	let payment = $state<'cash' | 'card'>('cash');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);
	// One key per booking attempt: a retried request returns the same ride, never a second one.
	let idempotencyKey = crypto.randomUUID();

	async function loadQuote(): Promise<boolean> {
		const { data, error: err } = await session.api
			.POST('/v1/rides/quote', {
				body: {
					pickup: { lat: pickup.lat, lon: pickup.lon },
					pickup_address: pickup.address,
					dropoff: { lat: dropoff.lat, lon: dropoff.lon },
					dropoff_address: dropoff.address
				}
			})
			.catch(() => ({ data: undefined, error: undefined }));
		if (!data) {
			error = messageOf(err, 'We couldn’t get prices. Check your connection.');
			return false;
		}
		quote = data;
		// Keep the choice if it is still bookable now, else pick the cheapest with a driver.
		const option = data.options.find((o) => o.tier === tier);
		if (when === 'now' && option?.pickup_eta_s == null) {
			tier = data.options.find((o) => o.pickup_eta_s != null)?.tier ?? tier;
		}
		return true;
	}

	onMount(() => {
		void loadQuote();
	});

	const chosen = $derived(quote?.options.find((o) => o.tier === tier));
	// A ride now needs a driver of that type nearby; a scheduled ride is dispatched later.
	const bookable = (eta: number | null | undefined) => when === 'later' || eta != null;
	const minSchedule = $derived(localInput(new Date(Date.now() + 31 * 60_000)));
	const maxSchedule = $derived(localInput(new Date(Date.now() + 7 * 24 * 3600_000)));

	function localInput(d: Date): string {
		const p = (n: number) => String(n).padStart(2, '0');
		return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
	}

	async function request() {
		error = null;
		notice = null;
		if (!quote || !chosen) return;
		let guest = null;
		if (forWhom === 'guest') {
			const phone = normalisePhone(guestPhone);
			if (!guestName.trim() || !phone)
				return void (error = 'Add the passenger’s name and Namibian mobile number.');
			guest = { name: guestName.trim(), phone };
		}
		let scheduled_for: string | null = null;
		if (when === 'later') {
			if (!at) return void (error = 'Choose when you want to be picked up.');
			scheduled_for = new Date(at).toISOString();
		}
		busy = true;
		const send = () =>
			session.api
				.POST('/v1/rides', {
					params: { header: { 'Idempotency-Key': idempotencyKey } },
					body: {
						quote: quote!.quote,
						tier,
						passengers,
						wheelchair,
						guest,
						payment_method: payment,
						scheduled_for
					}
				})
				.catch(() => ({ data: undefined, error: undefined, response: undefined }));
		const { data, error: err, response } = await send();
		busy = false;
		if (data) return onrequested(data);
		const message = messageOf(err, 'We couldn’t request your ride. Please try again.');
		if (response?.status === 422 && message.includes('expired')) {
			// The 5-minute price lock ran out: fetch the current price and let her confirm it.
			const before = chosen.fare.total;
			idempotencyKey = crypto.randomUUID();
			if (await loadQuote()) {
				const now = quote!.options.find((o) => o.tier === tier)?.fare.total;
				notice =
					now === before
						? 'Your price was refreshed. Tap Request again.'
						: `Prices changed while you were deciding. ${NAMES[tier].name} is now ${formatNad(now ?? 0)}.`;
			}
			return;
		}
		error = message;
	}
</script>

<Screen title="Choose a ride" {onback}>
	<div class="route">
		<button class="leg" onclick={onchangepickup}>
			<span class="dot from" aria-hidden="true"></span>
			<span><small>Pickup</small>{pickup.address}</span>
		</button>
		<div class="leg">
			<span class="dot to" aria-hidden="true"></span>
			<span><small>Destination</small>{dropoff.address}</span>
		</div>
	</div>

	{#if !quote && !error}
		<p class="hint" aria-live="polite">Getting your prices…</p>
	{:else if quote}
		<fieldset class="tiers">
			<legend>Ride type</legend>
			{#each quote.options as o (o.tier)}
				<label class="tier" class:on={tier === o.tier} class:off={!bookable(o.pickup_eta_s)}>
					<input
						type="radio"
						name="tier"
						value={o.tier}
						bind:group={tier}
						disabled={!bookable(o.pickup_eta_s)}
					/>
					<span class="name"
						><strong>{NAMES[o.tier].name}</strong><small>{NAMES[o.tier].blurb}</small></span
					>
					<span class="price">
						<strong>{formatNad(o.fare.total)}</strong>
						<small
							>{o.pickup_eta_s != null
								? `${formatMinutes(o.pickup_eta_s)} away`
								: 'No driver online within 3 km'}</small
						>
					</span>
				</label>
			{/each}
		</fieldset>
		<p class="hint">
			{(quote.distance_m / 1000).toFixed(1)} km · about {formatMinutes(quote.duration_s)}. The price
			is locked when you request{quote.demand !== 'normal'
				? ' and includes busy-time pricing'
				: ''}.
		</p>

		<div class="row">
			<span class="label" id="pax">Passengers</span>
			<div class="stepper" role="group" aria-labelledby="pax">
				<button
					aria-label="Fewer passengers"
					disabled={passengers <= 1}
					onclick={() => (passengers -= 1)}>−</button
				>
				<output aria-live="polite">{passengers}</output>
				<button
					aria-label="More passengers"
					disabled={passengers >= 4}
					onclick={() => (passengers += 1)}>+</button
				>
			</div>
		</div>
		<p class="hint">All women, never shared with strangers. The price doesn’t change.</p>
		<label class="row toggle">
			<span class="label">Wheelchair-accessible car</span>
			<input type="checkbox" bind:checked={wheelchair} />
		</label>

		<Segmented
			label="Who is riding?"
			options={[
				{ value: 'me', label: 'For me' },
				{ value: 'guest', label: 'Someone else' }
			]}
			bind:value={forWhom}
		/>
		{#if forWhom === 'guest'}
			<TextField label="Passenger’s name" bind:value={guestName} autocomplete="off" />
			<TextField
				label="Passenger’s mobile number"
				bind:value={guestPhone}
				type="tel"
				inputmode="tel"
				hint="We text her the driver, car, plate and PIN."
			/>
		{/if}

		<Segmented
			label="When?"
			options={[
				{ value: 'now', label: 'Now' },
				{ value: 'later', label: 'Schedule' }
			]}
			bind:value={when}
		/>
		{#if when === 'later'}
			<div class="field">
				<label for="when">Pickup time</label>
				<input
					id="when"
					type="datetime-local"
					bind:value={at}
					min={minSchedule}
					max={maxSchedule}
				/>
				<small>30 minutes to 7 days from now.</small>
			</div>
		{/if}

		<Segmented
			label="Payment"
			options={[
				{ value: 'cash', label: 'Cash' },
				{ value: 'card', label: 'Card (soon)', disabled: true }
			]}
			bind:value={payment}
		/>
	{/if}

	{#if notice}<Notice tone="attention"><p>{notice}</p></Notice>{/if}
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}

	{#snippet footer()}
		{#if quote && chosen}
			<Button onclick={request} loading={busy} disabled={!bookable(chosen.pickup_eta_s)}>
				{when === 'later' ? 'Schedule' : 'Request'}
				{NAMES[tier].name} · {formatNad(chosen.fare.total)}
			</Button>
		{:else if error}
			<Button variant="secondary" onclick={() => ((error = null), loadQuote())}>Try again</Button>
		{/if}
	{/snippet}
</Screen>

<style>
	.route {
		display: grid;
		gap: var(--space-1);
		padding: var(--space-2);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.leg {
		display: flex;
		gap: var(--space-3);
		align-items: center;
		min-height: var(--touch-target);
		padding: 0 var(--space-2);
		border: none;
		background: none;
		color: var(--color-text);
		font: inherit;
		text-align: left;
	}
	button.leg {
		cursor: pointer;
	}
	.leg span:last-child {
		display: grid;
	}
	small {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.dot {
		width: 12px;
		height: 12px;
		border-radius: 50%;
		flex: none;
	}
	.from {
		background: var(--color-brand);
	}
	.to {
		background: var(--color-route);
	}
	.tiers {
		border: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-2);
	}
	legend {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip-path: inset(50%);
	}
	.tier {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		border: 2px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
		cursor: pointer;
	}
	.tier.on {
		border-color: var(--color-brand);
	}
	.tier.off {
		opacity: 0.55;
		cursor: not-allowed;
	}
	.tier:has(input:focus-visible) {
		outline: 3px solid var(--color-focus-ring);
		outline-offset: 2px;
	}
	.tier input {
		position: absolute;
		opacity: 0;
		pointer-events: none;
	}
	.name,
	.price {
		display: grid;
	}
	.price {
		text-align: right;
	}
	.price strong {
		font-size: var(--text-lg);
	}
	.hint {
		margin: 0;
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		min-height: var(--touch-target);
	}
	.label {
		font-weight: 600;
	}
	.toggle input {
		width: 24px;
		height: 24px;
		accent-color: var(--color-brand);
	}
	.stepper {
		display: flex;
		align-items: center;
		gap: var(--space-3);
	}
	.stepper button {
		width: var(--touch-target);
		height: var(--touch-target);
		border: 1px solid var(--color-border);
		border-radius: 50%;
		background: var(--color-surface);
		color: var(--color-brand);
		font: inherit;
		font-size: var(--text-xl);
		cursor: pointer;
	}
	.stepper button:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	output {
		min-width: 1.5ch;
		text-align: center;
		font-weight: 700;
		font-size: var(--text-lg);
	}
	.field {
		display: grid;
		gap: var(--space-1);
	}
	.field label {
		font-weight: 600;
		font-size: var(--text-sm);
		color: var(--color-text-secondary);
	}
	.field input {
		min-height: var(--touch-target);
		padding: 0 var(--space-3);
		border: 1px solid var(--color-border-control);
		border-radius: var(--radius-sm);
		background: var(--color-surface);
		color: var(--color-text);
		font: inherit;
	}
</style>
