<!--
	Driver documents (PRD A-4, A-5): one card per required document with its status, reason if
	rejected, and expiry date. Uploading a new file replaces the current one.
-->
<script lang="ts">
	import type { DocumentKind, DriverApplication } from '@fuber/api-client';
	import { PhotoUpload, messageOf, useSession } from '@fuber/account';
	import { Button, Notice, Screen, TextField } from '@fuber/ui';

	interface Props {
		application: DriverApplication;
		step?: string;
		onchanged: () => void;
		onback?: () => void;
	}

	let { application, step, onchanged, onback }: Props = $props();
	const session = useSession();

	const kinds: {
		kind: DocumentKind;
		name: string;
		guidance: string;
		expires: boolean;
		capture?: 'user';
	}[] = [
		{
			kind: 'identity',
			name: 'ID card or passport',
			guidance: 'Photo page flat, all corners visible.',
			expires: false
		},
		{
			kind: 'selfie',
			name: 'Selfie',
			guidance: 'Face the camera in good light.',
			expires: false,
			capture: 'user'
		},
		{
			kind: 'licence',
			name: 'Driver’s licence',
			guidance: 'Front of the card, all corners visible.',
			expires: true
		},
		{
			kind: 'insurance',
			name: 'Insurance',
			guidance: 'The certificate page showing the car and dates.',
			expires: true
		},
		{
			kind: 'police_clearance',
			name: 'Police clearance',
			guidance: 'Certificate from NamPol, under 6 months old.',
			expires: true
		}
	];
	const statusText: Record<string, string> = {
		uploading: 'Checking file',
		in_review: 'In review',
		approved: 'Approved',
		rejected: 'Rejected',
		expired: 'Expired'
	};

	let open = $state<DocumentKind | null>(null);
	let uploadId = $state<string | null>(null);
	let expiresOn = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);
	const locked = $derived(application.status === 'pending');
	const tomorrow = new Date(Date.now() + 86_400_000).toISOString().slice(0, 10);

	function current(kind: DocumentKind) {
		return application.documents.find((d) => d.kind === kind);
	}

	function start(kind: DocumentKind) {
		open = kind;
		uploadId = null;
		expiresOn = '';
		error = null;
	}

	async function attach(needsExpiry: boolean) {
		if (!open || !uploadId) return;
		if (needsExpiry && !expiresOn)
			return void (error = 'Enter the expiry date printed on the document.');
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST('/v1/driver/documents', {
				body: { kind: open, upload_id: uploadId, expires_on: needsExpiry ? expiresOn : null }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data) return void (error = messageOf(err));
		open = null;
		onchanged();
	}
</script>

<Screen
	title="Your documents"
	lead="Photos or PDFs. Only VARA’s verification team sees them."
	{step}
	{onback}
>
	{#if locked}
		<Notice tone="info"
			><p>
				Your application is being reviewed. You can replace documents after the decision.
			</p></Notice
		>
	{/if}
	{#each kinds as k (k.kind)}
		{@const doc = current(k.kind)}
		<section class="doc">
			<div class="head">
				<h2>{k.name}</h2>
				<span class="badge {doc?.status ?? 'missing'}"
					>{doc ? (statusText[doc.status] ?? doc.status) : 'Needed'}</span
				>
			</div>
			{#if doc?.expires_on}<p class="small">Expires {doc.expires_on}</p>{/if}
			{#if doc?.reject_reason || doc?.upload_reject_reason}
				<Notice tone="danger"><p>{doc.reject_reason ?? doc.upload_reject_reason}</p></Notice>
			{/if}
			{#if open === k.kind}
				<PhotoUpload
					purpose={k.kind}
					label={k.name}
					guidance={k.guidance}
					capture={k.capture ?? 'environment'}
					allowPdf={k.expires}
					onready={(id) => (uploadId = id)}
				/>
				{#if k.expires}
					<TextField label="Expiry date" type="date" min={tomorrow} bind:value={expiresOn} />
				{/if}
				{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
				<div class="actions">
					<Button onclick={() => attach(k.expires)} disabled={!uploadId} loading={busy}
						>Use this document</Button
					>
					<Button variant="ghost" onclick={() => (open = null)}>Cancel</Button>
				</div>
			{:else if !locked}
				<Button variant={doc ? 'secondary' : 'primary'} onclick={() => start(k.kind)}>
					{doc ? 'Replace' : 'Add'}
				</Button>
			{/if}
		</section>
	{/each}
</Screen>

<style>
	.doc {
		display: grid;
		gap: var(--space-3);
		padding: var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: var(--space-3);
	}
	h2 {
		margin: 0;
		font-size: var(--text-lg);
	}
	.badge {
		font-size: var(--text-sm);
		font-weight: 700;
		color: var(--color-text-secondary);
	}
	.approved {
		color: var(--color-success);
	}
	.rejected,
	.expired,
	.missing {
		color: var(--color-attention);
	}
	.small {
		margin: 0;
		font-size: var(--text-sm);
		color: var(--color-text-muted);
	}
	.actions {
		display: grid;
		gap: var(--space-2);
	}
</style>
