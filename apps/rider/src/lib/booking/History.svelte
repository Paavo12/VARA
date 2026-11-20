<!--
	Your trips (PRD R-2): newest first, 20 at a time, with the fare, the driver's first name and
	the stars you gave.
-->
<script lang="ts">
	import type { components } from '@fuber/api-client';
	import { useSession } from '@fuber/account';
	import { Button, Screen, formatNad } from '@fuber/ui';
	import { onMount } from 'svelte';

	type RiderTrip = components['schemas']['RiderTrip'];

	interface Props {
		onback: () => void;
	}

	let { onback }: Props = $props();
	const session = useSession();

	let trips = $state<RiderTrip[]>([]);
	let more = $state(true);
	let busy = $state(false);
	let failed = $state(false);

	async function load() {
		busy = true;
		const before = trips.at(-1)?.requested_at;
		const { data } = await session.api
			.GET('/v1/rides', { params: { query: { before } } })
			.catch(() => ({ data: undefined }));
		busy = false;
		if (!data) return void (failed = true);
		trips = [...trips, ...data];
		more = data.length === 20;
	}

	onMount(() => void load());

	const STATES: Record<string, string> = {
		COMPLETED: 'Completed',
		RATED: 'Completed',
		CANCELLED: 'Cancelled',
		BLOCKED: 'Stopped for safety',
		SCHEDULED: 'Scheduled'
	};
	const day = (iso: string) =>
		new Date(iso).toLocaleString('en-NA', {
			day: 'numeric',
			month: 'short',
			year: 'numeric',
			hour: '2-digit',
			minute: '2-digit'
		});
	const done = (s: string) => s === 'COMPLETED' || s === 'RATED';
</script>

<Screen title="Your trips">
	{#if failed}<p class="muted">Your trips didn’t load. Check your connection.</p>{/if}
	{#if trips.length === 0 && !busy && !failed}
		<p class="muted">No trips yet. Your rides appear here.</p>
	{/if}
	<ul>
		{#each trips as t (t.id)}
			<li>
				<div class="where">
					<strong>{t.dropoff_address}</strong>
					<small>From {t.pickup_address}</small>
					<small
						>{day(t.completed_at ?? t.requested_at)} · {STATES[t.state] ??
							'In progress'}{t.driver_first_name ? ` · ${t.driver_first_name}` : ''}</small
					>
				</div>
				<div class="right">
					<strong>{done(t.state) ? formatNad(t.fare) : '—'}</strong>
					{#if t.my_rating}<small
							><span aria-hidden="true">{'★'.repeat(t.my_rating)}</span><span class="sr-only"
								>You gave {t.my_rating} stars</span
							></small
						>{/if}
				</div>
			</li>
		{/each}
	</ul>
	{#if more && trips.length > 0}
		<Button variant="ghost" onclick={load} loading={busy}>Show older trips</Button>
	{/if}
	{#snippet footer()}<Button variant="secondary" onclick={onback}>Back</Button>{/snippet}
</Screen>

<style>
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-2);
	}
	li {
		display: flex;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-3);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.where {
		display: grid;
		min-width: 0;
	}
	.right {
		display: grid;
		justify-items: end;
		font-variant-numeric: tabular-nums;
	}
	small,
	.muted {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.muted {
		margin: 0;
	}
</style>
