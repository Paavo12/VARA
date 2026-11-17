<!--
	Numbers (PRD O-8): trips, cancellations, matching, safety and verification over the last
	1, 7 or 30 days, and what is happening right now. Counts only, for every staff role.
-->
<script lang="ts">
	import { Notice, Segmented } from '@fuber/ui';
	import { messageOf } from '#lib/format.ts';
	import { api, type Metrics } from '#lib/staff.svelte.ts';

	let days = $state<'1' | '7' | '30'>('7');
	let m = $state<Metrics | null>(null);
	let error = $state<string | null>(null);

	async function load(d: number) {
		error = null;
		const { data, error: err } = await api
			.GET('/v1/ops/metrics', { params: { query: { days: d } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) m = data;
		else error = messageOf(err, 'The numbers didn’t load.');
	}

	const pct = (x: number | null | undefined) => (x == null ? '—' : `${Math.round(x * 100)} %`);
	const num = (x: number | null | undefined, unit: string) =>
		x == null ? '—' : `${x.toFixed(1)} ${unit}`;

	$effect(() => {
		void load(Number(days));
	});
</script>

<svelte:head><title>Numbers · VARA Ops</title></svelte:head>

<h1>Numbers</h1>
<Segmented
	label="Period"
	options={[
		{ value: '1', label: 'Today' },
		{ value: '7', label: '7 days' },
		{ value: '30', label: '30 days' }
	]}
	bind:value={days}
/>
{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}

{#if m}
	<section aria-labelledby="now">
		<h2 id="now">Right now</h2>
		<dl class="tiles">
			<div>
				<dt>Drivers online</dt>
				<dd>{m.drivers_online_now}</dd>
			</div>
			<div>
				<dt>Trips in progress</dt>
				<dd>{m.trips_in_progress_now}</dd>
			</div>
			<div>
				<dt>Open incidents</dt>
				<dd>{m.incidents_open_now}</dd>
			</div>
		</dl>
	</section>
	<section aria-labelledby="trips">
		<h2 id="trips">Trips</h2>
		<dl class="tiles">
			<div>
				<dt>Requested</dt>
				<dd>{m.requested}</dd>
			</div>
			<div>
				<dt>Completed</dt>
				<dd>{m.completed}</dd>
			</div>
			<div>
				<dt>Completion rate</dt>
				<dd>{pct(m.completion_rate)}</dd>
			</div>
			<div>
				<dt>Median time to a driver</dt>
				<dd>{num(m.median_match_min, 'min')}</dd>
			</div>
			<div>
				<dt>Cancelled by riders</dt>
				<dd>{m.cancelled_by_rider}</dd>
			</div>
			<div>
				<dt>Cancelled by drivers</dt>
				<dd>{m.cancelled_by_driver}</dd>
			</div>
			<div>
				<dt>No driver found</dt>
				<dd>{m.no_drivers}</dd>
			</div>
		</dl>
	</section>
	<section aria-labelledby="safety">
		<h2 id="safety">Safety</h2>
		<dl class="tiles">
			<div>
				<dt>SOS</dt>
				<dd>{m.sos}</dd>
			</div>
			<div>
				<dt>RideChecks asked</dt>
				<dd>{m.ridechecks_asked}</dd>
			</div>
			<div>
				<dt>RideChecks unanswered</dt>
				<dd>{m.ridechecks_unanswered}</dd>
			</div>
			<div>
				<dt>PIN lockouts</dt>
				<dd>{m.pin_lockouts}</dd>
			</div>
			<div>
				<dt>Safety reports</dt>
				<dd>{m.safety_reports}</dd>
			</div>
			<div>
				<dt>Other reports</dt>
				<dd>{m.other_reports}</dd>
			</div>
		</dl>
	</section>
	<section aria-labelledby="verification">
		<h2 id="verification">Verification</h2>
		<dl class="tiles">
			<div>
				<dt>Median ID check time</dt>
				<dd>{num(m.median_verification_h, 'h')}</dd>
			</div>
		</dl>
	</section>
{:else if !error}
	<p role="status">Loading…</p>
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	.tiles {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
		gap: var(--space-3);
		margin: 0;
	}
	.tiles div {
		padding: var(--space-3) var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	dt {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	dd {
		margin: 0;
		font-size: var(--text-2xl);
		font-weight: 800;
		font-variant-numeric: tabular-nums;
	}
</style>
