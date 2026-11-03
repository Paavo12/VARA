<!--
	Approve / reject buttons. Rejecting opens a reason field first: the applicant sees the reason,
	so it must tell her what to fix (the API refuses a rejection without one).
-->
<script lang="ts">
	import { Button, TextField } from '@fuber/ui';

	interface Props {
		/** What is being decided, for the buttons' accessible names ("the licence"). */
		subject: string;
		approveLabel?: string;
		disabled?: boolean;
		ondecide: (decision: 'approve' | 'reject', reason?: string) => Promise<void>;
	}

	let { subject, approveLabel = 'Approve', disabled = false, ondecide }: Props = $props();
	let rejecting = $state(false);
	let reason = $state('');
	let busy = $state<'approve' | 'reject' | null>(null);

	async function decide(decision: 'approve' | 'reject') {
		busy = decision;
		await ondecide(decision, decision === 'reject' ? reason.trim() : undefined);
		busy = null;
		rejecting = false;
		reason = '';
	}
</script>

{#if rejecting}
	<div class="reject">
		<TextField
			label="Reason the applicant will see"
			placeholder="The photo is blurred. Please retake it in good light."
			maxlength={300}
			bind:value={reason}
		/>
		<div class="row">
			<Button
				variant="secondary"
				disabled={disabled || !reason.trim()}
				loading={busy === 'reject'}
				onclick={() => decide('reject')}
				aria-label="Reject {subject}">Reject</Button
			>
			<Button variant="ghost" onclick={() => (rejecting = false)}>Back</Button>
		</div>
	</div>
{:else}
	<div class="row">
		<Button
			{disabled}
			loading={busy === 'approve'}
			onclick={() => decide('approve')}
			aria-label="{approveLabel} {subject}">{approveLabel}</Button
		>
		<Button
			variant="ghost"
			{disabled}
			onclick={() => (rejecting = true)}
			aria-label="Reject {subject}">Reject…</Button
		>
	</div>
{/if}

<style>
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
	}
	.reject {
		display: grid;
		gap: var(--space-2);
	}
</style>
