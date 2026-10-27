<!--
	Accept the Terms and Privacy Policy (and, for drivers, the Driver Agreement) — PRD A-7.
	Shown at sign-up and again whenever a new version needs re-acceptance. Nothing is pre-ticked.
-->
<script lang="ts">
	import type { LegalDocument } from '@fuber/api-client';
	import { Button, Notice, Screen } from '@fuber/ui';
	import { messageOf, useSession } from './context';

	interface Props {
		documents: LegalDocument[];
		step?: string;
		/** True when the user accepted an earlier version already. */
		updated?: boolean;
		onaccepted: () => void;
	}

	let { documents, step, updated = false, onaccepted }: Props = $props();
	const session = useSession();

	const names: Record<string, string> = {
		terms: 'Terms and Conditions',
		privacy: 'Privacy Policy',
		driver_agreement: 'Driver Agreement'
	};
	let agreed = $state(false);
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function accept() {
		error = null;
		busy = true;
		const { response, error: err } = await session.api
			.POST('/v1/consents', { body: { document_ids: documents.map((d) => d.id) } })
			.catch(() => ({ response: undefined, error: undefined }));
		busy = false;
		if (response?.status !== 204) return void (error = messageOf(err));
		onaccepted();
	}
</script>

<Screen
	title={updated ? 'We’ve updated our terms' : 'Before you continue'}
	lead="Please read and accept:"
	{step}
>
	<ul>
		{#each documents as doc (doc.id)}
			<li>
				<a href={doc.url} target="_blank" rel="noopener noreferrer">{names[doc.kind] ?? doc.kind}</a
				>
				<span class="version">version {doc.version}</span>
			</li>
		{/each}
	</ul>
	<label class="check">
		<input type="checkbox" bind:checked={agreed} />
		<span>I have read and accept these documents.</span>
	</label>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#snippet footer()}
		<Button onclick={accept} disabled={!agreed} loading={busy}>Accept and continue</Button>
	{/snippet}
</Screen>

<style>
	ul {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: var(--space-3);
	}
	li {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	a {
		color: var(--color-brand);
		font-weight: 600;
	}
	.version {
		font-size: var(--text-xs);
		color: var(--color-text-muted);
	}
	.check {
		display: flex;
		gap: var(--space-3);
		align-items: center;
		min-height: var(--touch-target);
	}
	.check input {
		width: 22px;
		height: 22px;
		accent-color: var(--color-brand);
	}
</style>
