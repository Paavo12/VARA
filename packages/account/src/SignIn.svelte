<!--
	Phone + SMS code sign-in (App Flow §2.1, PRD A-1). Two steps on one screen: number, then
	the 6-digit code with a resend countdown. Calls `onsignedin` when the session is ready.
	Locally there is no SMS gateway, so the API returns the code and sign-in skips that step.
-->
<script lang="ts">
	import { Button, Notice, Screen, TextField, formatPhone } from '@fuber/ui';
	import { messageOf, useSession } from './context';
	import { normalisePhone } from './phone';

	interface Props {
		/** Shown above the form, e.g. "Rides by women, for women." */
		tagline: string;
		onsignedin: (newUser: boolean) => void;
	}

	let { tagline, onsignedin }: Props = $props();
	const session = useSession();

	let phoneInput = $state('');
	let phone = $state<string | null>(null);
	let challenge = $state<string | null>(null);
	let code = $state('');
	let error = $state<string | null>(null);
	let busy = $state(false);
	let resendAt = $state(0);
	let now = $state(Date.now());

	$effect(() => {
		const t = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(t);
	});
	const resendIn = $derived(Math.max(0, Math.ceil((resendAt - now) / 1000)));

	async function sendCode(e?: Event) {
		e?.preventDefault();
		error = null;
		const normalised = normalisePhone(phoneInput);
		if (!normalised) {
			error = 'Enter a Namibian mobile number, e.g. 081 123 4567.';
			return;
		}
		busy = true;
		const {
			data,
			error: err,
			response
		} = await session.requestCode(normalised).catch(() => ({
			data: undefined,
			error: undefined,
			response: undefined
		}));
		busy = false;
		if (!data) {
			const retry = Number(response?.headers.get('retry-after') ?? 0);
			error = retry
				? `Please wait ${retry} seconds before asking for another code.`
				: messageOf(err, 'We couldn’t send the code. Check your connection and try again.');
			return;
		}
		phone = normalised;
		challenge = data.challenge_id;
		resendAt = Date.now() + data.resend_after_s * 1000;
		code = data.dev_code ?? '';
		if (data.dev_code) await verify();
	}

	async function verify(e?: Event) {
		e?.preventDefault();
		if (!phone || !challenge) return;
		error = null;
		if (!/^\d{6}$/.test(code.trim())) {
			error = 'The code has 6 digits.';
			return;
		}
		busy = true;
		const res = await session.verifyCode(challenge, phone, code.trim()).catch(() => undefined);
		busy = false;
		if (!res?.data) {
			const e = res?.error as { error?: { code?: string } } | undefined;
			if (e?.error?.code === 'otp_locked') challenge = null;
			error = messageOf(
				res?.error,
				'We couldn’t check the code. Check your connection and try again.'
			);
			return;
		}
		onsignedin(res.data.new_user);
	}
</script>

{#if !challenge || !phone}
	<Screen title="Welcome to VARA" lead={tagline}>
		<form id="phone-form" onsubmit={sendCode} novalidate>
			<TextField
				label="Mobile number"
				type="tel"
				inputmode="tel"
				autocomplete="tel"
				placeholder="081 123 4567"
				bind:value={phoneInput}
				{error}
				hint="Namibian numbers only."
			/>
		</form>
		<p class="small">VARA is for women only. Every rider and driver is verified.</p>
		{#snippet footer()}
			<Button type="submit" form="phone-form" loading={busy}>Continue</Button>
		{/snippet}
	</Screen>
{:else}
	<Screen
		title="Enter your code"
		lead="We sent a 6-digit code to {formatPhone(phone)}."
		onback={() => {
			challenge = null;
			error = null;
		}}
	>
		<form id="code-form" onsubmit={verify} novalidate>
			<TextField
				label="Code"
				inputmode="numeric"
				autocomplete="one-time-code"
				maxlength={6}
				bind:value={code}
				{error}
			/>
		</form>
		{#if resendIn > 0}
			<p class="small">You can ask for a new code in {resendIn} s.</p>
		{:else}
			<Button variant="ghost" onclick={() => sendCode()} disabled={busy}>Send a new code</Button>
		{/if}
		<Notice tone="info"><p>VARA will never call you to ask for this code.</p></Notice>
		{#snippet footer()}
			<Button type="submit" form="code-form" loading={busy}>Continue</Button>
		{/snippet}
	</Screen>
{/if}

<style>
	form {
		display: grid;
		gap: var(--space-4);
	}
	.small {
		margin: 0;
		font-size: var(--text-sm);
		color: var(--color-text-muted);
	}
</style>
