<!--
	Review queues (PRD O-1): driver applications and rider identity checks waiting for a
	decision, oldest first.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Notice } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import {
		api,
		staff,
		type DriverQueueItem,
		type VerificationQueueItem
	} from '#lib/staff.svelte.ts';

	let drivers = $state<DriverQueueItem[] | null>(null);
	let riders = $state<VerificationQueueItem[] | null>(null);
	let error = $state<string | null>(null);

	async function load() {
		error = null;
		const [d, r] = await Promise.all([
			api.GET('/v1/ops/drivers').catch(() => ({ data: undefined, error: undefined })),
			api.GET('/v1/ops/verifications').catch(() => ({ data: undefined, error: undefined }))
		]);
		drivers = d.data ?? null;
		riders = r.data ?? null;
		if (!d.data || !r.data) error = messageOf(d.error ?? r.error, 'The queues didn’t load.');
	}

	$effect(() => {
		if (staff.canReview) load();
	});
</script>

<svelte:head><title>Review queue · FUber Ops</title></svelte:head>

{#if !staff.canReview}
	<Notice tone="attention" title="No review access">
		<p>Your role can’t review applications. Ask an ops admin for the reviewer role.</p>
	</Notice>
{:else}
	<div class="top">
		<h1>Waiting for review</h1>
		<button type="button" class="link" onclick={load}>Refresh</button>
	</div>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}

	<section aria-labelledby="drivers">
		<h2 id="drivers">Driver applications <span class="count">{drivers?.length ?? '…'}</span></h2>
		{#if drivers && drivers.length === 0}
			<p class="empty">No driver applications waiting. Submitted applications appear here.</p>
		{:else if drivers}
			<ul class="list">
				{#each drivers as d (d.user_id)}
					<li>
						<a href={resolve('/drivers/[id]', { id: d.user_id })}>
							<strong>{d.full_name ?? 'Name not given'}</strong>
							<span>Submitted {formatWhen(d.submitted_at)}</span>
							<span>{d.requested_tiers.join(', ') || 'No ride types'}</span>
							<span class="pill">{d.documents_in_review} documents to check</span>
						</a>
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<section aria-labelledby="riders">
		<h2 id="riders">Rider ID checks <span class="count">{riders?.length ?? '…'}</span></h2>
		{#if riders && riders.length === 0}
			<p class="empty">
				No riders waiting. A rider appears here once her ID and selfie pass the upload checks.
			</p>
		{:else if riders}
			<ul class="list">
				{#each riders as v (v.id)}
					<li>
						<a href={resolve('/verifications/[id]', { id: v.id })}>
							<strong>{v.display_name ?? 'Name not given'}</strong>
							<span>Submitted {formatWhen(v.submitted_at)}</span>
							<span
								>{v.previous_attempts > 0
									? `${v.previous_attempts} earlier attempt${v.previous_attempts > 1 ? 's' : ''}`
									: 'First attempt'}</span
							>
						</a>
					</li>
				{/each}
			</ul>
		{/if}
	</section>
{/if}

<style>
	.top {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: var(--space-3);
	}
	h1 {
		margin: 0 0 var(--space-4);
		font-size: var(--text-2xl);
	}
	h2 {
		font-size: var(--text-lg);
		margin: var(--space-5) 0 var(--space-3);
	}
	.count {
		display: inline-block;
		min-width: 1.6em;
		padding: 0 var(--space-2);
		border-radius: var(--radius-pill);
		background: var(--color-surface-tinted);
		color: var(--color-brand);
		font-size: var(--text-sm);
		text-align: center;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-2);
	}
	.list a {
		display: grid;
		grid-template-columns: minmax(0, 1.4fr) minmax(0, 1.2fr) minmax(0, 1fr) auto;
		align-items: center;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-sm);
		color: inherit;
		text-decoration: none;
	}
	.list a:hover,
	.list a:focus-visible {
		border-color: var(--color-brand);
	}
	.list span {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.list .pill {
		color: var(--color-attention);
		background: var(--color-attention-fill);
		padding: 2px var(--space-2);
		border-radius: var(--radius-pill);
	}
	.empty {
		color: var(--color-text-muted);
	}
	.link {
		background: none;
		border: 0;
		font: inherit;
		color: var(--color-brand);
		cursor: pointer;
	}
	@media (max-width: 640px) {
		.list a {
			grid-template-columns: minmax(0, 1fr);
			gap: var(--space-1);
		}
	}
</style>
