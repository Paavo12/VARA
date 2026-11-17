<!--
	User search (PRD O-2): by phone number (081… or +264…) or part of a name. Each search goes
	in the audit log.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Button, Notice, TextField } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import { api, staff, type UserItem } from '#lib/staff.svelte.ts';

	let q = $state('');
	let results = $state<UserItem[] | null>(null);
	let busy = $state(false);
	let error = $state<string | null>(null);

	async function search(e: SubmitEvent) {
		e.preventDefault();
		error = null;
		busy = true;
		const { data, error: err } = await api
			.GET('/v1/ops/users', { params: { query: { q: q.trim() } } })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		results = data ?? null;
		if (!data) error = messageOf(err, 'The search didn’t work.');
	}

	const STATUS: Record<string, string> = {
		pending: 'Not verified yet',
		active: 'Active',
		suspended: 'Suspended',
		deleted: 'Deleted'
	};
</script>

<svelte:head><title>Users · VARA Ops</title></svelte:head>

{#if !staff.canSupport}
	<Notice tone="attention" title="No support access">
		<p>Your role can’t look up users. Ask an ops admin for the support role.</p>
	</Notice>
{:else}
	<h1>Users</h1>
	<form class="search" onsubmit={search}>
		<TextField label="Phone number or name" bind:value={q} maxlength={80} />
		<Button type="submit" loading={busy} disabled={q.trim().length < 2}>Search</Button>
	</form>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if results && results.length === 0}
		<p class="empty">Nobody found.</p>
	{:else if results}
		<ul class="list">
			{#each results as u (u.user_id)}
				<li>
					<a href={resolve('/users/[id]', { id: u.user_id })}>
						<strong>{u.full_name ?? u.display_name ?? 'Name not given'}</strong>
						<span>{u.phone ?? '—'}</span>
						<span
							>{[u.is_rider && 'Rider', u.is_driver && 'Driver'].filter(Boolean).join(', ')}</span
						>
						<span class:warn={u.status === 'suspended'}>{STATUS[u.status] ?? u.status}</span>
						<span>Joined {formatWhen(u.created_at)}</span>
					</a>
				</li>
			{/each}
		</ul>
	{/if}
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	.search {
		display: flex;
		gap: var(--space-3);
		align-items: flex-end;
		flex-wrap: wrap;
		margin-bottom: var(--space-4);
	}
	.list {
		list-style: none;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	.list a {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
		color: var(--color-text);
		text-decoration: none;
	}
	.warn {
		color: var(--color-attention);
		font-weight: 700;
	}
	.empty {
		color: var(--color-text-muted);
	}
</style>
