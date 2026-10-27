<!--
	Take or choose a photo (or a PDF, for paper documents) and upload it through the signed-URL
	pipeline. Uses the system camera via a file input with `capture`, which works in the
	Capacitor WebView on Android and iOS without an extra plugin. Reports the upload ID once the
	server has accepted the file, or its reason for refusing it.
-->
<script lang="ts">
	import { UploadError, uploadFile, waitForUpload, type DocumentKind } from '@fuber/api-client';
	import { Button, Notice } from '@fuber/ui';
	import { useSession } from './context';

	interface Props {
		purpose: DocumentKind;
		label: string;
		guidance?: string;
		/** `user` opens the front camera (selfie); `environment` the back camera. */
		capture?: 'user' | 'environment';
		allowPdf?: boolean;
		/** Called with the upload ID when the file has passed the checks. */
		onready: (uploadId: string) => void;
	}

	let {
		purpose,
		label,
		guidance,
		capture = 'environment',
		allowPdf = false,
		onready
	}: Props = $props();
	const session = useSession();

	type Phase = 'empty' | 'uploading' | 'checking' | 'ready' | 'rejected';
	let phase = $state<Phase>('empty');
	let preview = $state<string | null>(null);
	let message = $state<string | null>(null);
	let input: HTMLInputElement | undefined = $state();

	$effect(() => () => {
		if (preview) URL.revokeObjectURL(preview);
	});

	async function picked(e: Event) {
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (!file) return;
		if (preview) URL.revokeObjectURL(preview);
		preview = file.type.startsWith('image/') ? URL.createObjectURL(file) : null;
		message = null;
		phase = 'uploading';
		try {
			const id = await uploadFile(session.api, purpose, file);
			phase = 'checking';
			const result = await waitForUpload(session.api, id);
			if (result.status === 'clean') {
				phase = 'ready';
				onready(id);
			} else {
				phase = 'rejected';
				message = result.reject_reason ?? 'This file couldn’t be used. Please try again.';
			}
		} catch (err) {
			phase = 'rejected';
			message =
				err instanceof UploadError
					? err.message
					: 'Upload failed. Please check your connection and try again.';
		} finally {
			if (input) input.value = '';
		}
	}
</script>

<section class="photo" aria-busy={phase === 'uploading' || phase === 'checking'}>
	<div class="head">
		<h2>{label}</h2>
		{#if phase === 'ready'}<span class="ok">✓ Ready</span>{/if}
	</div>
	{#if guidance}<p class="guidance">{guidance}</p>{/if}
	{#if preview}
		<img src={preview} alt="Preview of {label.toLowerCase()}" />
	{/if}
	{#if phase === 'uploading'}
		<p class="status">Uploading…</p>
	{:else if phase === 'checking'}
		<p class="status">Checking the file…</p>
	{:else if phase === 'rejected' && message}
		<Notice tone="danger"><p>{message}</p></Notice>
	{/if}
	<input
		bind:this={input}
		class="visually-hidden"
		type="file"
		accept={allowPdf ? 'image/jpeg,image/png,application/pdf' : 'image/jpeg,image/png'}
		{capture}
		onchange={picked}
		tabindex="-1"
		aria-hidden="true"
	/>
	<Button
		variant={phase === 'ready' ? 'secondary' : 'primary'}
		onclick={() => input?.click()}
		loading={phase === 'uploading' || phase === 'checking'}
	>
		{phase === 'empty' ? (capture === 'user' ? 'Take selfie' : 'Take photo') : 'Retake'}
	</Button>
</section>

<style>
	.photo {
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
	}
	h2 {
		margin: 0;
		font-size: var(--text-lg);
	}
	.ok {
		color: var(--color-success);
		font-weight: 700;
	}
	.guidance,
	.status {
		margin: 0;
		color: var(--color-text-secondary);
		font-size: var(--text-sm);
	}
	img {
		width: 100%;
		max-height: 240px;
		object-fit: contain;
		border-radius: var(--radius-sm);
		background: var(--color-surface-tinted);
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
