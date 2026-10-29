import { afterEach, describe, expect, it, vi } from 'vitest';
import { connectLive, wsUrl, type LiveMessage } from './live';

class FakeSocket {
	static OPEN = 1;
	static all: FakeSocket[] = [];
	readyState = 0;
	sent: string[] = [];
	onopen: (() => void) | null = null;
	onmessage: ((ev: { data: string }) => void) | null = null;
	onclose: (() => void) | null = null;
	onerror: (() => void) | null = null;
	constructor(public url: string) {
		FakeSocket.all.push(this);
	}
	send(s: string) {
		this.sent.push(s);
	}
	close() {
		this.readyState = 3;
		this.onclose?.();
	}
	open() {
		this.readyState = 1;
		this.onopen?.();
	}
	receive(m: object) {
		this.onmessage?.({ data: JSON.stringify(m) });
	}
}

const flush = () => new Promise((r) => setTimeout(r, 0));

afterEach(() => {
	FakeSocket.all = [];
	vi.useRealTimers();
});

describe('wsUrl', () => {
	it('maps the API base URL to the socket URL', () => {
		expect(wsUrl('http://127.0.0.1:8080')).toBe('ws://127.0.0.1:8080/v1/ws');
		expect(wsUrl('https://api.fuber.app/')).toBe('wss://api.fuber.app/v1/ws');
	});
});

describe('connectLive', () => {
	it('authenticates in the first message, never the URL, and delivers messages', async () => {
		const got: LiveMessage[] = [];
		let connects = 0;
		const live = connectLive({
			baseUrl: 'http://api.test',
			token: async () => 'secret-token',
			onmessage: (m) => got.push(m),
			onconnect: () => connects++,
			WebSocketImpl: FakeSocket as unknown as typeof WebSocket
		});
		await flush();
		const ws = FakeSocket.all[0];
		expect(ws.url).toBe('ws://api.test/v1/ws');
		expect(ws.url).not.toContain('secret');
		ws.open();
		expect(JSON.parse(ws.sent[0])).toEqual({ type: 'auth', data: { token: 'secret-token' } });
		expect(live.send('ping', {})).toBe(false); // not ready until the server says so
		ws.receive({ type: 'ready', data: {} });
		expect(connects).toBe(1);
		ws.receive({ type: 'ride.updated', data: { state: 'SEARCHING' } });
		ws.receive({ type: 'pong', data: {} });
		expect(got.map((m) => m.type)).toEqual(['ride.updated']);
		expect(live.send('location.update', { at: { lat: -22.5, lon: 17.0 } })).toBe(true);
		live.close();
	});

	it('reconnects after a drop and stops when the session is revoked', async () => {
		vi.useFakeTimers();
		let revoked = false;
		connectLive({
			baseUrl: 'http://api.test',
			token: async () => 't',
			onmessage: () => {},
			onrevoked: () => (revoked = true),
			WebSocketImpl: FakeSocket as unknown as typeof WebSocket
		});
		await vi.advanceTimersByTimeAsync(0);
		FakeSocket.all[0].close();
		await vi.advanceTimersByTimeAsync(1000);
		expect(FakeSocket.all.length).toBe(2);
		const ws = FakeSocket.all[1];
		ws.open();
		ws.receive({ type: 'session.revoked', data: {} });
		expect(revoked).toBe(true);
		ws.close();
		await vi.advanceTimersByTimeAsync(60_000);
		expect(FakeSocket.all.length).toBe(2);
	});
});
