<!--
	Button (design brief §6). Variants: primary, secondary, ghost, sos.
	`size="driver"` gives the 56 px target required for driver primary actions.
-->
<script lang="ts">
	import type { HTMLButtonAttributes } from 'svelte/elements';

	interface Props extends HTMLButtonAttributes {
		variant?: 'primary' | 'secondary' | 'ghost' | 'sos';
		size?: 'default' | 'driver';
		loading?: boolean;
	}

	let {
		variant = 'primary',
		size = 'default',
		loading = false,
		type = 'button',
		disabled,
		children,
		...rest
	}: Props = $props();
</script>

<button
	{type}
	class="btn {variant} {size}"
	disabled={disabled || loading}
	aria-busy={loading || undefined}
	{...rest}
>
	{@render children?.()}
</button>

<style>
	.btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: var(--space-2);
		min-height: var(--touch-target);
		min-width: var(--touch-target);
		padding: 0 var(--space-5);
		border: 1px solid transparent;
		border-radius: var(--radius-pill);
		font: inherit;
		font-weight: 700;
		cursor: pointer;
		transition: background-color 120ms ease;
	}
	.driver {
		min-height: var(--touch-target-driver);
		font-size: var(--text-lg);
	}
	.primary {
		background: var(--button-primary-bg);
		color: var(--button-primary-fg);
	}
	.primary:active:not(:disabled) {
		background: var(--button-primary-bg-pressed);
	}
	.secondary {
		background: var(--color-surface);
		color: var(--color-brand);
		border-color: var(--color-border);
	}
	.ghost {
		background: transparent;
		color: var(--color-brand);
	}
	.sos {
		background: var(--button-sos-bg);
		color: var(--button-sos-fg);
		min-height: var(--touch-target-driver);
		letter-spacing: 0.04em;
	}
	.btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
</style>
