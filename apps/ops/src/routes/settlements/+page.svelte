<!--
	Weekly settlements (PRD P-6), admins only. Every Monday each driver gets a statement for the
	week before: negative when she owes VARA the service fees on her cash trips, positive when
	VARA owes her. Until a payout provider exists, record each one here once the money has moved,
	with the bank or deposit reference. Each settlement goes in the audit log.
-->
<script lang="ts">
	import { Button, Notice, Segmented, TextField, formatNad } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import { api, staff, type SettlementItem } from '#lib/staff.svelte.ts';

	let view = $state<'pending' | 'settled'>('pending');
	let items = $state<SettlementItem[] | null>(null);
	let settling = $state<string | null>(null);
	let reference = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function load(status: 'pending' | 'settled') {
		error = null;
		items = null;
		const { data, error: err } = await api
			.GET('/v1/ops/settlements', { params: { query: { status } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) items = data;
		else error = messageOf(err, 'The statements didn’t load.');
	}

	async function settle(item: SettlementItem) {
		error = notice = null;
		busy = true;
		const { data, error: err } = await api
			.POST('/v1/ops/settlements/{id}/settle', {
				params: { path: { id: item.id } },
				body: { reference: reference.trim() }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			notice = `Settled: ${item.driver_name ?? 'driver'}, ${formatNad(Math.abs(item.amount))}.`;
			settling = null;
			reference = '';
			await load(view);
		} else error = messageOf(err);
	}

	const week = (i: SettlementItem) => `${i.period_start} to ${i.period_end}`;

	$effect(() => {
		if (staff.canSettle) void load(view);
	});
</script>

<svelte:head><title>Settlements · VARA Ops</title></svelte:head>

{#if !staff.canSettle}
	<Notice tone="attention" title="Admins only">
		<p>Settlements move money. Ask an ops admin.</p>
	</Notice>
{:else}
	<h1>Weekly settlements</h1>
	<Segmented
		label="Show"
		options={[
			{ value: 'pending', label: 'To settle' },
			{ value: 'settled', label: 'Settled' }
		]}
		bind:value={view}
	/>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if notice}<Notice tone="success"><p>{notice}</p></Notice>{/if}

	{#if items && items.length === 0}
		<p class="empty">{view === 'pending' ? 'Nothing to settle.' : 'No settlements yet.'}</p>
	{:else if items}
		<ul class="list">
			{#each items as i (i.id)}
				<li>
					<div class="head">
						<strong>{i.driver_name ?? 'Name not given'}</strong>
						<span>{i.driver_phone ?? ''}</span>
						<span>Week {week(i)}</span>
					</div>
					<p class="amount">
						{i.amount < 0
							? `She pays VARA ${formatNad(-i.amount)}`
							: `VARA pays her ${formatNad(i.amount)}`}
					</p>
					{#if i.status !== 'pending'}
						<p class="muted">
							{i.status === 'received' ? 'Received' : 'Sent'}
							{i.settled_at ? formatWhen(i.settled_at) : ''} · {i.reference}
						</p>
					{:else if settling === i.id}
						<TextField label="Bank or deposit reference" bind:value={reference} maxlength={100} />
						<div class="actions">
							<Button onclick={() => settle(i)} loading={busy} disabled={!reference.trim()}>
								{i.amount < 0 ? 'Record payment received' : 'Record payout sent'}
							</Button>
							<Button variant="ghost" onclick={() => (settling = null)}>Cancel</Button>
						</div>
					{:else}
						<Button
							variant="secondary"
							onclick={() => {
								settling = i.id;
								reference = '';
							}}>Settle</Button
						>
					{/if}
				</li>
			{/each}
		</ul>
	{:else if !error}
		<p class="muted" role="status">Loading…</p>
	{/if}
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	.list {
		list-style: none;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}
	.list > li {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-3);
		align-items: baseline;
	}
	.amount {
		margin: 0;
		font-weight: 700;
		font-variant-numeric: tabular-nums;
	}
	.muted,
	.empty {
		color: var(--color-text-muted);
		margin: 0;
	}
	.actions {
		display: flex;
		gap: var(--space-2);
	}
</style>
