<!--
	Labelled text input (design brief §6). The label is always visible (no placeholder-only
	fields); `hint` and `error` are announced with the input via aria-describedby.
-->
<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';

	interface Props extends HTMLInputAttributes {
		label: string;
		value?: string;
		hint?: string;
		error?: string | null;
	}

	let { label, value = $bindable(''), hint, error, id, ...rest }: Props = $props();
	const fallbackId = `f-${Math.random().toString(36).slice(2, 9)}`;
	const inputId = $derived(id ?? fallbackId);
</script>

<div class="field">
	<label for={inputId}>{label}</label>
	<input
		id={inputId}
		bind:value
		aria-invalid={error ? true : undefined}
		aria-describedby={error || hint ? `${inputId}-note` : undefined}
		{...rest}
	/>
	{#if error}
		<p class="note error" id="{inputId}-note" role="alert">{error}</p>
	{:else if hint}
		<p class="note" id="{inputId}-note">{hint}</p>
	{/if}
</div>

<style>
	.field {
		display: grid;
		gap: var(--space-1);
	}
	label {
		font-weight: 600;
		font-size: var(--text-sm);
		color: var(--color-text-secondary);
	}
	input {
		min-height: var(--touch-target);
		padding: 0 var(--space-3);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-sm);
		background: var(--color-surface);
		color: var(--color-text);
		font: inherit;
	}
	input[aria-invalid='true'] {
		border-color: var(--color-sos);
	}
	.note {
		margin: 0;
		font-size: var(--text-sm);
		color: var(--color-text-muted);
	}
	.error {
		color: var(--color-sos);
	}
</style>
