<!--
	Delete account from inside the app (PRD A-8). Explains what happens, asks for typed
	confirmation, then signs out on this device.
-->
<script lang="ts">
	import { Button, Notice, Screen, TextField } from '@fuber/ui';
	import { messageOf, useSession } from './context';

	interface Props {
		ondeleted: () => void;
		onback: () => void;
	}

	let { ondeleted, onback }: Props = $props();
	const session = useSession();

	let confirm = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function remove() {
		error = null;
		busy = true;
		const { response, error: err } = await session.api
			.DELETE('/v1/me')
			.catch(() => ({ response: undefined, error: undefined }));
		busy = false;
		if (response?.status !== 202) return void (error = messageOf(err));
		await session.forget();
		ondeleted();
	}
</script>

<Screen title="Delete your account" {onback}>
	<Notice tone="attention" title="This can’t be undone after 30 days">
		<p>
			You’ll be signed out on all your devices straight away. We erase your name, number, photos, ID
			details and emergency contacts after 30 days. Records the law requires us to keep, such as
			trip receipts, are kept without your personal details. To change your mind within 30 days,
			contact VARA support.
		</p>
	</Notice>
	<TextField label="Type DELETE to confirm" autocomplete="off" bind:value={confirm} />
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#snippet footer()}
		<Button variant="sos" onclick={remove} disabled={confirm.trim() !== 'DELETE'} loading={busy}
			>Delete my account</Button
		>
		<Button variant="ghost" onclick={onback}>Keep my account</Button>
	{/snippet}
</Screen>
