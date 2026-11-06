<!--
	Rider app: sign-in and onboarding (App Flow §2.1). Which screen shows is worked out from the
	account state on the server, so the flow resumes where it left off on any device.
	A verified rider lands on booking.
-->
<script lang="ts">
	import type { Me } from '@fuber/api-client';
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
	import Booking from '#lib/booking/Booking.svelte';
	import Verify from '#lib/Verify.svelte';
	import { riderSession } from '#lib/session.ts';

	type View =
		| 'loading'
		| 'offline'
		| 'signin'
		| 'details'
		| 'consents'
		| 'verify'
		| 'contacts'
		| 'status'
		| 'settings'
		| 'delete';

	let view = $state<View>('loading');
	let me = $state<Me | null>(null);
	let askedForContacts = $state(false);

	const session = riderSession(() => {
		me = null;
		view = 'signin';
	});
	provideSession(session);

	async function refresh() {
		const { data, response } = await session.api
			.GET('/v1/me')
			.catch(() => ({ data: undefined, response: undefined }));
		if (data) {
			me = data;
			view = next(data);
		} else if (response?.status === 401) {
			view = 'signin';
		} else {
			view = 'offline';
		}
	}

	function next(m: Me): View {
		if (!m.profile_complete) return 'details';
		if (m.pending_consents.length > 0) return 'consents';
		if (m.verification === 'not_started' || m.verification === 'rejected') return 'verify';
		if (m.emergency_contacts === 0 && !askedForContacts) return 'contacts';
		return 'status';
	}

	onMount(async () => {
		if (await session.restore()) await refresh();
		else view = 'signin';
	});

	const steps = $derived(
		me?.verification === 'approved' ? undefined : (n: number) => `Step ${n} of 4`
	);
</script>

<svelte:head>
	<title>VARA</title>
</svelte:head>

{#if view === 'loading'}
	<div class="splash" aria-label="Loading"><span>VARA</span></div>
{:else if view === 'offline'}
	<Screen title="No connection" lead="We couldn’t reach VARA. Check your internet connection.">
		{#snippet footer()}<Button onclick={refresh}>Try again</Button>{/snippet}
	</Screen>
{:else if view === 'signin'}
	<SignIn tagline="Rides by women, for women." onsignedin={refresh} />
{:else if view === 'details' && me}
	<Details {me} step={steps?.(1)} onsaved={refresh} />
{:else if view === 'consents' && me}
	<Consents
		documents={me.pending_consents}
		step={steps?.(2)}
		updated={me.verification === 'approved'}
		onaccepted={refresh}
	/>
{:else if view === 'verify' && me}
	<Verify step={steps?.(3)} rejectReason={me.verification_reject_reason} onsubmitted={refresh} />
{:else if view === 'contacts'}
	<EmergencyContacts
		step={steps?.(4)}
		skippable
		ondone={() => {
			askedForContacts = true;
			refresh();
		}}
	/>
{:else if view === 'settings' && me}
	<Screen title="Settings" onback={() => (view = 'status')}>
		<Button variant="secondary" onclick={() => (view = 'contacts')}
			>Emergency contacts ({me.emergency_contacts})</Button
		>
		<Button variant="secondary" onclick={() => session.signOut()}>Sign out</Button>
		<Button variant="ghost" onclick={() => (view = 'delete')}>Delete account</Button>
	</Screen>
{:else if view === 'delete'}
	<DeleteAccount onback={() => (view = 'settings')} ondeleted={() => (view = 'signin')} />
{:else if view === 'status' && me}
	{#if me.verification === 'approved'}
		<Booking {me} onsettings={() => (view = 'settings')} />
	{:else}
		<Screen
			title="We’re checking your details"
			lead="This usually takes less than a day. We’ll let you know when you’re verified."
		>
			<Notice tone="info"
				><p>Until then you can look around, but you can’t request a ride yet.</p></Notice
			>
			{#snippet footer()}
				<Button variant="secondary" onclick={refresh}>Check again</Button>
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
</style>
