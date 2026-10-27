<!--
	Status message box. Tone sets the accent stripe; the background stays a neutral tinted
	surface so text contrast holds in light and dark themes.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		tone?: 'info' | 'success' | 'attention' | 'danger';
		title?: string;
		children?: Snippet;
	}

	let { tone = 'info', title, children }: Props = $props();
</script>

<div class="notice {tone}" role={tone === 'danger' ? 'alert' : 'status'}>
	{#if title}<p class="title">{title}</p>{/if}
	{@render children?.()}
</div>

<style>
	.notice {
		padding: var(--space-3) var(--space-4);
		border-left: 4px solid var(--stripe);
		border-radius: var(--radius-sm);
		background: var(--color-surface-tinted);
		color: var(--color-text);
	}
	.notice :global(p) {
		margin: 0;
	}
	.title {
		font-weight: 700;
		margin-bottom: var(--space-1) !important;
	}
	.info {
		--stripe: var(--color-info);
	}
	.success {
		--stripe: var(--color-success);
	}
	.attention {
		--stripe: var(--color-attention);
	}
	.danger {
		--stripe: var(--color-sos);
	}
</style>
