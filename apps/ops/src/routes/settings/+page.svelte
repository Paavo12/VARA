<!--
	Matching, booking and RideCheck parameters (PRD D-6, O-5), admins only. Each one has bounds
	the API enforces; matching and RideCheck use a change from their next run. Every change is
	audited with the old and new value.
-->
<script lang="ts">
	import { Button, Notice } from '@fuber/ui';
	import { formatWhen, messageOf } from '#lib/format.ts';
	import { api, staff, type Setting } from '#lib/staff.svelte.ts';
	import TableScroll from '#lib/TableScroll.svelte';

	let settings = $state<Setting[] | null>(null);
	let drafts = $state<Record<string, number>>({});
	let saving = $state<string | null>(null);
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);

	async function load() {
		error = null;
		const { data, error: err } = await api
			.GET('/v1/ops/settings')
			.catch(() => ({ data: undefined, error: undefined }));
		if (!data) {
			error = messageOf(err, 'The settings didn’t load.');
			return;
		}
		settings = data;
		drafts = Object.fromEntries(data.map((s) => [s.key, s.value ?? s.min]));
	}

	async function save(s: Setting) {
		error = notice = null;
		saving = s.key;
		const { data, error: err } = await api
			.PUT('/v1/ops/settings/{key}', {
				params: { path: { key: s.key } },
				body: { value: Number(drafts[s.key]) }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		saving = null;
		if (data) {
			notice = `Saved: ${data.label} = ${data.value}.`;
			await load();
		} else error = messageOf(err);
	}

	$effect(() => {
		if (staff.canSettle) void load();
	});
</script>

<svelte:head><title>Settings · VARA Ops</title></svelte:head>

{#if !staff.canSettle}
	<Notice tone="attention" title="Admins only">
		<p>These settings change how matching and RideCheck work. Ask an ops admin.</p>
	</Notice>
{:else}
	<h1>Settings</h1>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	{#if notice}<Notice tone="success"><p>{notice}</p></Notice>{/if}
	{#if settings}
		<TableScroll label="Settings">
			<table>
				<thead>
					<tr><th>Setting</th><th>Value</th><th>Allowed</th><th>Changed</th><th></th></tr>
				</thead>
				<tbody>
					{#each settings as s (s.key)}
						<tr>
							<td><label for={s.key}>{s.label}</label><br /><code>{s.key}</code></td>
							<td>
								<input
									id={s.key}
									type="number"
									min={s.min}
									max={s.max}
									bind:value={drafts[s.key]}
								/>
							</td>
							<td>{s.min} – {s.max}</td>
							<td>{s.updated_at ? formatWhen(s.updated_at) : 'Default'}</td>
							<td>
								<Button
									variant="secondary"
									loading={saving === s.key}
									disabled={drafts[s.key] === s.value}
									onclick={() => save(s)}>Save</Button
								>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</TableScroll>
	{/if}
{/if}

<style>
	h1 {
		margin-top: 0;
	}
	table {
		width: 100%;
		border-collapse: collapse;
		background: var(--color-surface);
	}
	th,
	td {
		text-align: left;
		padding: var(--space-2) var(--space-3);
		border-bottom: 1px solid var(--color-border);
		vertical-align: middle;
	}
	code {
		color: var(--color-text-muted);
		font-size: var(--text-xs);
	}
	input {
		width: 9rem;
		font: inherit;
		padding: var(--space-1) var(--space-2);
	}
</style>
