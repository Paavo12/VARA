<!--
	Chat with the other person on the trip (PRD T-8, design brief §5.1 item 11): a button under
	SOS with the unread count, opening a full screen with quick replies. Neither side sees a phone
	number. The screen that holds the socket passes pushed messages to `receive` and calls `refresh`
	after every reconnect. SOS stays above this screen, so it is never out of reach (also for screen readers).
-->
<script lang="ts">
	import type { ChatMessage, components } from '@fuber/api-client';
	import { Button, Notice } from '@fuber/ui';
	import { tick, untrack } from 'svelte';
	import { messageOf, useSession } from './context';

	type QuickReply = components['schemas']['QuickReplyOption'];

	interface Props {
		rideId: string;
		/** Which side this app is. */
		party: 'rider' | 'driver';
		/** The other person's first name. */
		name: string;
	}

	let { rideId, party, name }: Props = $props();
	const session = useSession();

	let messages = $state<ChatMessage[]>([]);
	let quickReplies = $state<QuickReply[]>([]);
	let canSend = $state(true);
	let open = $state(false);
	let draft = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);
	let seenId = $state(0);
	/** Read out by screen readers: only the newest message from the other person. */
	let announcement = $state('');
	let list = $state<HTMLElement>();
	let fab = $state<HTMLButtonElement>();
	let input = $state<HTMLTextAreaElement>();
	let closeButton = $state<HTMLButtonElement>();

	/** Trying a failed message again soon after reuses its key, so it can't arrive twice. */
	const RETRY_WINDOW_MS = 60_000;
	let retry: { content: string; key: string; at: number } | null = null;
	/** Messages that arrived while a refetch was under way: they may be newer than its answer. */
	const arrivedDuring = new Set<ChatMessage[]>();

	const unread = $derived(messages.filter((m) => m.sender !== party && m.id > seenId).length);

	function merge(incoming: ChatMessage[]) {
		const byId = new Map(messages.map((m) => [m.id, m]));
		for (const m of incoming) byId.set(m.id, m);
		messages = [...byId.values()].sort((a, b) => a.id - b.id);
		if (open) markSeen();
		void scrollToEnd();
	}

	/** One new message: pushed, or the answer to sending. */
	function add(m: ChatMessage) {
		for (const batch of arrivedDuring) batch.push(m);
		const isNew = !messages.some((x) => x.id === m.id);
		merge([m]);
		if (!isNew) return;
		if (m.sender !== party) {
			announcement = open ? `${name}: ${m.text}` : `New message from ${name}`;
		} else if (retry?.content === m.text || retry?.content === `quick:${m.quick_reply}`) {
			// The "didn't send" message arrived after all (its answer was lost on the way back).
			retry = null;
			error = null;
		}
	}

	function markSeen() {
		seenId = messages.at(-1)?.id ?? seenId;
	}

	async function scrollToEnd() {
		await tick();
		list?.scrollTo({ top: list.scrollHeight });
	}

	/** A message the socket pushed. */
	export function receive(m: ChatMessage) {
		if (m.ride_id === rideId) add(m);
	}

	/**
	 * Refetches the conversation (on opening, and after every reconnect). The server's copy
	 * replaces what is on screen: after a missed update it may belong to another driver.
	 */
	export async function refresh() {
		const ride = rideId;
		const arrived: ChatMessage[] = [];
		arrivedDuring.add(arrived);
		const { data } = await session.api
			.GET('/v1/rides/{id}/messages', { params: { path: { id: ride } } })
			.catch(() => ({ data: undefined }));
		arrivedDuring.delete(arrived);
		if (!data || ride !== rideId) return;
		quickReplies = data.quick_replies;
		canSend = data.can_send;
		messages = [];
		merge([...data.messages, ...arrived]);
	}

	async function send(body: { text: string } | { quick_reply: string }, content: string) {
		error = null;
		busy = true;
		const now = Date.now();
		const key =
			retry && retry.content === content && now - retry.at < RETRY_WINDOW_MS
				? retry.key
				: crypto.randomUUID();
		const { data, error: err } = await session.api
			.POST('/v1/rides/{id}/messages', {
				params: { path: { id: rideId }, header: { 'Idempotency-Key': key } },
				body: { text: null, quick_reply: null, ...body }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (data) {
			retry = null;
			add(data);
			return true;
		}
		if (err?.error.code === 'chat_closed') {
			retry = null;
			canSend = false;
			return false;
		}
		retry = { content, key, at: now };
		error = messageOf(err, 'That didn’t send. Check your connection and try again.');
		return false;
	}

	async function sendDraft(event: SubmitEvent) {
		event.preventDefault();
		const text = draft.trim();
		// Keep anything she typed while it was sending.
		if (text && (await send({ text }, text)) && draft.trim() === text) draft = '';
	}

	async function show() {
		open = true;
		markSeen();
		void scrollToEnd();
		void refresh();
		await tick();
		(input ?? closeButton)?.focus();
	}

	async function hide() {
		open = false;
		await tick();
		fab?.focus();
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			void hide();
		}
	}

	function clock(iso: string) {
		return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
	}

	$effect(() => {
		// Another ride is another conversation: start empty. Rerun only when `rideId` changes.
		void rideId;
		untrack(() => {
			messages = [];
			seenId = 0;
			retry = null;
			error = null;
			draft = '';
			canSend = true;
			announcement = '';
			void refresh();
		});
	});
</script>

<!-- Outside the #if so it exists before anything is announced. -->
<p class="sr-only" aria-live="polite">{announcement}</p>

{#if !open}
	<button
		class="fab"
		type="button"
		bind:this={fab}
		onclick={show}
		aria-label={`Message ${name}, ${unread} unread`}
	>
		<svg viewBox="0 0 24 24" width="26" height="26" aria-hidden="true">
			<path
				d="M4 4h16a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H9l-5 4v-4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"
				fill="currentColor"
			/>
		</svg>
		{#if unread > 0}<span class="badge" aria-hidden="true">{unread}</span>{/if}
	</button>
{:else}
	<!-- Not aria-modal: that would hide the SOS button above it from screen readers. -->
	<div class="sheet" role="dialog" aria-labelledby="chat-title" tabindex="-1" {onkeydown}>
		<header>
			<button class="back" type="button" bind:this={closeButton} onclick={hide}>Close</button>
			<h2 id="chat-title">Message {name}</h2>
		</header>

		<div class="list" bind:this={list}>
			{#each messages as m (m.id)}
				<div class="bubble" class:mine={m.sender === party}>
					<p>{m.text}</p>
					<time datetime={m.sent_at}>{clock(m.sent_at)}</time>
				</div>
			{:else}
				<p class="empty">Your phone numbers stay hidden from each other.</p>
			{/each}
		</div>

		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}

		{#if canSend}
			<div class="quick">
				{#each quickReplies as q (q.id)}
					<button
						type="button"
						disabled={busy}
						onclick={() => send({ quick_reply: q.id }, `quick:${q.id}`)}>{q.text}</button
					>
				{/each}
			</div>
			<form onsubmit={sendDraft}>
				<textarea
					bind:this={input}
					bind:value={draft}
					rows="1"
					maxlength="500"
					placeholder="Type a message"
					aria-label="Message"></textarea>
				<Button type="submit" loading={busy} disabled={!draft.trim()}>Send</Button>
			</form>
		{:else}
			<p class="closed">This trip’s chat has closed.</p>
		{/if}
		<p class="note">
			Kept for 90 days. VARA’s safety team can read it if a safety issue is raised.
		</p>
	</div>
{/if}

<style>
	.sr-only {
		position: absolute;
		width: 1px;
		height: 1px;
		margin: -1px;
		padding: 0;
		overflow: hidden;
		clip-path: inset(50%);
		white-space: nowrap;
		border: 0;
	}
	.fab {
		position: fixed;
		top: calc(env(safe-area-inset-top, 0px) + var(--space-3) + 56px + var(--space-3));
		right: var(--space-3);
		z-index: 30;
		display: grid;
		place-items: center;
		width: 56px;
		height: 56px;
		border: 0;
		border-radius: 50%;
		background: var(--button-primary-bg);
		color: var(--button-primary-fg);
		box-shadow: 0 4px 12px rgb(0 0 0 / 0.25);
		cursor: pointer;
	}
	.fab:focus-visible,
	.quick button:focus-visible,
	.back:focus-visible {
		outline: 3px solid var(--color-focus-ring);
		outline-offset: 2px;
	}
	.badge {
		position: absolute;
		top: -2px;
		right: -2px;
		min-width: 22px;
		height: 22px;
		padding: 0 6px;
		border-radius: var(--radius-pill);
		background: var(--button-sos-bg);
		color: var(--button-sos-fg);
		font-size: var(--text-sm);
		font-weight: 700;
		line-height: 22px;
		text-align: center;
	}
	/* Under the SOS button (z-index 50), which must stay reachable from here. */
	.sheet {
		position: fixed;
		inset: 0;
		z-index: 40;
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		padding: calc(env(safe-area-inset-top, 0px) + var(--space-3)) var(--space-4)
			calc(env(safe-area-inset-bottom, 0px) + var(--space-3));
		background: var(--color-bg);
		color: var(--color-text);
	}
	header {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		min-height: 56px;
		/* Room for SOS, top right. */
		padding-right: 64px;
	}
	h2 {
		margin: 0;
		font-size: var(--text-lg);
		overflow-wrap: anywhere;
	}
	.back {
		min-height: 44px;
		padding: 0 var(--space-2);
		border: 0;
		background: none;
		color: var(--color-brand);
		font: inherit;
		font-weight: 700;
		cursor: pointer;
	}
	.list {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
	}
	.bubble {
		align-self: flex-start;
		max-width: 80%;
		padding: var(--space-2) var(--space-3);
		border-radius: var(--radius-md);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
	}
	.bubble.mine {
		align-self: flex-end;
		background: var(--color-surface-tinted);
	}
	.bubble p {
		margin: 0;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	time {
		display: block;
		margin-top: 2px;
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	.empty,
	.closed,
	.note {
		margin: 0;
		color: var(--color-text-muted);
		font-size: var(--text-sm);
		text-align: center;
	}
	.empty {
		margin: auto 0;
	}
	.quick {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
	}
	.quick button {
		min-height: 44px;
		padding: 0 var(--space-3);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-pill);
		background: var(--color-surface);
		color: var(--color-text);
		font: inherit;
		cursor: pointer;
	}
	.quick button:disabled {
		opacity: 0.6;
	}
	form {
		display: flex;
		align-items: flex-end;
		gap: var(--space-2);
	}
	textarea {
		flex: 1;
		min-width: 0;
		min-height: 48px;
		max-height: 120px;
		padding: var(--space-3);
		border: 1px solid var(--color-border-control);
		border-radius: var(--radius-md);
		background: var(--color-surface);
		color: var(--color-text);
		/* 16px or more, so phones don't zoom in on focus. */
		font: inherit;
		font-size: max(1rem, 16px);
		resize: none;
	}
</style>
