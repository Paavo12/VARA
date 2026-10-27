<!-- "Your details": full name, date of birth (18+), optional email (PRD A-2, A-4). -->
<script lang="ts">
	import type { Me } from '@fuber/api-client';
	import { Button, Notice, Screen, TextField } from '@fuber/ui';
	import { untrack } from 'svelte';
	import { messageOf, useSession } from './context';
	import { adultMaxDate } from './phone';

	interface Props {
		me: Me;
		step?: string;
		onsaved: (me: Me) => void;
		onback?: () => void;
	}

	let { me, step, onsaved, onback }: Props = $props();
	const session = useSession();

	// The form starts from the saved values and is then edited locally.
	const saved = untrack(() => me);
	let fullName = $state(saved.full_name ?? '');
	let dob = $state(saved.date_of_birth ?? '');
	let email = $state(saved.email ?? '');
	let error = $state<string | null>(null);
	let busy = $state(false);

	async function save(e: Event) {
		e.preventDefault();
		error = null;
		if (!fullName.trim()) return void (error = 'Enter your full name as it appears on your ID.');
		if (!dob) return void (error = 'Enter your date of birth.');
		busy = true;
		const body = me.identity_locked
			? { email: email.trim() || null }
			: { full_name: fullName, date_of_birth: dob, email: email.trim() || null };
		const { data, error: err } = await session.api
			.PATCH('/v1/me', { body })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data) return void (error = messageOf(err));
		onsaved(data);
	}
</script>

<Screen
	title="Your details"
	lead="Use the name on your ID. Riders and drivers only see your first name."
	{step}
	{onback}
>
	<form id="details" onsubmit={save} novalidate>
		<TextField
			label="Full name"
			autocomplete="name"
			bind:value={fullName}
			disabled={me.identity_locked}
		/>
		<TextField
			label="Date of birth"
			type="date"
			autocomplete="bday"
			max={adultMaxDate()}
			bind:value={dob}
			disabled={me.identity_locked}
			hint="You must be 18 or older."
		/>
		<TextField
			label="Email (optional)"
			type="email"
			autocomplete="email"
			bind:value={email}
			hint="For receipts."
		/>
	</form>
	{#if me.identity_locked}
		<Notice tone="info"
			><p>Your name and date of birth are verified. Contact support to change them.</p></Notice
		>
	{/if}
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#snippet footer()}
		<Button type="submit" form="details" loading={busy}>Continue</Button>
	{/snippet}
</Screen>

<style>
	form {
		display: grid;
		gap: var(--space-4);
	}
</style>
