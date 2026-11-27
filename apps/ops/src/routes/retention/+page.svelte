<!--
	Incident retention review (schema §12), admins only: safety incidents closed over 5 years ago
	that still hold their ride's data. Keep one another year when it is still needed (an open
	police case or legal claim; a reason is required), or release it: the trip's GPS track, chat
	and addresses are then deleted by the nightly clean-up, and the incident's notes, outcome and
	report text are removed. Both are audited.
-->
<script lang="ts">
	import { Button, Notice, TextField } from '@fuber/ui';
	import { INCIDENT_KINDS, formatWhen, messageOf } from '#lib/format.ts';
	import { api, staff, type RetentionItem } from '#lib/staff.svelte.ts';

	let items = $state<RetentionItem[]>([]);
	let loaded = $state(false);
	let error = $state<string | null>(null);
	/** The incident whose "keep" reason is being written, or whose release is being confirmed. */
	let keeping = $state<string | null>(null);
	let releasing = $state<string | null>(null);
	let reason = $state('');
	let busy = $state<string | null>(null);

	async function load() {
		const { data, error: err } = await api
			.GET('/v1/ops/incidents/retention')
			.catch(() => ({ data: undefined, error: undefined }));
		loaded = true;
		if (!data) {
			error = messageOf(err, 'The review list didn’t load.');
			return;
		}
		error = null;
		items = data;
	}

	async function decide(item: RetentionItem, decision: 'keep' | 'release') {
		busy = item.id;
		const { error: err, response } = await api
			.POST('/v1/ops/incidents/{id}/retention', {
				params: { path: { id: item.id } },
				body: { decision, reason: decision === 'keep' ? reason.trim() : null }
			})
			.catch(() => ({ error: undefined, response: undefined }));
		busy = null;
		if (!response?.ok) {
			error = messageOf(err, 'That didn’t save. Try again.');
			return;
		}
		keeping = null;
		releasing = null;
		reason = '';
		items = items.filter((i) => i.id !== item.id);
	}

	$effect(() => {
		if (staff.canSettle) void load();
	});
</script>

<svelte:head><title>Retention review · VARA Ops</title></svelte:head>

{#if !staff.canSettle}
	<Notice tone="attention" title="Admins only">
		<p>The retention review is for ops admins.</p>
	</Notice>
{:else}
	<h1>Retention review</h1>
	<p class="lead">
		Safety incidents are kept for 5 years after they close. Keep one for another year only when it
		is still needed, such as an open police case or legal claim. Releasing it deletes the trip’s GPS
		track, chat and addresses and removes the incident’s notes and report text. This can’t be
		undone.
	</p>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if loaded && items.length === 0 && !error}
		<p class="muted">Nothing is due for review.</p>
	{/if}
	<ul class="list">
		{#each items as i (i.id)}
			<li class="card">
				<p class="head">
					<strong>{INCIDENT_KINDS[i.kind] ?? i.kind}</strong>
					{#if i.ride_code}<span>Trip {i.ride_code}</span>{/if}
					{#if i.escalated_to_police}<span class="police">Reported to the police</span>{/if}
				</p>
				<p class="muted">
					Raised {formatWhen(i.created_at)} · closed {formatWhen(i.closed_at)}
					{#if i.kept_until}· kept after a review until {formatWhen(i.kept_until)}{/if}
				</p>
				{#if i.outcome}<p>Outcome: {i.outcome}</p>{/if}
				{#if keeping === i.id}
					<TextField
						label="Why it is still needed"
						placeholder="Police case still open."
						maxlength={300}
						bind:value={reason}
					/>
					<div class="row">
						<Button
							disabled={!reason.trim()}
							loading={busy === i.id}
							onclick={() => decide(i, 'keep')}>Keep for another year</Button
						>
						<Button variant="ghost" onclick={() => (keeping = null)}>Back</Button>
					</div>
				{:else if releasing === i.id}
					<p role="alert">
						Release it for good? The trip’s track, chat and addresses and this incident’s notes will
						be deleted.
					</p>
					<div class="row">
						<Button variant="secondary" loading={busy === i.id} onclick={() => decide(i, 'release')}
							>Release for good</Button
						>
						<Button variant="ghost" onclick={() => (releasing = null)}>Back</Button>
					</div>
				{:else}
					<div class="row">
						<Button variant="secondary" disabled={busy !== null} onclick={() => (keeping = i.id)}
							>Keep</Button
						>
						<Button
							variant="ghost"
							disabled={busy !== null}
							onclick={() => (releasing = i.id)}
							aria-label="Release this {INCIDENT_KINDS[i.kind] ?? i.kind} incident">Release</Button
						>
					</div>
				{/if}
			</li>
		{/each}
	</ul>
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	.lead {
		max-width: 65ch;
	}
	.list {
		list-style: none;
		padding: 0;
		display: grid;
		gap: var(--space-3);
	}
	.card {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		padding: var(--space-4);
	}
	.head {
		display: flex;
		gap: var(--space-3);
		flex-wrap: wrap;
		margin-top: 0;
	}
	.police {
		font-weight: 600;
	}
	.row {
		display: flex;
		gap: var(--space-2);
		flex-wrap: wrap;
	}
	.muted {
		color: var(--color-text-muted);
	}
</style>
