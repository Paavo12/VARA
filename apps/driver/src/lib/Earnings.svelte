<!--
	Earnings and trips (PRD P-6, R-2): today or this week (trips, earnings, cash collected, hours
	online), the balance with VARA (negative = service fees she owes from cash trips, settled
	weekly), and her trip history, 20 at a time.
-->
<script lang="ts">
	import type { components } from '@fuber/api-client';
	import { messageOf, useSession } from '@fuber/account';
	import { Button, Notice, Screen, Segmented, formatMinutes, formatNad } from '@fuber/ui';
	import { onMount } from 'svelte';

	type Earnings = components['schemas']['Earnings'];
	type DriverTrip = components['schemas']['DriverTrip'];
	type Statement = components['schemas']['Statement'];

	interface Props {
		onback: () => void;
	}

	let { onback }: Props = $props();
	const session = useSession();

	let period = $state<'today' | 'week'>('today');
	let earnings = $state<Earnings | null>(null);
	let trips = $state<DriverTrip[]>([]);
	let more = $state(true);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let statements = $state<Statement[]>([]);

	async function loadStatements() {
		const { data } = await session.api
			.GET('/v1/driver/statements')
			.catch(() => ({ data: undefined }));
		if (data) statements = data;
	}

	async function loadEarnings(p: 'today' | 'week') {
		const { data, error: err } = await session.api
			.GET('/v1/driver/earnings', { params: { query: { period: p } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) earnings = data;
		else error = messageOf(err, 'Your earnings didn’t load. Check your connection.');
	}

	async function loadTrips() {
		busy = true;
		const before = trips.at(-1)?.requested_at;
		const { data } = await session.api
			.GET('/v1/driver/trips', { params: { query: { before } } })
			.catch(() => ({ data: undefined }));
		busy = false;
		if (!data) return;
		trips = [...trips, ...data];
		more = data.length === 20;
	}

	$effect(() => {
		void loadEarnings(period);
	});
	onMount(() => {
		void loadTrips();
		void loadStatements();
	});

	const STATES: Record<string, string> = {
		COMPLETED: 'Completed',
		RATED: 'Completed',
		CANCELLED: 'Cancelled',
		BLOCKED: 'Blocked'
	};
	const day = (iso: string) =>
		new Date(iso).toLocaleString('en-NA', {
			day: 'numeric',
			month: 'short',
			hour: '2-digit',
			minute: '2-digit'
		});
	/** A calendar date such as `2026-11-09`, without a time. */
	const date = (ymd: string) =>
		new Date(`${ymd}T12:00:00`).toLocaleDateString('en-NA', { day: 'numeric', month: 'short' });
</script>

<Screen title="Earnings" lead="What you earned after VARA’s service fee.">
	<Segmented
		label="Period"
		options={[
			{ value: 'today', label: 'Today' },
			{ value: 'week', label: 'This week' }
		]}
		bind:value={period}
	/>
	{#if error}<Notice tone="attention"><p>{error}</p></Notice>{/if}
	{#if earnings}
		<p class="total">{formatNad(earnings.earnings)}</p>
		<dl class="facts">
			<div>
				<dt>Trips</dt>
				<dd>{earnings.trips}</dd>
			</div>
			<div>
				<dt>Online</dt>
				<dd>{formatMinutes(earnings.online_s)}</dd>
			</div>
			<div>
				<dt>Cash collected</dt>
				<dd>{formatNad(earnings.cash_collected)}</dd>
			</div>
			<div>
				<dt>Service fees</dt>
				<dd>{formatNad(earnings.service_fees)}</dd>
			</div>
		</dl>
		<Notice tone={earnings.balance < 0 ? 'attention' : 'info'} title="Balance with VARA">
			<p>
				{earnings.balance < 0
					? `You owe VARA ${formatNad(-earnings.balance)} in service fees from cash trips. It’s settled weekly.`
					: earnings.balance > 0
						? `VARA owes you ${formatNad(earnings.balance)}. It’s paid out weekly.`
						: 'Nothing owed either way.'}
			</p>
		</Notice>
	{/if}

	{#if statements.length > 0}
		<h2>Weekly statements</h2>
		<ul class="trips">
			{#each statements as st (st.id)}
				<li>
					<div>
						<strong>{date(st.period_start)} – {date(st.period_end)}</strong>
						<small
							>{st.status === 'pending'
								? st.amount < 0
									? 'To pay VARA'
									: 'To be paid to you'
								: st.status === 'received'
									? 'Paid. Thank you'
									: 'Paid to you'}</small
						>
					</div>
					<div class="right">
						<strong>{formatNad(Math.abs(st.amount))}</strong>
					</div>
				</li>
			{/each}
		</ul>
	{/if}

	<h2>Your trips</h2>
	{#if trips.length === 0 && !busy}
		<p class="muted">No trips yet. Completed trips appear here.</p>
	{/if}
	<ul class="trips">
		{#each trips as trip (trip.id)}
			<li>
				<div>
					<strong>{trip.dropoff_address ?? trip.pickup_address ?? 'Trip ' + trip.code}</strong>
					<small
						>{day(trip.completed_at ?? trip.requested_at)} · {trip.code} · {STATES[trip.state] ??
							trip.state}</small
					>
				</div>
				<div class="right">
					<strong
						>{trip.state === 'COMPLETED' || trip.state === 'RATED'
							? formatNad(trip.earning)
							: '—'}</strong
					>
					{#if trip.payment_method === 'cash' && trip.payment_status === 'pending'}
						<small class="due">Cash not confirmed</small>
					{/if}
				</div>
			</li>
		{/each}
	</ul>
	{#if more && trips.length > 0}
		<Button variant="ghost" onclick={loadTrips} loading={busy}>Show older trips</Button>
	{/if}
	{#snippet footer()}<Button variant="secondary" onclick={onback}>Back</Button>{/snippet}
</Screen>

<style>
	.total {
		margin: 0;
		text-align: center;
		font-size: var(--text-3xl);
		font-weight: 800;
		font-variant-numeric: tabular-nums;
	}
	.facts {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: var(--space-2);
		margin: 0;
	}
	.facts div {
		padding: var(--space-3);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	dt {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	dd {
		margin: 0;
		font-weight: 700;
		font-variant-numeric: tabular-nums;
	}
	h2 {
		margin: var(--space-3) 0 0;
		font-size: var(--text-lg);
	}
	.muted {
		margin: 0;
		color: var(--color-text-muted);
	}
	.trips {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-2);
	}
	.trips li {
		display: flex;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-3);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.trips li > div {
		display: grid;
		min-width: 0;
	}
	.right {
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	small {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	small.due {
		color: var(--color-attention);
	}
</style>
