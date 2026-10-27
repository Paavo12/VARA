<!--
	Driver app: sign-in and application (App Flow §2.2). The application hub shows what's done
	and what's missing; the driver submits once everything is in, then follows the review.
	Going online arrives in Phase 2.
-->
<script lang="ts">
	import type { DriverApplication, Me } from '@fuber/api-client';
	import {
		Consents,
		DeleteAccount,
		Details,
		EmergencyContacts,
		SignIn,
		provideSession
	} from '@fuber/account';
	import { Button, Notice, Screen } from '@fuber/ui';
	import { onMount } from 'svelte';
	import Documents from '#lib/Documents.svelte';
	import VehicleForm from '#lib/VehicleForm.svelte';
	import { driverSession } from '#lib/session.ts';

	type View =
		| 'loading'
		| 'offline'
		| 'signin'
		| 'details'
		| 'consents'
		| 'hub'
		| 'vehicle'
		| 'documents'
		| 'contacts'
		| 'settings'
		| 'delete';

	let view = $state<View>('loading');
	let me = $state<Me | null>(null);
	let app = $state<DriverApplication | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);

	const session = driverSession(() => {
		me = null;
		app = null;
		view = 'signin';
	});
	provideSession(session);

	async function refresh(go: View | null = null) {
		const [m, a] = await Promise.all([
			session.api.GET('/v1/me').catch(() => ({ data: undefined, response: undefined })),
			session.api
				.GET('/v1/driver/application')
				.catch(() => ({ data: undefined, response: undefined }))
		]);
		if (m.response?.status === 401) return void (view = 'signin');
		if (!m.data || !a.data) return void (view = 'offline');
		me = m.data;
		app = a.data;
		if (!me.profile_complete) view = 'details';
		else if (me.pending_consents.length > 0) view = 'consents';
		else view = go ?? 'hub';
	}

	async function submit() {
		error = null;
		busy = true;
		const { data, error: err } = await session.api
			.POST('/v1/driver/application/submit')
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data)
			return void (error =
				(err as { error?: { message?: string } } | undefined)?.error?.message ??
				'Please try again.');
		app = data;
	}

	onMount(async () => {
		if (await session.restore()) await refresh();
		else view = 'signin';
	});

	const editable = $derived(app?.status === 'not_started' || app?.status === 'rejected');
</script>

<svelte:head>
	<title>FUber Driver</title>
</svelte:head>

{#if view === 'loading'}
	<div class="splash" aria-label="Loading"><span>FUber Driver</span></div>
{:else if view === 'offline'}
	<Screen title="No connection" lead="We couldn’t reach FUber. Check your internet connection.">
		{#snippet footer()}<Button size="driver" onclick={() => refresh()}>Try again</Button>{/snippet}
	</Screen>
{:else if view === 'signin'}
	<SignIn tagline="Drive on your terms. Only women passengers." onsignedin={() => refresh()} />
{:else if view === 'details' && me}
	<Details {me} step="Step 1 of 4" onsaved={() => refresh()} />
{:else if view === 'consents' && me}
	<Consents
		documents={me.pending_consents}
		step="Step 2 of 4"
		updated={me.driver_application === 'approved'}
		onaccepted={() => refresh()}
	/>
{:else if view === 'vehicle' && app}
	<VehicleForm
		application={app}
		step="Step 3 of 4"
		onsaved={() => refresh()}
		onback={() => (view = 'hub')}
	/>
{:else if view === 'documents' && app}
	<Documents
		application={app}
		step="Step 4 of 4"
		onchanged={() => refresh('documents')}
		onback={() => refresh()}
	/>
{:else if view === 'contacts'}
	<EmergencyContacts ondone={() => refresh('settings')} onback={() => (view = 'settings')} />
{:else if view === 'delete'}
	<DeleteAccount onback={() => (view = 'settings')} ondeleted={() => (view = 'signin')} />
{:else if view === 'settings'}
	<Screen title="Settings" onback={() => (view = 'hub')}>
		<Button variant="secondary" onclick={() => (view = 'contacts')}>Emergency contacts</Button>
		<Button variant="secondary" onclick={() => session.signOut()}>Sign out</Button>
		<Button variant="ghost" onclick={() => (view = 'delete')}>Delete account</Button>
	</Screen>
{:else if view === 'hub' && app && me}
	{#if app.status === 'approved'}
		<Screen title="You’re approved, {me.display_name}" lead="Welcome to FUber.">
			<Notice tone="success">
				<p>Approved ride types: {app.approved_tiers.join(', ') || 'none yet'}.</p>
			</Notice>
			{#if !app.can_go_online}
				<Notice tone="attention" title="You can’t go online right now">
					<p>A document has expired or needs review. Check your documents.</p>
				</Notice>
			{/if}
			<Button variant="secondary" onclick={() => (view = 'documents')}>Documents</Button>
			{#snippet footer()}
				<Button size="driver" disabled>Go online (next update)</Button>
				<Button variant="ghost" onclick={() => (view = 'settings')}>Settings</Button>
			{/snippet}
		</Screen>
	{:else if app.status === 'pending'}
		<Screen
			title="Application in review"
			lead="We check every document and your car. This usually takes 2–3 working days."
		>
			<ul class="list">
				{#each app.documents as d (d.id)}<li>
						<span>{d.kind.replace('_', ' ')}</span><span>{d.status.replace('_', ' ')}</span>
					</li>{/each}
			</ul>
			{#snippet footer()}
				<Button size="driver" variant="secondary" onclick={() => refresh()}>Check again</Button>
				<Button variant="ghost" onclick={() => (view = 'settings')}>Settings</Button>
			{/snippet}
		</Screen>
	{:else}
		<Screen title="Apply to drive" lead="Finish each part, then send your application.">
			{#if app.status === 'rejected'}
				<Notice tone="attention" title="Your application needs changes">
					<p>{app.decision_reason ?? 'Please check the notes on your documents.'}</p>
				</Notice>
			{/if}
			<button class="part" onclick={() => (view = 'vehicle')} disabled={!editable}>
				<span>Car, ride types and payout</span>
				<span class="state">{app.vehicle && app.payout_account_saved ? '✓' : 'To do'}</span>
			</button>
			<button class="part" onclick={() => (view = 'documents')}>
				<span>Documents</span>
				<span class="state"
					>{app.missing_documents.length ? `${app.missing_documents.length} to add` : '✓'}</span
				>
			</button>
			{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
			{#snippet footer()}
				<Button size="driver" onclick={submit} disabled={!app?.ready_to_submit} loading={busy}
					>Send application</Button
				>
				<Button variant="ghost" onclick={() => (view = 'settings')}>Settings</Button>
			{/snippet}
		</Screen>
	{/if}
{/if}

<style>
	.splash {
		min-height: 100dvh;
		display: grid;
		place-content: center;
		font-size: var(--text-3xl);
		font-weight: 800;
		color: var(--color-brand);
	}
	.part {
		display: flex;
		justify-content: space-between;
		align-items: center;
		min-height: var(--touch-target-driver);
		padding: 0 var(--space-4);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		background: var(--color-surface);
		color: var(--color-text);
		font: inherit;
		font-weight: 600;
		cursor: pointer;
	}
	.part:disabled {
		opacity: 0.6;
		cursor: default;
	}
	.state {
		color: var(--color-text-muted);
	}
	.list {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: var(--space-2);
	}
	.list li {
		display: flex;
		justify-content: space-between;
		padding: var(--space-3) var(--space-4);
		border-radius: var(--radius-sm);
		background: var(--color-surface);
		text-transform: capitalize;
	}
</style>
