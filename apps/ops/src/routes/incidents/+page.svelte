<!--
	Safety desk (PRD S-1, T-3): incidents not yet closed. Closing a PIN lockout needs an outcome,
	cancels the blocked trip and is the only thing that lets the driver get requests again. The
	trip's chat loads only when asked for, because each view goes in the audit log.
-->
<script lang="ts">
	import { Button, Notice, TextField } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import { api, staff, type IncidentItem, type IncidentMessage } from '#lib/staff.svelte.ts';

	const KINDS: Record<string, string> = {
		sos: 'SOS',
		pin_lockout: 'PIN lockout',
		ridecheck_deviation: 'RideCheck: route',
		ridecheck_stop: 'RideCheck: stop',
		report: 'Report'
	};

	let incidents = $state<IncidentItem[] | null>(null);
	let closing = $state<string | null>(null);
	let outcome = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let chats = $state<Record<string, IncidentMessage[]>>({});

	async function showChat(incident: IncidentItem) {
		error = null;
		const { data, error: err } = await api
			.GET('/v1/ops/incidents/{id}/messages', { params: { path: { id: incident.id } } })
			.catch(() => ({ data: undefined, error: undefined }));
		if (data) chats[incident.id] = data;
		else error = messageOf(err, 'The chat didn’t load.');
	}

	async function load() {
		error = null;
		const { data, error: err } = await api
			.GET('/v1/ops/incidents')
			.catch(() => ({ data: undefined, error: undefined }));
		incidents = data ?? null;
		if (!data) error = messageOf(err, 'The incidents didn’t load.');
	}

	async function close(incident: IncidentItem) {
		error = notice = null;
		busy = true;
		const { response, error: err } = await api
			.POST('/v1/ops/incidents/{id}/close', {
				params: { path: { id: incident.id } },
				body: { outcome: outcome.trim() }
			})
			.catch(() => ({ response: undefined, error: undefined }));
		busy = false;
		if (response?.ok) {
			notice =
				incident.kind === 'pin_lockout'
					? `Closed. ${incident.driver?.display_name ?? 'The driver'} can get requests again.`
					: 'Closed.';
			closing = null;
			outcome = '';
			await load();
		} else error = messageOf(err);
	}

	$effect(() => {
		if (staff.canHandleIncidents) load();
	});
</script>

<svelte:head><title>Safety desk · VARA Ops</title></svelte:head>

{#if !staff.canHandleIncidents}
	<Notice tone="attention" title="No safety access">
		<p>Your role can’t handle incidents. Ask an ops admin for the safety role.</p>
	</Notice>
{:else}
	<div class="top">
		<h1>Safety desk</h1>
		<button type="button" class="link" onclick={load}>Refresh</button>
	</div>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if notice}<Notice tone="success"><p>{notice}</p></Notice>{/if}

	{#if incidents && incidents.length === 0}
		<p class="empty">No open incidents.</p>
	{:else if incidents}
		<ul class="list">
			{#each incidents as i (i.id)}
				<li>
					<div class="head">
						<strong>{KINDS[i.kind] ?? i.kind}</strong>
						<span>{i.ride_code ?? 'No trip'}</span>
						<span>{formatWhen(i.created_at)}</span>
					</div>
					{#if i.driver}
						<p>
							Driver: {i.driver.display_name ?? 'Name not given'}, {i.driver.phone}
							{#if i.kind === 'pin_lockout'}<span class="held">Held from new requests</span>{/if}
						</p>
					{/if}
					{#if i.raised_by}
						<p>Raised by: {i.raised_by.display_name ?? 'Name not given'}, {i.raised_by.phone}</p>
					{/if}
					{#if chats[i.id]}
						<ol class="chat" aria-label="Chat on this trip">
							{#each chats[i.id] as m (m.id)}
								<li>
									<span>{m.sender === 'rider' ? 'Rider' : 'Driver'}, {formatWhen(m.sent_at)}</span>
									<p>{m.text}</p>
								</li>
							{:else}
								<li><p>No messages on this trip.</p></li>
							{/each}
						</ol>
					{:else if i.ride_id}
						<button type="button" class="link" onclick={() => showChat(i)}>Show chat</button>
					{/if}
					{#if closing === i.id}
						<TextField label="Outcome of the review" bind:value={outcome} maxlength={500} />
						<div class="actions">
							<Button onclick={() => close(i)} loading={busy} disabled={!outcome.trim()}>
								{i.kind === 'pin_lockout' ? 'Close and release driver' : 'Close incident'}
							</Button>
							<Button variant="ghost" onclick={() => (closing = null)}>Keep open</Button>
						</div>
					{:else}
						<Button
							variant="secondary"
							onclick={() => {
								closing = i.id;
								outcome = '';
							}}>Review and close</Button
						>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
{/if}

<style>
	.top {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: var(--space-3);
	}
	h1 {
		margin: 0 0 var(--space-4);
		font-size: var(--text-2xl);
	}
	.link {
		background: none;
		border: 0;
		padding: var(--space-1);
		font: inherit;
		color: var(--color-brand);
		cursor: pointer;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-3);
	}
	.list li {
		display: grid;
		gap: var(--space-2);
		padding: var(--space-3) var(--space-4);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
	}
	.list p {
		margin: 0;
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-3);
		align-items: baseline;
	}
	.head span {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.held {
		margin-left: var(--space-2);
		color: var(--color-brand);
		font-weight: 700;
		font-size: var(--text-sm);
	}
	.chat {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: var(--space-2);
	}
	.chat li {
		display: block;
		padding: 0;
		border: 0;
	}
	.chat span {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.chat p {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.link {
		justify-self: start;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
	}
	.empty {
		color: var(--color-text-muted);
	}
</style>
