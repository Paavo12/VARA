<!--
	Report an issue (PRD S-5) from either app, during a ride or after it (up to 30 days). Safety
	categories reach the safety desk as an incident; the rest go to support.
-->
<script lang="ts">
	import type { IssueCategory } from '@fuber/api-client';
	import { Button, Notice } from '@fuber/ui';
	import { messageOf, useSession } from './context';

	interface Props {
		rideId: string;
		/** Whose app: the wording differs ("driver" vs "rider"). */
		party: 'rider' | 'driver';
	}

	let { rideId, party }: Props = $props();
	const session = useSession();

	const MAX = 2000;
	const categories = $derived<{ id: IssueCategory; label: string }[]>([
		{ id: 'harassment', label: 'Harassment or threats' },
		{
			id: 'unsafe_driving',
			label: party === 'rider' ? 'Unsafe driving' : 'Unsafe behaviour in the car'
		},
		{
			id: 'wrong_person',
			label:
				party === 'rider'
					? 'The driver was not the person shown'
					: 'The rider was not the person booked'
		},
		{ id: 'wrong_vehicle', label: 'Wrong car or plate' },
		{ id: 'lost_item', label: party === 'rider' ? 'I lost an item' : 'A rider left an item' },
		{ id: 'fare', label: 'Fare or payment' },
		{ id: 'other', label: 'Something else' }
	]);

	let open = $state(false);
	let category = $state<IssueCategory | null>(null);
	let text = $state('');
	let busy = $state(false);
	let error = $state('');
	let done = $state('');

	async function send(e: SubmitEvent) {
		e.preventDefault();
		if (!category || !text.trim()) return;
		busy = true;
		error = '';
		const { data, error: err } = await session.api
			.POST('/v1/rides/{id}/report', {
				params: { path: { id: rideId } },
				body: { category, text: text.trim() }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			done = data.message;
			text = '';
			category = null;
		} else {
			error = messageOf(err, "We couldn't send your report. Check your connection and try again.");
		}
	}
</script>

{#if done}
	<Notice tone="success" title="Report sent"><p>{done}</p></Notice>
{:else if !open}
	<Button variant="ghost" onclick={() => (open = true)}>Report an issue</Button>
{:else}
	<form class="report" onsubmit={send} aria-label="Report an issue">
		<fieldset>
			<legend>What happened?</legend>
			{#each categories as c (c.id)}
				<label class="option">
					<input type="radio" name="category" value={c.id} bind:group={category} />
					{c.label}
				</label>
			{/each}
		</fieldset>
		<label class="text">
			Tell us more
			<textarea bind:value={text} maxlength={MAX} rows="4" required></textarea>
		</label>
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		<div class="actions">
			<Button type="submit" loading={busy} disabled={!category || !text.trim()}>Send report</Button>
			<Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
		</div>
	</form>
{/if}

<style>
	.report {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	legend {
		font-weight: 700;
		margin-bottom: var(--space-2);
	}
	.option {
		display: flex;
		gap: var(--space-2);
		align-items: center;
		min-height: 44px;
	}
	.text {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}
	textarea {
		font: inherit;
		padding: var(--space-2);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-sm);
		background: var(--color-surface);
		color: var(--color-text);
	}
	.actions {
		display: flex;
		gap: var(--space-2);
		flex-wrap: wrap;
	}
</style>
