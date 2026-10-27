<!--
	Emergency contacts (PRD A-3): up to three people alerted by SMS if SOS is pressed. Can be
	skipped during sign-up and edited later from settings.
-->
<script lang="ts">
	import { Button, Notice, Screen, TextField } from '@fuber/ui';
	import { onMount } from 'svelte';
	import { messageOf, useSession } from './context';
	import { normalisePhone } from './phone';

	interface Props {
		step?: string;
		/** Offer "Skip for now" (during sign-up). */
		skippable?: boolean;
		ondone: () => void;
		onback?: () => void;
	}

	let { step, skippable = false, ondone, onback }: Props = $props();
	const session = useSession();

	let contacts = $state<{ name: string; phone: string }[]>([]);
	let error = $state<string | null>(null);
	let busy = $state(false);
	let loaded = $state(false);

	onMount(async () => {
		const { data } = await session.api
			.GET('/v1/me/emergency-contacts')
			.catch(() => ({ data: undefined }));
		contacts = data?.contacts.map((c) => ({ name: c.name, phone: c.phone })) ?? [];
		if (contacts.length === 0) contacts = [{ name: '', phone: '' }];
		loaded = true;
	});

	async function save(e: Event) {
		e.preventDefault();
		error = null;
		const filled = contacts.filter((c) => c.name.trim() || c.phone.trim());
		const body = [];
		for (const c of filled) {
			const phone = normalisePhone(c.phone);
			if (!c.name.trim() || !phone)
				return void (error = 'Each contact needs a name and a Namibian mobile number.');
			body.push({ name: c.name.trim(), phone });
		}
		busy = true;
		const { data, error: err } = await session.api
			.PUT('/v1/me/emergency-contacts', { body: { contacts: body } })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data) return void (error = messageOf(err));
		ondone();
	}
</script>

<Screen
	title="Emergency contacts"
	lead="If you press SOS during a ride, we alert these people by SMS with your live location."
	{step}
	{onback}
>
	{#if loaded}
		<form id="contacts" onsubmit={save} novalidate>
			{#each contacts as contact, i (i)}
				<fieldset>
					<legend>Contact {i + 1}</legend>
					<TextField label="Name" autocomplete="off" bind:value={contact.name} />
					<TextField label="Mobile number" type="tel" inputmode="tel" bind:value={contact.phone} />
					{#if contacts.length > 1}
						<Button variant="ghost" onclick={() => contacts.splice(i, 1)}>Remove</Button>
					{/if}
				</fieldset>
			{/each}
			{#if contacts.length < 3}
				<Button variant="secondary" onclick={() => contacts.push({ name: '', phone: '' })}
					>Add another contact</Button
				>
			{/if}
		</form>
	{/if}
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#snippet footer()}
		<Button type="submit" form="contacts" loading={busy}>Save</Button>
		{#if skippable}<Button variant="ghost" onclick={ondone}>Skip for now</Button>{/if}
	{/snippet}
</Screen>

<style>
	form {
		display: grid;
		gap: var(--space-4);
	}
	fieldset {
		display: grid;
		gap: var(--space-3);
		margin: 0;
		padding: var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	legend {
		font-weight: 700;
		padding: 0 var(--space-1);
	}
</style>
