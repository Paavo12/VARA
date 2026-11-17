<!--
	One user (PRD O-2, O-6): status, priority points, last trips and reports. Support can suspend
	or reinstate with a reason and adjust priority points. Opening this page is audited.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { Button, Notice, TextField, formatNad } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import { api, type UserDetail } from '#lib/staff.svelte.ts';

	const id = $derived(page.params.id ?? '');
	let user = $state<UserDetail | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let reason = $state('');
	let busy = $state(false);
	let role = $state<'rider' | 'driver'>('rider');
	let delta = $state(10);
	let pointsReason = $state('');

	async function load() {
		error = null;
		const { data, error: err } = await api
			.GET('/v1/ops/users/{id}', { params: { path: { id } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) {
			user = data;
			role = data.rider_priority != null ? 'rider' : 'driver';
		} else error = messageOf(err, 'This user didn’t load.');
	}

	async function setStatus(action: 'suspend' | 'reinstate') {
		error = notice = null;
		busy = true;
		const path =
			action === 'suspend' ? '/v1/ops/users/{id}/suspend' : '/v1/ops/users/{id}/reinstate';
		const { response, error: err } = await api
			.POST(path, { params: { path: { id } }, body: { reason: reason.trim() } })
			.catch(() => ({ response: undefined, error: undefined }));
		busy = false;
		if (response?.ok) {
			notice = action === 'suspend' ? 'Suspended. She is signed out and offline.' : 'Reinstated.';
			reason = '';
			await load();
		} else error = messageOf(err);
	}

	async function adjust() {
		error = notice = null;
		busy = true;
		const { data, error: err } = await api
			.POST('/v1/ops/users/{id}/priority', {
				params: { path: { id } },
				body: { role, delta, reason: pointsReason.trim() }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			notice = `Priority points now ${data.points}.`;
			pointsReason = '';
			await load();
		} else error = messageOf(err);
	}

	$effect(() => {
		if (id) void load();
	});
</script>

<svelte:head><title>User · VARA Ops</title></svelte:head>

<p><a href={resolve('/users')}>← Users</a></p>
{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
{#if notice}<Notice tone="success"><p>{notice}</p></Notice>{/if}

{#if user}
	<h1>{user.user.full_name ?? user.user.display_name ?? 'Name not given'}</h1>
	<dl class="facts">
		<div>
			<dt>Phone</dt>
			<dd>{user.user.phone ?? '—'}</dd>
		</div>
		<div>
			<dt>Status</dt>
			<dd>{user.user.status}{user.suspended_reason ? ` (${user.suspended_reason})` : ''}</dd>
		</div>
		<div>
			<dt>Identity</dt>
			<dd>{user.verification}</dd>
		</div>
		{#if user.rider_priority != null}<div>
				<dt>Rider priority</dt>
				<dd>{user.rider_priority}</dd>
			</div>{/if}
		{#if user.driver_priority != null}<div>
				<dt>Driver priority</dt>
				<dd>{user.driver_priority}</dd>
			</div>{/if}
	</dl>

	<section class="box" aria-labelledby="account">
		<h2 id="account">Account</h2>
		<TextField label="Reason (kept in the audit log)" bind:value={reason} maxlength={500} />
		{#if user.user.status === 'suspended'}
			<Button onclick={() => setStatus('reinstate')} loading={busy} disabled={!reason.trim()}
				>Reinstate</Button
			>
		{:else if user.user.status !== 'deleted'}
			<Button
				variant="sos"
				onclick={() => setStatus('suspend')}
				loading={busy}
				disabled={!reason.trim()}>Suspend</Button
			>
		{/if}
	</section>

	{#if user.rider_priority != null || user.driver_priority != null}
		<section class="box" aria-labelledby="points">
			<h2 id="points">Priority points</h2>
			{#if user.rider_priority != null && user.driver_priority != null}
				<label
					>Side <select bind:value={role}
						><option value="rider">Rider</option><option value="driver">Driver</option></select
					></label
				>
			{/if}
			<label>Change (−50 to 50) <input type="number" min="-50" max="50" bind:value={delta} /></label
			>
			<TextField label="Reason" bind:value={pointsReason} maxlength={500} />
			<Button
				variant="secondary"
				onclick={adjust}
				loading={busy}
				disabled={!pointsReason.trim() || !delta}>Adjust points</Button
			>
		</section>
	{/if}

	<h2>Last trips</h2>
	{#if user.trips.length === 0}<p class="muted">No trips.</p>{/if}
	<ul class="list">
		{#each user.trips as t (t.ride_id)}
			<li>
				<strong>{t.code}</strong>
				<span>as {t.as_party}</span>
				<span>{t.state}</span>
				<span>{t.pickup_address} → {t.dropoff_address}</span>
				<span>{formatNad(t.fare)}</span>
				<span>{formatWhen(t.requested_at)}</span>
			</li>
		{/each}
	</ul>

	<h2>Reports she made</h2>
	{#if user.reports.length === 0}<p class="muted">None.</p>{/if}
	<ul class="list">
		{#each user.reports as r (r.id)}
			<li>
				<strong>{r.category}</strong>
				<span>{r.status}</span>
				<span>{formatWhen(r.created_at)}</span>
				<p class="text">{r.text}</p>
			</li>
		{/each}
	</ul>
{:else if !error}
	<p class="muted" role="status">Loading…</p>
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	.facts {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
		gap: var(--space-3);
	}
	.facts dt {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.facts dd {
		margin: 0;
		font-weight: 700;
	}
	.box {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		align-items: flex-start;
		margin: var(--space-4) 0;
		padding: var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
	}
	.box h2 {
		margin: 0;
	}
	.list {
		list-style: none;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	.list li {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-3);
		padding: var(--space-3);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-sm);
	}
	.text {
		flex-basis: 100%;
		margin: 0;
		white-space: pre-wrap;
	}
	.muted {
		color: var(--color-text-muted);
	}
</style>
