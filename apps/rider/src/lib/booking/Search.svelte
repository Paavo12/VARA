<!--
	Where to? / Pickup search (PRD B-2, design brief §5.1 items 4–5). Landmarks, suburbs and
	streets; case and accents don't matter. With nothing typed, the landmarks show as quick
	picks, as in the demo.
-->
<script lang="ts">
	import type { Place } from '@fuber/api-client';
	import { messageOf, useSession } from '@fuber/account';
	import { Notice, Screen, TextField } from '@fuber/ui';
	import { onMount } from 'svelte';
	import type { Stop } from './types';

	interface Props {
		title: string;
		label: string;
		/** Offered at the top, e.g. the phone's current location for the pickup. */
		current?: Stop | null;
		onpick: (stop: Stop) => void;
		onback: () => void;
	}

	let { title, label, current = null, onpick, onback }: Props = $props();
	const session = useSession();

	let query = $state('');
	let results = $state<Place[]>([]);
	let error = $state<string | null>(null);
	let searched = $state(false);
	let timer: ReturnType<typeof setTimeout> | undefined;
	let latest = 0;

	async function search(q: string) {
		const mine = ++latest;
		const { data, error: err } = await session.api
			.GET('/v1/places', { params: { query: { q } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (mine !== latest) return; // a newer search is on its way
		if (!data) return void (error = messageOf(err, 'We couldn’t search right now.'));
		error = null;
		results = data;
		searched = q.trim().length > 0;
	}

	function typed() {
		clearTimeout(timer);
		timer = setTimeout(() => search(query), 250);
	}

	onMount(() => {
		void search('');
		return () => clearTimeout(timer);
	});

	const icon = (kind: string) => (kind === 'landmark' ? '★' : kind === 'suburb' ? '◎' : '—');
</script>

<Screen {title} {onback}>
	<TextField
		{label}
		bind:value={query}
		oninput={typed}
		placeholder="Landmark, suburb or street"
		autocomplete="off"
		enterkeyhint="search"
		maxlength={80}
	/>
	{#if current && !searched}
		<button class="place" onclick={() => onpick(current)}>
			<span class="icon" aria-hidden="true">◉</span>
			<span><strong>Current location</strong><small>{current.address}</small></span>
		</button>
	{/if}
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if !searched && results.length}<p class="hint">Popular places</p>{/if}
	<ul class="results">
		{#each results as p (p.id)}
			<li>
				<button class="place" onclick={() => onpick({ address: p.name, lat: p.lat, lon: p.lon })}>
					<span class="icon" aria-hidden="true">{icon(p.kind)}</span>
					<span><strong>{p.name}</strong><small>{p.detail}</small></span>
				</button>
			</li>
		{/each}
	</ul>
	{#if searched && results.length === 0 && !error}
		<p class="hint">No places match “{query}”. Try a landmark, suburb or street name.</p>
	{/if}
	<!-- Results change as she types; say how many (WCAG 4.1.3). -->
	<p class="sr-only" aria-live="polite">
		{searched && !error
			? results.length === 0
				? 'No places found'
				: `${results.length} place${results.length === 1 ? '' : 's'} found`
			: ''}
	</p>
</Screen>

<style>
	.results {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-1);
	}
	.place {
		width: 100%;
		display: flex;
		gap: var(--space-3);
		align-items: center;
		min-height: var(--touch-target);
		padding: var(--space-2) var(--space-3);
		border: none;
		border-radius: var(--radius-sm);
		background: var(--color-surface);
		color: var(--color-text);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.place span:last-child {
		display: grid;
	}
	.place small {
		color: var(--color-text-muted);
	}
	.icon {
		width: 1.5rem;
		text-align: center;
		color: var(--color-brand);
	}
	.hint {
		margin: 0;
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
</style>
