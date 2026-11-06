<!--
	Vehicle, requested ride types and payout account (App Flow §2.2, PRD A-4). No vehicle
	registration document: the driver may not own the car.
-->
<script lang="ts">
	import type { DriverApplication, Tier } from '@fuber/api-client';
	import { messageOf, useSession } from '@fuber/account';
	import { Button, Notice, Screen, TextField } from '@fuber/ui';
	import { untrack } from 'svelte';

	interface Props {
		application: DriverApplication;
		step?: string;
		onsaved: (a: DriverApplication) => void;
		onback?: () => void;
	}

	let { application, step, onsaved, onback }: Props = $props();
	const session = useSession();

	const tiers: { id: Tier; name: string; note: string }[] = [
		{ id: 'classic', name: 'Classic', note: 'Everyday rides, any tidy 4-seater' },
		{ id: 'comfort', name: 'Comfort', note: 'Newer cars with extra legroom' },
		{ id: 'softlife', name: 'SoftLife', note: 'Premium cars and service' }
	];

	const saved = untrack(() => application);
	const v = saved.vehicle;
	let make = $state(v?.make ?? '');
	let model = $state(v?.model ?? '');
	let colour = $state(v?.colour ?? '');
	let year = $state(v ? String(v.year) : '');
	let plate = $state(v?.plate ?? '');
	let seats = $state(v ? String(v.seats) : '4');
	let wheelchair = $state(v?.wheelchair_accessible ?? false);
	let requested = $state<Tier[]>(
		saved.requested_tiers.length ? [...saved.requested_tiers] : ['classic']
	);
	let bank = $state('');
	let holder = $state('');
	let account = $state('');
	let branch = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);

	function toggle(t: Tier) {
		requested = requested.includes(t) ? requested.filter((x) => x !== t) : [...requested, t];
	}

	async function save(e: Event) {
		e.preventDefault();
		error = null;
		if (!requested.length) return void (error = 'Choose at least one ride type.');
		const payoutTyped = bank || holder || account || branch;
		if (!saved.payout_account_saved && !payoutTyped)
			return void (error = 'Add the bank account for your weekly payouts.');
		busy = true;
		const { data, error: err } = await session.api
			.PUT('/v1/driver/application', {
				body: {
					requested_tiers: requested,
					vehicle: {
						make,
						model,
						colour,
						year: Number(year),
						plate,
						seats: Number(seats),
						wheelchair_accessible: wheelchair
					},
					payout_account: payoutTyped
						? { bank, account_holder: holder, account_number: account, branch_code: branch }
						: null
				}
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data) return void (error = messageOf(err));
		onsaved(data);
	}
</script>

<Screen
	title="Your car"
	lead="Tell us about the car you’ll drive. You don’t need to own it."
	{step}
	{onback}
>
	<form id="vehicle" onsubmit={save} novalidate>
		<TextField label="Make" placeholder="Toyota" bind:value={make} />
		<TextField label="Model" placeholder="Corolla" bind:value={model} />
		<TextField label="Colour" placeholder="Silver" bind:value={colour} />
		<div class="row">
			<TextField label="Year" inputmode="numeric" maxlength={4} bind:value={year} />
			<TextField
				label="Seats"
				inputmode="numeric"
				maxlength={1}
				bind:value={seats}
				hint="Including yours"
			/>
		</div>
		<TextField
			label="Number plate"
			placeholder="N 1234 W"
			autocapitalize="characters"
			bind:value={plate}
		/>
		<label class="check">
			<input type="checkbox" bind:checked={wheelchair} />
			<span>Wheelchair accessible</span>
		</label>

		<fieldset>
			<legend>Ride types you’d like to offer</legend>
			{#each tiers as t (t.id)}
				<label class="check">
					<input type="checkbox" checked={requested.includes(t.id)} onchange={() => toggle(t.id)} />
					<span><strong>{t.name}</strong> · {t.note}</span>
				</label>
			{/each}
			<p class="small">VARA checks your car and confirms which types you can drive.</p>
		</fieldset>

		<fieldset>
			<legend>Payout account</legend>
			{#if saved.payout_account_saved}
				<p class="small">An account is saved. Fill in these fields only to replace it.</p>
			{/if}
			<TextField label="Bank" placeholder="Bank Windhoek" bind:value={bank} />
			<TextField label="Account holder" autocomplete="name" bind:value={holder} />
			<TextField label="Account number" inputmode="numeric" bind:value={account} />
			<TextField label="Branch code" inputmode="numeric" bind:value={branch} />
			<p class="small">Stored encrypted. Earnings are paid weekly.</p>
		</fieldset>
	</form>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#snippet footer()}
		<Button size="driver" type="submit" form="vehicle" loading={busy}>Save</Button>
	{/snippet}
</Screen>

<style>
	form,
	fieldset {
		display: grid;
		gap: var(--space-4);
	}
	fieldset {
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
	.row {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--space-3);
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
		flex: none;
		accent-color: var(--color-brand);
	}
	.small {
		margin: 0;
		font-size: var(--text-sm);
		color: var(--color-text-muted);
	}
</style>
