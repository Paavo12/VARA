<!-- Staff sign-in: email, password and the 6-digit code from the authenticator app. -->
<script lang="ts">
	import { Button, Notice, TextField } from '@fuber/ui';
	import { messageOf } from './format.ts';
	import { api, signedIn } from './staff.svelte.ts';

	let email = $state('');
	let password = $state('');
	let totp = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		error = null;
		busy = true;
		const { data, error: err } = await api
			.POST('/v1/ops/auth/login', { body: { email, password, totp: totp.replace(/\s/g, '') } })
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			password = '';
			totp = '';
			signedIn(data);
		} else {
			error = messageOf(err, 'We can’t reach the FUber API. Is it running?');
		}
	}
</script>

<form class="card" onsubmit={submit}>
	<h1>Staff sign-in</h1>
	<p class="lead">For FUber reviewers and the safety desk. Every action is recorded.</p>
	<TextField label="Work email" type="email" autocomplete="username" bind:value={email} required />
	<TextField
		label="Password"
		type="password"
		autocomplete="current-password"
		bind:value={password}
		required
	/>
	<TextField
		label="Authenticator code"
		inputmode="numeric"
		autocomplete="one-time-code"
		maxlength={7}
		bind:value={totp}
		required
	/>
	{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
	<Button type="submit" loading={busy}>Sign in</Button>
</form>

<style>
	.card {
		max-width: 400px;
		margin: var(--space-6) auto;
		display: grid;
		gap: var(--space-4);
		padding: var(--space-5);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
	}
	h1 {
		margin: 0;
		font-size: var(--text-xl);
	}
	.lead {
		margin: 0;
		color: var(--color-text-muted);
	}
</style>
