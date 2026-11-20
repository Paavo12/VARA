<!--
	A full-height onboarding/settings screen: heading, optional step counter and lead text,
	content, and a footer pinned to the bottom for the primary action.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		title: string;
		lead?: string;
		step?: string;
		onback?: () => void;
		children?: Snippet;
		footer?: Snippet;
	}

	let { title, lead, step, onback, children, footer }: Props = $props();

	// The apps swap screens without a page load, so tell screen readers where they are (WCAG 2.4.3,
	// 4.1.3). If the control that had focus went away with the old screen, focus the new heading.
	// If focus is still somewhere (typing in the chat, or the same screen with a new title such as
	// "Your driver has arrived"), leave it there and announce the title instead.
	let heading: HTMLHeadingElement | undefined = $state();
	let announced = $state('');
	let mounted = false;
	$effect(() => {
		const now = title;
		const lost = !document.activeElement || document.activeElement === document.body;
		if (lost) heading?.focus();
		else if (mounted) announced = now;
		mounted = true;
	});
</script>

<main class="screen">
	<p class="sr-only" aria-live="polite">{announced}</p>
	<header>
		{#if onback}
			<button class="back" type="button" onclick={onback} aria-label="Back">←</button>
		{/if}
		{#if step}<p class="step">{step}</p>{/if}
		<h1 bind:this={heading} tabindex="-1">{title}</h1>
		{#if lead}<p class="lead">{lead}</p>{/if}
	</header>
	<div class="body">{@render children?.()}</div>
	{#if footer}<footer>{@render footer()}</footer>{/if}
</main>

<style>
	.screen {
		min-height: 100dvh;
		max-width: 480px;
		margin: 0 auto;
		display: flex;
		flex-direction: column;
		gap: var(--space-5);
		padding: max(var(--space-5), env(safe-area-inset-top)) var(--space-4)
			max(var(--space-4), env(safe-area-inset-bottom));
	}
	header {
		display: grid;
		gap: var(--space-2);
	}
	.back {
		justify-self: start;
		min-width: var(--touch-target);
		min-height: var(--touch-target);
		border: none;
		background: none;
		color: var(--color-brand);
		font-size: var(--text-xl);
		cursor: pointer;
	}
	.step {
		margin: 0;
		font-size: var(--text-sm);
		font-weight: 600;
		color: var(--color-text-muted);
	}
	h1 {
		margin: 0;
		font-size: var(--text-2xl);
		line-height: 1.2;
	}
	/* Focused by the script for screen readers only; it isn't a control, so no ring. */
	h1:focus {
		outline: none;
	}
	.lead {
		margin: 0;
		color: var(--color-text-secondary);
	}
	.body {
		flex: 1;
		display: grid;
		align-content: start;
		gap: var(--space-4);
	}
	footer {
		display: grid;
		gap: var(--space-2);
	}
</style>
