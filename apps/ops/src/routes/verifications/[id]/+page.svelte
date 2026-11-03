<!--
	One rider identity check (PRD A-2): the ID document next to the selfie, the details she
	entered, and earlier attempts. Approving lets her book rides.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { Notice } from '@fuber/ui';
	import Decide from '#lib/Decide.svelte';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import { api, type VerificationDetail } from '#lib/staff.svelte.ts';

	const id = $derived(page.params.id ?? '');
	let v = $state<VerificationDetail | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function load() {
		error = null;
		const { data, error: err } = await api
			.GET('/v1/ops/verifications/{id}', { params: { path: { id } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) v = data;
		else error = messageOf(err, 'This ID check didn’t load.');
	}

	$effect(() => {
		if (id) load();
	});

	async function decide(decision: 'approve' | 'reject', reason?: string) {
		error = notice = null;
		const { data, error: err } = await api
			.POST('/v1/ops/verifications/{id}/decision', {
				params: { path: { id } },
				body: { decision, reason: reason ?? null }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) {
			v = data;
			notice =
				decision === 'approve'
					? 'Verified. She can book rides now.'
					: 'Rejected. She sees your reason and can try again.';
		} else error = messageOf(err);
	}
</script>

<svelte:head><title>{v?.full_name ?? 'Rider'} · FUber Ops</title></svelte:head>

<p><a href={resolve('/')}>← Review queue</a></p>

{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
{#if notice}<Notice tone="success"><p>{notice}</p></Notice>{/if}

{#if v}
	<div class="head">
		<h1>{v.full_name ?? 'Name not given'}</h1>
		<span class="status">{v.status}</span>
	</div>
	<dl>
		<dt>Date of birth</dt>
		<dd>{v.date_of_birth ?? '—'}</dd>
		<dt>ID number</dt>
		<dd>{v.national_id ?? '—'}</dd>
	</dl>

	<div class="photos">
		<figure>
			{#if v.id_document_url}<img
					src={v.id_document_url}
					alt="ID document of {v.full_name}"
				/>{:else}<p class="muted">No ID photo</p>{/if}
			<figcaption>ID document</figcaption>
		</figure>
		<figure>
			{#if v.selfie_url}<img src={v.selfie_url} alt="Selfie of {v.full_name}" />{:else}<p
					class="muted"
				>
					No selfie
				</p>{/if}
			<figcaption>Selfie</figcaption>
		</figure>
	</div>
	<p class="muted small">
		Check the face matches, the name and date of birth match the ID, and the ID is a woman’s. Links
		last 5 minutes; every view is logged.
	</p>

	{#if v.previous.length}
		<h2>Earlier attempts</h2>
		<ul class="muted">
			{#each v.previous as p (p.created_at)}
				<li>
					{formatWhen(p.created_at)}: {p.status}{p.reject_reason ? ` (${p.reject_reason})` : ''}
				</li>
			{/each}
		</ul>
	{/if}

	{#if v.status === 'pending'}
		<div class="final">
			<h2>Decision</h2>
			<Decide subject="this rider" approveLabel="Verify rider" ondecide={decide} />
		</div>
	{/if}
{:else if !error}
	<p class="muted">Loading…</p>
{/if}

<style>
	h1 {
		margin: 0;
		font-size: var(--text-2xl);
	}
	h2 {
		font-size: var(--text-lg);
		margin: var(--space-5) 0 var(--space-3);
	}
	p {
		margin: 0;
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		margin: var(--space-3) 0;
	}
	.status {
		font-size: var(--text-sm);
		font-weight: 700;
		padding: 2px var(--space-3);
		border-radius: var(--radius-pill);
		color: var(--color-attention);
		background: var(--color-attention-fill);
		text-transform: capitalize;
	}
	dl {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		gap: var(--space-1) var(--space-4);
		margin: 0 0 var(--space-4);
	}
	dt {
		color: var(--color-text-muted);
	}
	dd {
		margin: 0;
		font-weight: 600;
	}
	.photos {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(100%, 320px), 1fr));
		gap: var(--space-3);
	}
	figure {
		margin: 0;
		display: grid;
		gap: var(--space-2);
		padding: var(--space-3);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-sm);
	}
	img {
		width: 100%;
		max-height: 380px;
		object-fit: contain;
		background: var(--color-bg);
		border-radius: var(--radius-sm);
	}
	figcaption,
	.muted {
		color: var(--color-text-muted);
	}
	.small {
		font-size: var(--text-sm);
		margin-top: var(--space-2);
	}
	.final {
		display: grid;
		gap: var(--space-3);
	}
</style>
