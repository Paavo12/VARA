<!--
	Audit log (PRD O-7), admins only: every staff action, newest first, 100 at a time. Filter by
	action (`user.suspend`, or a prefix such as `user.`) or by a person's ID. Reading the log is
	itself audited.
-->
<script lang="ts">
	import { Button, Notice, TextField } from '@fuber/ui';
	import { formatDetail, formatWhen, messageOf } from '#lib/format.ts';
	import { api, staff, type AuditEntry } from '#lib/staff.svelte.ts';
	import TableScroll from '#lib/TableScroll.svelte';

	let action = $state('');
	let userId = $state('');
	let entries = $state<AuditEntry[]>([]);
	let more = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);

	async function load(older = false) {
		error = null;
		busy = true;
		const before = older ? entries.at(-1)?.id : undefined;
		const { data, error: err } = await api
			.GET('/v1/ops/audit', {
				params: {
					query: { action: action.trim() || undefined, user_id: userId.trim() || undefined, before }
				}
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data) {
			error = messageOf(err, 'The audit log didn’t load.');
			return;
		}
		entries = older ? [...entries, ...data] : data;
		more = data.length === 100;
	}

	$effect(() => {
		if (staff.canSettle) void load();
	});
</script>

<svelte:head><title>Audit log · VARA Ops</title></svelte:head>

{#if !staff.canSettle}
	<Notice tone="attention" title="Admins only">
		<p>The audit log is for ops admins.</p>
	</Notice>
{:else}
	<h1>Audit log</h1>
	<form
		class="filters"
		onsubmit={(e) => {
			e.preventDefault();
			void load();
		}}
	>
		<TextField label="Action (e.g. user.suspend or user.)" bind:value={action} maxlength={100} />
		<TextField label="Person ID" bind:value={userId} maxlength={36} />
		<Button type="submit" loading={busy}>Filter</Button>
	</form>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if entries.length === 0 && !busy}
		<p class="muted">No entries.</p>
	{:else}
		<TableScroll label="Audit entries">
			<table>
				<thead>
					<tr
						><th>When</th><th>Staff</th><th>Action</th><th>Target</th><th>Detail</th><th>IP</th></tr
					>
				</thead>
				<tbody>
					{#each entries as e (e.id)}
						<tr>
							<td>{formatWhen(e.at)}</td>
							<td>{e.staff_email ?? 'System'}</td>
							<td><code>{e.action}</code></td>
							<td>{e.target_type} <code>{e.target_id}</code></td>
							<td><code class="detail">{formatDetail(e.detail)}</code></td>
							<td>{e.ip ?? ''}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</TableScroll>
		{#if more}<Button variant="ghost" loading={busy} onclick={() => load(true)}
				>Older entries</Button
			>{/if}
	{/if}
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	.filters {
		display: flex;
		gap: var(--space-3);
		align-items: flex-end;
		flex-wrap: wrap;
		margin-bottom: var(--space-4);
	}
	table {
		width: 100%;
		border-collapse: collapse;
		background: var(--color-surface);
		font-size: var(--text-sm);
	}
	th,
	td {
		text-align: left;
		padding: var(--space-2);
		border-bottom: 1px solid var(--color-border);
		vertical-align: top;
	}
	.detail {
		word-break: break-word;
	}
	.muted {
		color: var(--color-text-muted);
	}
</style>
