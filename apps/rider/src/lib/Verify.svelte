<!-- Rider identity verification (App Flow §2.1): ID photo, live selfie, optional ID number. -->
<script lang="ts">
	import { PhotoUpload, messageOf, useSession } from '@fuber/account';
	import { Button, Notice, Screen, TextField } from '@fuber/ui';

	interface Props {
		step?: string;
		/** Shown when a previous attempt was rejected. */
		rejectReason?: string | null;
		onsubmitted: () => void;
	}

	let { step, rejectReason, onsubmitted }: Props = $props();
	const session = useSession();

	let idUpload = $state<string | null>(null);
	let selfieUpload = $state<string | null>(null);
	let nationalId = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function submit() {
		if (!idUpload || !selfieUpload) return;
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST('/v1/me/verification', {
				body: {
					id_document_upload_id: idUpload,
					selfie_upload_id: selfieUpload,
					national_id: nationalId.trim() || null
				}
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data) return void (error = messageOf(err));
		onsubmitted();
	}
</script>

<Screen
	title="Verify it’s you"
	lead="VARA is for women only, so every rider is checked by our team. Your photos are only seen by VARA reviewers."
	{step}
>
	{#if rejectReason}
		<Notice tone="attention" title="Please try again">
			<p>{rejectReason}</p>
		</Notice>
	{/if}
	<PhotoUpload
		purpose="identity"
		label="ID card or passport"
		guidance="Place the photo page flat in good light. All four corners visible, no glare."
		onready={(id) => (idUpload = id)}
	/>
	<PhotoUpload
		purpose="selfie"
		label="Selfie"
		capture="user"
		guidance="Face the camera in good light. No sunglasses, hat or mask."
		onready={(id) => (selfieUpload = id)}
	/>
	<TextField
		label="ID or passport number (optional)"
		autocomplete="off"
		bind:value={nationalId}
		hint="Speeds up checking. Stored encrypted."
	/>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#snippet footer()}
		<Button onclick={submit} disabled={!idUpload || !selfieUpload} loading={busy}
			>Submit for review</Button
		>
	{/snippet}
</Screen>
