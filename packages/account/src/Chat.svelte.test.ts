// @vitest-environment jsdom
import type { ChatMessage, Session } from '@fuber/api-client';
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ChatHarness from './test/ChatHarness.svelte';

type Thread = { messages: ChatMessage[]; can_send: boolean; quick_replies: [] };

const msg = (
	id: number,
	sender: 'rider' | 'driver',
	text: string,
	ride_id = 'r1'
): ChatMessage => ({
	id,
	ride_id,
	sender,
	text,
	quick_reply: null,
	sent_at: '2026-11-10T12:00:00Z'
});

const thread = (...messages: ChatMessage[]): { data: Thread } => ({
	data: { messages, can_send: true, quick_replies: [] }
});

/** A session whose API answers GET and POST with whatever the test hands it. */
function fakeSession(get: () => Promise<unknown>, post?: () => Promise<unknown>) {
	return {
		api: {
			GET: vi.fn(get),
			POST: vi.fn(post ?? (() => Promise.resolve({ data: undefined, error: undefined })))
		}
	} as unknown as Session;
}

// jsdom has no layout, so no scrolling either.
Element.prototype.scrollTo = () => undefined;

let mounted: ReturnType<typeof mount> | null = null;
afterEach(() => {
	if (mounted) unmount(mounted);
	mounted = null;
	document.body.innerHTML = '';
});

function render(session: Session, props: { rideId: string }) {
	const app = mount(ChatHarness, { target: document.body, props: { session, ...props } });
	mounted = app;
	return app as unknown as {
		component(): { refresh(): Promise<void>; receive(m: ChatMessage): void };
	};
}

const label = () => document.querySelector('.fab')?.getAttribute('aria-label');

describe('Chat', () => {
	it('shows only what the server has after a refetch (another driver took over)', async () => {
		const answers = [thread(msg(1, 'driver', 'From the first driver')), thread()];
		const app = render(
			fakeSession(async () => answers.shift()),
			{ rideId: 'r1' }
		);
		await vi.waitFor(() => expect(label()).toBe('Message Hilma, 1 unread'));
		await app.component().refresh();
		flushSync();
		expect(label()).toBe('Message Hilma, 0 unread');
	});

	it('keeps a message pushed while the refetch was on its way', async () => {
		let answer!: (v: unknown) => void;
		const app = render(
			fakeSession(() => new Promise((r) => (answer = r))),
			{ rideId: 'r1' }
		);
		await tick();
		app.component().receive(msg(7, 'driver', 'I’m outside'));
		answer(thread());
		await vi.waitFor(() => expect(label()).toBe('Message Hilma, 1 unread'));
	});

	it('starts empty for another ride', async () => {
		const props = $state({ rideId: 'r1' });
		const answers = [thread(msg(1, 'driver', 'Old ride')), thread()];
		mounted = mount(ChatHarness, {
			target: document.body,
			props: {
				session: fakeSession(async () => answers.shift()),
				get rideId() {
					return props.rideId;
				}
			}
		});
		await vi.waitFor(() => expect(label()).toBe('Message Hilma, 1 unread'));
		props.rideId = 'r2';
		flushSync();
		expect(label()).toBe('Message Hilma, 0 unread');
	});

	it('closes the composer when the server says the chat has closed', async () => {
		const closed = {
			data: undefined,
			error: { error: { code: 'chat_closed', message: 'Closed.' } }
		};
		render(
			fakeSession(
				async () => thread(),
				async () => closed
			),
			{ rideId: 'r1' }
		);
		await tick();
		(document.querySelector('.fab') as HTMLButtonElement).click();
		flushSync();
		const box = document.querySelector('textarea')!;
		box.value = 'Hello';
		box.dispatchEvent(new Event('input'));
		flushSync();
		document.querySelector('form')!.requestSubmit();
		await vi.waitFor(() =>
			expect(document.querySelector('.closed')?.textContent).toContain('chat has closed')
		);
		expect(document.querySelector('textarea')).toBeNull();
	});
});
