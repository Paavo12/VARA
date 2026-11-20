<!--
	Ops console frame: top bar with the signed-in staff member, and the staff sign-in form until
	someone has signed in (password + authenticator code). After a reload the session cookie is
	checked first, so a signed-in agent goes straight back to work.
-->
<script lang="ts">
	import '@fuber/ui/tokens.css';
	import favicon from '#lib/assets/favicon.svg';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { restoreSession, signOut, staff } from '#lib/staff.svelte.ts';
	import SignIn from '#lib/SignIn.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	void restoreSession();

	/** Marks the link of the page you're on, for screen readers and visually (WCAG 2.4.8, 1.3.1). */
	const here = (path: string) =>
		page.url.pathname === path || page.url.pathname.startsWith(`${path}/`) ? 'page' : undefined;
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<meta name="robots" content="noindex, nofollow" />
</svelte:head>

<a class="skip" href="#main">Skip to content</a>
<header>
	<a class="brand" href={resolve('/')}>VARA Ops</a>
	{#if staff.session}
		<nav aria-label="Ops console">
			{#if staff.canHandleIncidents}<a
					class="link"
					href={resolve('/incidents')}
					aria-current={here('/incidents')}>Safety desk</a
				>{/if}
			<a class="link" href={resolve('/metrics')} aria-current={here('/metrics')}>Numbers</a>
			{#if staff.canSupport}<a class="link" href={resolve('/users')} aria-current={here('/users')}
					>Users</a
				>
				<a class="link" href={resolve('/reports')} aria-current={here('/reports')}>Reports</a>{/if}
			{#if staff.canSettle}<a
					class="link"
					href={resolve('/settlements')}
					aria-current={here('/settlements')}>Settlements</a
				>
				<a class="link" href={resolve('/settings')} aria-current={here('/settings')}>Settings</a>
				<a class="link" href={resolve('/pricing')} aria-current={here('/pricing')}>Pricing</a>
				<a class="link" href={resolve('/audit')} aria-current={here('/audit')}>Audit log</a>{/if}
		</nav>
		<span class="who">{staff.session.roles.join(', ').replaceAll('ops_', '')}</span>
		<button type="button" class="link" onclick={signOut}>Sign out</button>
	{/if}
</header>

<main id="main" tabindex="-1">
	{#if staff.session}
		{@render children()}
	{:else if staff.checked}
		<SignIn />
	{:else}
		<p class="muted" role="status">Loading…</p>
	{/if}
</main>

<style>
	:global(body) {
		margin: 0;
		background: var(--color-bg);
		color: var(--color-text);
		font-family: var(--font-sans);
	}
	.muted {
		color: var(--color-text-muted);
	}
	.skip {
		position: absolute;
		left: var(--space-2);
		top: -100px;
		padding: var(--space-2) var(--space-3);
		background: var(--color-surface);
		color: var(--color-brand);
		font-weight: 700;
		z-index: 10;
	}
	.skip:focus {
		top: var(--space-2);
	}
	header {
		display: flex;
		flex-wrap: wrap; /* reflows at 320 px wide / 400 % zoom (WCAG 1.4.10) */
		align-items: center;
		gap: var(--space-2) var(--space-4);
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
	nav {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2) var(--space-4);
	}
	.link {
		background: none;
		border: 0;
		padding: var(--space-1);
		font: inherit;
		color: var(--color-brand);
		cursor: pointer;
	}
	.link[aria-current='page'] {
		font-weight: 800;
		text-decoration: underline;
		text-underline-offset: 4px;
	}
	main:focus {
		outline: none;
	}
	main {
		max-width: 1100px;
		margin: 0 auto;
		padding: var(--space-5) var(--space-4) var(--space-6);
	}
</style>
