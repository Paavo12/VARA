<!--
	One driver application (PRD A-4 – A-6): each document, the car, the ride types she may drive,
	then the decision. Approving the application needs every required document, the car and at
	least one ride type approved first; the API checks the same.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { Button, Notice, Plate } from '@fuber/ui';
	import Decide from '#lib/Decide.svelte';
	import { documentName, messageOf } from '#lib/format.ts';
	import { api, type DriverReview, type ReviewDocument, type Tier } from '#lib/staff.svelte.ts';

	const TIERS: { id: Tier; name: string }[] = [
		{ id: 'classic', name: 'Classic' },
		{ id: 'comfort', name: 'Comfort' },
		{ id: 'softlife', name: 'Soft Life' }
	];

	const id = $derived(page.params.id ?? '');
	let review = $state<DriverReview | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let tiers = $state<Tier[]>([]);
	let savingTiers = $state(false);

	async function load() {
		error = null;
		const { data, error: err } = await api
			.GET('/v1/ops/drivers/{id}', { params: { path: { id } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) adopt(data);
		else error = messageOf(err, 'This application didn’t load.');
	}

	function adopt(r: DriverReview) {
		review = r;
		tiers = (r.approved_tiers.length ? r.approved_tiers : r.requested_tiers) as Tier[];
	}

	$effect(() => {
		if (id) load();
	});

	async function decideDocument(
		doc: ReviewDocument,
		decision: 'approve' | 'reject',
		reason?: string
	) {
		error = notice = null;
		const { data, error: err } = await api
			.POST('/v1/ops/documents/{id}/decision', {
				params: { path: { id: doc.id } },
				body: { decision, reason: reason ?? null }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		if (!data) error = messageOf(err);
		await load();
	}

	async function decideVehicle(vehicleId: string, decision: 'approve' | 'reject', reason?: string) {
		error = notice = null;
		const { data, error: err } = await api
			.POST('/v1/ops/vehicles/{id}/decision', {
				params: { path: { id: vehicleId } },
				body: { decision, reason: reason ?? null }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		if (!data) error = messageOf(err);
		await load();
	}

	async function saveTiers() {
		error = notice = null;
		savingTiers = true;
		const { data, error: err } = await api
			.PUT('/v1/ops/drivers/{id}/tiers', { params: { path: { id } }, body: { tiers } })
			.catch(() => ({ data: undefined, error: undefined }));
		savingTiers = false;
		if (data) {
			adopt(data);
			notice = 'Ride types saved.';
		} else error = messageOf(err);
	}

	async function decideApplication(decision: 'approve' | 'reject', reason?: string) {
		error = notice = null;
		const { data, error: err } = await api
			.POST('/v1/ops/drivers/{id}/decision', {
				params: { path: { id } },
				body: { decision, reason: reason ?? null }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) {
			adopt(data);
			notice =
				decision === 'approve'
					? 'Application approved. She can go online now.'
					: 'Application rejected. She sees your reason in the app.';
		} else error = messageOf(err);
	}

	function statusTone(status: string): string {
		return status === 'approved'
			? 'ok'
			: status === 'rejected' || status === 'expired'
				? 'bad'
				: 'wait';
	}

	const pending = $derived(review?.application_status === 'pending');
</script>

<svelte:head><title>{review?.full_name ?? 'Driver'} · VARA Ops</title></svelte:head>

<p><a href={resolve('/')}>← Review queue</a></p>

{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
{#if notice}<Notice tone="success"><p>{notice}</p></Notice>{/if}

{#if review}
	<div class="head">
		<div>
			<h1>{review.full_name ?? 'Name not given'}</h1>
			<p class="muted">
				Born {review.date_of_birth ?? '—'} · Asked for {review.requested_tiers.join(', ') ||
					'no ride types'}
			</p>
		</div>
		<span class="status {statusTone(review.application_status)}"
			>{review.application_status.replace('_', ' ')}</span
		>
	</div>
	{#if review.decision_reason}<p class="muted">Last decision: {review.decision_reason}</p>{/if}

	<section>
		<h2>Documents</h2>
		{#if review.documents.length === 0}
			<p class="muted">No documents yet. They appear once she submits her application.</p>
		{/if}
		<div class="docs">
			{#each review.documents as doc (doc.id)}
				<article class="doc">
					<div class="doc-head">
						<h3>{documentName(doc.kind)}</h3>
						<span class="status {statusTone(doc.status)}">{doc.status.replace('_', ' ')}</span>
					</div>
					<!-- Signed links to the API's file storage, not pages of this app. -->
					{#if doc.url && doc.mime_type?.startsWith('image/')}
						<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
						<a href={doc.url} target="_blank" rel="noreferrer noopener">
							<img src={doc.url} alt="{documentName(doc.kind)} of {review.full_name}" />
						</a>
					{:else if doc.url}
						<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
						<a href={doc.url} target="_blank" rel="noreferrer noopener">Open the file (PDF)</a>
					{:else}
						<p class="muted">Still being checked for viruses and file type.</p>
					{/if}
					<p class="muted small">
						{doc.expires_on ? `Expires ${doc.expires_on}` : 'No expiry date'}{doc.reject_reason
							? ` · Rejected: ${doc.reject_reason}`
							: ''}
					</p>
					{#if doc.status === 'in_review'}
						<Decide
							subject="the {documentName(doc.kind).toLowerCase()}"
							ondecide={(d, r) => decideDocument(doc, d, r)}
						/>
					{/if}
				</article>
			{/each}
		</div>
		<p class="muted small">
			Links to the files last 5 minutes; reload the page for new ones. Every view is logged.
		</p>
	</section>

	<section>
		<h2>Car</h2>
		{#if review.vehicle}
			{@const v = review.vehicle}
			<div class="car">
				<div>
					<p>
						<strong>{v.colour} {v.make} {v.model}</strong> · {v.year} · {v.seats} seats{v.wheelchair_accessible
							? ' · wheelchair accessible'
							: ''}
					</p>
					<Plate plate={v.plate} />
					{#if v.reject_reason}<p class="muted small">Rejected: {v.reject_reason}</p>{/if}
				</div>
				<div class="car-side">
					<span class="status {v.approved ? 'ok' : 'wait'}"
						>{v.approved ? 'approved' : 'not approved'}</span
					>
					{#if !v.approved}
						<Decide subject="the car" ondecide={(d, r) => decideVehicle(v.id, d, r)} />
					{/if}
				</div>
			</div>
		{:else}
			<p class="muted">No car added yet.</p>
		{/if}
	</section>

	<section>
		<h2>Ride types she may drive</h2>
		<fieldset class="tiers">
			<legend class="muted small">Approved: {review.approved_tiers.join(', ') || 'none yet'}</legend
			>
			{#each TIERS as t (t.id)}
				<label>
					<input type="checkbox" value={t.id} bind:group={tiers} />
					{t.name}{review.requested_tiers.includes(t.id) ? '' : ' (not asked for)'}
				</label>
			{/each}
			<Button variant="secondary" loading={savingTiers} onclick={saveTiers}>Save ride types</Button>
		</fieldset>
	</section>

	<section class="final">
		<h2>Decision</h2>
		{#if pending}
			<p class="muted">
				Approve once every document, the car and at least one ride type are approved.
			</p>
			<Decide
				subject="the application"
				approveLabel="Approve application"
				ondecide={decideApplication}
			/>
		{:else}
			<p class="muted">
				This application is {review.application_status.replace('_', ' ')}{review.can_go_online
					? ' and she can go online.'
					: '; she can’t go online.'}
			</p>
		{/if}
	</section>
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
	h3 {
		margin: 0;
		font-size: var(--text-md);
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-start;
		justify-content: space-between;
		gap: var(--space-3);
		margin: var(--space-3) 0;
	}
	.muted {
		color: var(--color-text-muted);
	}
	.small {
		font-size: var(--text-sm);
	}
	p {
		margin: 0;
	}
	.status {
		font-size: var(--text-sm);
		font-weight: 700;
		padding: 2px var(--space-3);
		border-radius: var(--radius-pill);
		text-transform: capitalize;
		white-space: nowrap;
	}
	.status.ok {
		color: var(--color-success);
		background: var(--color-success-fill);
	}
	.status.wait {
		color: var(--color-attention);
		background: var(--color-attention-fill);
	}
	.status.bad {
		color: var(--color-text);
		background: var(--color-surface-tinted);
	}
	.docs {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(100%, 300px), 1fr));
		gap: var(--space-3);
	}
	.doc,
	.car,
	.tiers {
		display: grid;
		gap: var(--space-2);
		padding: var(--space-4);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-sm);
		min-width: 0;
	}
	.doc-head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: var(--space-2);
	}
	.doc img {
		width: 100%;
		max-height: 260px;
		object-fit: contain;
		background: var(--color-bg);
		border-radius: var(--radius-sm);
	}
	.car {
		grid-template-columns: minmax(0, 1fr) auto;
		align-items: start;
	}
	.car-side {
		display: grid;
		gap: var(--space-2);
		justify-items: end;
	}
	.tiers {
		margin: 0;
		justify-items: start;
	}
	.tiers label {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--touch-target);
	}
	.final {
		display: grid;
		gap: var(--space-3);
	}
	@media (max-width: 640px) {
		.car {
			grid-template-columns: minmax(0, 1fr);
		}
		.car-side {
			justify-items: start;
		}
	}
</style>
