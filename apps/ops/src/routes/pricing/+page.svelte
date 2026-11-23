<!--
	Pricing and surge (PRD O-5), admins only. New rates are added as a new version from a start
	time (now or later); rides already quoted keep the rates they were locked with. The service
	fee stays 15–18 %. The launch promotion: 13 % for everyone, Map Experts included, for 90 days
	from the launch date an admin sets, then the tier fees again by themselves. Surge zones:
	change the multiplier and demand level of an existing zone. Every change is audited.
-->
<script lang="ts">
	import { Button, Notice, formatNad } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import {
		api,
		staff,
		type LaunchPromo,
		type SurgeZone,
		type TierRates
	} from '#lib/staff.svelte.ts';
	import TableScroll from '#lib/TableScroll.svelte';

	type Tier = 'classic' | 'comfort' | 'softlife';
	const NAMES: Record<string, string> = {
		classic: 'Classic',
		comfort: 'Comfort',
		softlife: 'Soft Life'
	};

	let rates = $state<TierRates[] | null>(null);
	let zones = $state<SurgeZone[]>([]);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busy = $state(false);

	/** The form, in N$ and %, for one ride type. */
	let editing = $state<Tier | null>(null);
	let form = $state({ base: 0, km: 0, min: 0, minimum: 0, fee: 15, from: '' });
	let zoneDrafts = $state<Record<string, { multiplier: number; demand_level: string }>>({});
	let promo = $state<LaunchPromo | null>(null);
	let launchAt = $state('');

	async function load() {
		error = null;
		const [r, z, p] = await Promise.all([
			api.GET('/v1/ops/pricing').catch(() => ({ data: undefined, error: undefined })),
			api.GET('/v1/ops/surge-zones').catch(() => ({ data: undefined, error: undefined })),
			api.GET('/v1/ops/pricing/launch-promo').catch(() => ({ data: undefined, error: undefined }))
		]);
		if (r.data) rates = r.data;
		if (p.data) promo = p.data;
		if (z.data) {
			zones = z.data;
			zoneDrafts = Object.fromEntries(
				z.data.map((x) => [x.id, { multiplier: x.multiplier, demand_level: x.demand_level }])
			);
		}
		if (!r.data || !z.data) error = messageOf(r.error ?? z.error, 'Pricing didn’t load.');
	}

	function edit(r: TierRates) {
		editing = r.tier as Tier;
		form = {
			base: r.base / 100,
			km: r.per_km / 100,
			min: r.per_min / 100,
			minimum: r.minimum / 100,
			fee: r.service_fee_bp / 100,
			from: ''
		};
	}

	const cents = (nad: number) => Math.round(Number(nad) * 100);

	async function save() {
		if (!editing) return;
		error = notice = null;
		busy = true;
		const { data, error: err } = await api
			.POST('/v1/ops/pricing', {
				body: {
					tier: editing,
					base_cents: cents(form.base),
					per_km_cents: cents(form.km),
					per_min_cents: cents(form.min),
					minimum_cents: cents(form.minimum),
					service_fee_bp: Math.round(Number(form.fee) * 100),
					effective_from: form.from ? new Date(form.from).toISOString() : null
				}
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			notice = data.current
				? `${NAMES[data.tier]} rates are in force now.`
				: `${NAMES[data.tier]} rates start ${formatWhen(data.effective_from)}.`;
			editing = null;
			await load();
		} else error = messageOf(err);
	}

	/** Sets, moves or (with `null`) removes the launch date; fixed once the promotion has started. */
	async function savePromo(startsAt: string | null) {
		error = notice = null;
		busy = true;
		const { data, error: err } = await api
			.PUT('/v1/ops/pricing/launch-promo', { body: { starts_at: startsAt } })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			promo = data;
			launchAt = '';
			notice = data.starts_at
				? `The launch promotion runs ${formatWhen(data.starts_at)} – ${formatWhen(data.ends_at ?? '')}.`
				: 'The launch date is removed.';
		} else error = messageOf(err);
	}

	const promoState = $derived(
		!promo?.starts_at
			? 'unset'
			: promo.active
				? 'running'
				: new Date(promo.starts_at) > new Date()
					? 'scheduled'
					: 'ended'
	);

	async function saveZone(z: SurgeZone) {
		error = notice = null;
		busy = true;
		const d = zoneDrafts[z.id];
		const { data, error: err } = await api
			.PUT('/v1/ops/surge-zones/{id}', {
				params: { path: { id: z.id } },
				body: { multiplier: Number(d.multiplier), demand_level: d.demand_level }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			notice = `${data.name}: ×${data.multiplier.toFixed(2)}, ${data.demand_level}.`;
			await load();
		} else error = messageOf(err);
	}

	$effect(() => {
		if (staff.canSettle) void load();
	});
</script>

<svelte:head><title>Pricing · VARA Ops</title></svelte:head>

{#if !staff.canSettle}
	<Notice tone="attention" title="Admins only">
		<p>Prices are set by ops admins.</p>
	</Notice>
{:else}
	<h1>Pricing</h1>
	{#if promo}
		<section class="box" aria-labelledby="promo">
			<h2 id="promo">Launch promotion: 13 % for {promo.days} days</h2>
			{#if promoState === 'running'}
				<p>
					Running until {formatWhen(promo.ends_at ?? '')}. Every driver, Map Experts included, pays
					13 % on the rides she accepts.
				</p>
			{:else if promoState === 'ended'}
				<p>Ended {formatWhen(promo.ends_at ?? '')}. The tier fees and the Map Expert fee apply.</p>
			{:else}
				<p>
					{promoState === 'scheduled'
						? `Starts ${formatWhen(promo.starts_at ?? '')}, ends ${formatWhen(promo.ends_at ?? '')}.`
						: 'Not set. Enter the official launch date to start it.'}
					Afterwards 15–18 % (15 % for Map Experts) applies again by itself. Once it has started, the
					dates can’t change.
				</p>
				<div class="actions">
					<label>Launch date and time <input type="datetime-local" bind:value={launchAt} /></label>
					<Button
						onclick={() => savePromo(new Date(launchAt).toISOString())}
						loading={busy}
						disabled={!launchAt}>{promoState === 'scheduled' ? 'Move launch' : 'Set launch'}</Button
					>
					{#if promoState === 'scheduled'}
						<Button variant="ghost" onclick={() => savePromo(null)} disabled={busy}
							>Remove launch date</Button
						>
					{/if}
				</div>
			{/if}
		</section>
	{/if}
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if notice}<Notice tone="success"><p>{notice}</p></Notice>{/if}

	{#if rates}
		<TableScroll label="Rates per ride type">
			<table>
				<thead>
					<tr>
						<th>Ride type</th><th>Base</th><th>Per km</th><th>Per min</th><th>Minimum</th>
						<th>Service fee</th><th>From</th><th></th>
					</tr>
				</thead>
				<tbody>
					{#each rates as r (r.id)}
						<tr class:later={!r.current}>
							<td>{NAMES[r.tier] ?? r.tier}{r.current ? '' : ' (scheduled)'}</td>
							<td>{formatNad(r.base)}</td>
							<td>{formatNad(r.per_km)}</td>
							<td>{formatNad(r.per_min)}</td>
							<td>{formatNad(r.minimum)}</td>
							<td>{(r.service_fee_bp / 100).toFixed(1)} %</td>
							<td>{formatWhen(r.effective_from)}</td>
							<td>
								{#if r.current}<Button variant="ghost" onclick={() => edit(r)}>New rates…</Button
									>{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</TableScroll>
	{/if}

	{#if editing}
		<section class="box" aria-labelledby="new-rates">
			<h2 id="new-rates">New {NAMES[editing]} rates</h2>
			<div class="grid">
				<label>Base (N$) <input type="number" min="0" step="0.5" bind:value={form.base} /></label>
				<label>Per km (N$) <input type="number" min="0" step="0.1" bind:value={form.km} /></label>
				<label
					>Per minute (N$) <input type="number" min="0" step="0.1" bind:value={form.min} /></label
				>
				<label
					>Minimum fare (N$) <input
						type="number"
						min="0"
						step="0.5"
						bind:value={form.minimum}
					/></label
				>
				<label
					>Service fee (%) <input
						type="number"
						min="15"
						max="18"
						step="0.5"
						bind:value={form.fee}
					/></label
				>
				<label>Start (empty = now) <input type="datetime-local" bind:value={form.from} /></label>
			</div>
			<p class="muted">Quotes already locked keep their old price.</p>
			<div class="actions">
				<Button onclick={save} loading={busy}>Add rates</Button>
				<Button variant="ghost" onclick={() => (editing = null)}>Cancel</Button>
			</div>
		</section>
	{/if}

	<h2>Surge zones</h2>
	{#if zones.length === 0}
		<p class="muted">No surge zones yet. Drawing zones needs the map (later).</p>
	{:else}
		<TableScroll label="Surge zones">
			<table>
				<thead
					><tr><th>Zone</th><th>Multiplier</th><th>Demand</th><th>Changed</th><th></th></tr></thead
				>
				<tbody>
					{#each zones as z (z.id)}
						<tr>
							<td>{z.name}</td>
							<td>
								<input
									aria-label="Multiplier for {z.name}"
									type="number"
									min="1"
									max="3"
									step="0.05"
									bind:value={zoneDrafts[z.id].multiplier}
								/>
							</td>
							<td>
								<select aria-label="Demand for {z.name}" bind:value={zoneDrafts[z.id].demand_level}>
									<option value="normal">Normal</option>
									<option value="high">High</option>
									<option value="peak">Peak</option>
								</select>
							</td>
							<td>{formatWhen(z.updated_at)}</td>
							<td
								><Button variant="secondary" loading={busy} onclick={() => saveZone(z)}>Save</Button
								></td
							>
						</tr>
					{/each}
				</tbody>
			</table>
		</TableScroll>
	{/if}
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	table {
		width: 100%;
		border-collapse: collapse;
		background: var(--color-surface);
		margin-bottom: var(--space-4);
	}
	th,
	td {
		text-align: left;
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px solid var(--color-border);
		font-variant-numeric: tabular-nums;
	}
	.later {
		color: var(--color-text-muted);
	}
	.box {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		padding: var(--space-4);
		margin-bottom: var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.box h2 {
		margin: 0;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
		gap: var(--space-3);
	}
	label {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}
	input,
	select {
		font: inherit;
		padding: var(--space-1) var(--space-2);
	}
	td input {
		width: 6rem;
	}
	.actions {
		display: flex;
		gap: var(--space-2);
	}
	.muted {
		color: var(--color-text-muted);
		margin: 0;
	}
</style>
