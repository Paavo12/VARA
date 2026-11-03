<!--
	Ops console frame: top bar with the signed-in staff member, and the staff sign-in form until
	someone has signed in (password + authenticator code).
-->
<script lang="ts">
	import '@fuber/ui/tokens.css';
	import favicon from '#lib/assets/favicon.svg';
	import { resolve } from '$app/paths';
	import { signOut, staff } from '#lib/staff.svelte.ts';
	import SignIn from '#lib/SignIn.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<meta name="robots" content="noindex, nofollow" />
</svelte:head>

<header>
	<a class="brand" href={resolve('/')}>FUber Ops</a>
	{#if staff.session}
		<span class="who">{staff.session.roles.join(', ').replaceAll('ops_', '')}</span>
		<button type="button" class="link" onclick={signOut}>Sign out</button>
	{/if}
</header>

<main>
	{#if staff.session}
		{@render children()}
	{:else}
		<SignIn />
	{/if}
</main>

<style>
	:global(body) {
		margin: 0;
		background: var(--color-bg);
		color: var(--color-text);
		font-family: var(--font-sans);
	}
	header {
		display: flex;
		align-items: center;
		gap: var(--space-4);
		padding: var(--space-3) max(var(--space-4), env(safe-area-inset-left, 0px));
		background: var(--color-surface);
		border-bottom: 1px solid var(--color-border);
	}
	.brand {
		font-weight: 800;
		color: var(--color-brand);
		text-decoration: none;
		margin-right: auto;
	}
	.who {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.link {
		background: none;
		border: 0;
		padding: var(--space-1);
		font: inherit;
		color: var(--color-brand);
		cursor: pointer;
	}
	main {
		max-width: 1100px;
		margin: 0 auto;
		padding: var(--space-5) var(--space-4) var(--space-6);
	}
</style>
