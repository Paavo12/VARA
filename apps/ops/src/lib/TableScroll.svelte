<!--
	A wide table that scrolls sideways on its own, so a narrow window or 400 % zoom never makes
	the whole page scroll (WCAG 1.4.10). Focusable and named, so keyboard users can scroll it
	with the arrow keys and screen readers announce it.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		label: string;
		children: Snippet;
	}

	let { label, children }: Props = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="table-scroll" role="region" aria-label={label} tabindex="0">
	{@render children()}
</div>

<style>
	.table-scroll {
		max-width: 100%;
		overflow-x: auto;
	}
</style>
