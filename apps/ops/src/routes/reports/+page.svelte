<!--
	Reported issues (PRD S-5, O-6) for support: lost items, fares and everything else, oldest
	first. Safety reports are here too, marked, because the safety desk also has them as
	incidents. Each change of status goes in the audit log.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Button, Notice, Segmented, TextField } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import { api, staff, type SupportReport } from '#lib/staff.svelte.ts';

	type Status = 'open' | 'in_progress' | 'resolved';
	const CATEGORIES: Record<string, string> = {
		harassment: 'Harassment or threats',
		unsafe_driving: 'Unsafe driving or behaviour',
		wrong_person: 'Wrong person',
		wrong_vehicle: 'Wrong car or plate',
		lost_item: 'Lost item',
		fare: 'Fare or payment',
		other: 'Something else'
	};

	let view = $state<Status>('open');
	let reports = $state<SupportReport[] | null>(null);
	let notes = $state<Record<string, string>>({});
	let busy = $state(false);
	let error = $state<string | null>(null);

	async function load(status: Status) {
		error = null;
		reports = null;
		const { data, error: err } = await api
			.GET('/v1/ops/reports', { params: { query: { status } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) {
			// Every note box needs a string: binding `undefined` to TextField's value throws.
			notes = Object.fromEntries(data.map((r) => [r.id, notes[r.id] ?? '']));
			reports = data;
		} else error = messageOf(err, 'The reports didn’t load.');
	}

	async function move(r: SupportReport, status: 'in_progress' | 'resolved') {
		error = null;
		busy = true;
		const note = notes[r.id]?.trim();
		const { response, error: err } = await api
			.POST('/v1/ops/reports/{id}/status', {
				params: { path: { id: r.id } },
				body: { status, note: note || null }
			})
			.catch(() => ({ response: undefined, error: undefined }));
		busy = false;
		if (response?.ok) await load(view);
		else error = messageOf(err);
	}

	$effect(() => {
		if (staff.canSupport) void load(view);
	});
</script>

<svelte:head><title>Reports · VARA Ops</title></svelte:head>

{#if !staff.canSupport}
	<Notice tone="attention" title="No support access">
		<p>Your role can’t handle reports. Ask an ops admin for the support role.</p>
	</Notice>
{:else}
	<h1>Reported issues</h1>
	<Segmented
		label="Show"
		options={[
			{ value: 'open', label: 'New' },
			{ value: 'in_progress', label: 'In progress' },
			{ value: 'resolved', label: 'Resolved' }
		]}
		bind:value={view}
	/>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if reports && reports.length === 0}
		<p class="empty">Nothing here.</p>
	{:else if reports}
		<ul class="list">
			{#each reports as r (r.id)}
				<li>
					<div class="head">
						<strong>{CATEGORIES[r.category] ?? r.category}</strong>
						{#if r.incident_id}<span class="safety">Safety desk has it</span>{/if}
						<span>{r.ride_code ?? 'No trip'}</span>
						<span>{formatWhen(r.created_at)}</span>
					</div>
					<p>
						From
						<a href={resolve('/users/[id]', { id: r.reporter_id })}
							>{r.reporter_name ?? 'Name not given'}</a
						>,
						{r.reporter_phone ?? '—'}
					</p>
					<p class="text">{r.text}</p>
					{#if r.status !== 'resolved'}
						<TextField label="Note (what was done)" bind:value={notes[r.id]} maxlength={500} />
						<div class="actions">
							{#if r.status === 'open'}
								<Button variant="secondary" loading={busy} onclick={() => move(r, 'in_progress')}
									>Start working on it</Button
								>
							{/if}
							<Button loading={busy} onclick={() => move(r, 'resolved')}>Resolved</Button>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{:else if !error}
		<p class="empty" role="status">Loading…</p>
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
	.list p {
		margin: 0;
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-3);
	}
	.safety {
		color: var(--color-sos-text);
		font-weight: 700;
	}
	.text {
		white-space: pre-wrap;
	}
	.actions {
		display: flex;
		gap: var(--space-2);
	}
	.empty {
		color: var(--color-text-muted);
	}
</style>
