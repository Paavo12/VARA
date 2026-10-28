/**
 * Live updates over the API's WebSocket (TRD §5): one socket per signed-in app, authenticated
 * by the first message (never the URL), reconnecting with exponential back-off.
 *
 * The socket is a hint channel: after every (re)connect the app must refetch its state
 * (`GET /v1/rides/active` or `GET /v1/driver/status`), so `onconnect` runs then.
 */

export interface LiveMessage {
	type: string;
	id?: string;
	data: unknown;
}

export interface LiveOptions {
	/** The API base URL, e.g. `http://127.0.0.1:8080`; `http` becomes `ws`. */
	baseUrl: string;
	/** A current access token (the session refreshes it when needed). */
	token: () => Promise<string | null>;
	onmessage: (m: LiveMessage) => void;
	/** Connected and authenticated: refetch state now. */
	onconnect?: () => void;
	/** The server ended the session (signed out elsewhere, suspended). */
	onrevoked?: () => void;
	WebSocketImpl?: typeof WebSocket;
}

export interface Live {
	/** Sends a message if connected; returns whether it was sent. */
	send(type: string, data: unknown): boolean;
	readonly connected: boolean;
	close(): void;
}

const MAX_BACKOFF_MS = 30_000;
const PING_MS = 20_000;

export function wsUrl(baseUrl: string): string {
	return baseUrl.replace(/^http/, 'ws').replace(/\/$/, '') + '/v1/ws';
}

export function connectLive(opts: LiveOptions): Live {
	const WS = opts.WebSocketImpl ?? WebSocket;
	let socket: WebSocket | null = null;
	let ready = false;
	let closed = false;
	let attempt = 0;
	let retryTimer: ReturnType<typeof setTimeout> | undefined;
	let pingTimer: ReturnType<typeof setInterval> | undefined;

	function schedule() {
		if (closed) return;
		// 1 s, 2 s, 4 s … 30 s, with jitter so a city's phones don't reconnect in lockstep.
		const base = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** attempt);
		attempt += 1;
		retryTimer = setTimeout(open, base / 2 + Math.random() * (base / 2));
	}

	async function open() {
		if (closed) return;
		const token = await opts.token();
		if (!token) return schedule();
		const ws = new WS(wsUrl(opts.baseUrl));
		socket = ws;
		ws.onopen = () => ws.send(JSON.stringify({ type: 'auth', data: { token } }));
		ws.onmessage = (ev) => {
			let m: LiveMessage;
			try {
				m = JSON.parse(String(ev.data));
			} catch {
				return;
			}
			if (m.type === 'ready') {
				ready = true;
				attempt = 0;
				pingTimer = setInterval(() => send('ping', {}), PING_MS);
				opts.onconnect?.();
			} else if (m.type === 'session.revoked') {
				closed = true;
				opts.onrevoked?.();
			} else if (m.type !== 'pong') {
				opts.onmessage(m);
			}
		};
		ws.onclose = () => {
			ready = false;
			clearInterval(pingTimer);
			if (socket === ws) socket = null;
			schedule();
		};
		ws.onerror = () => ws.close();
	}

	function send(type: string, data: unknown): boolean {
		if (!socket || !ready || socket.readyState !== WS.OPEN) return false;
		socket.send(JSON.stringify({ type, data }));
		return true;
	}

	void open();

	return {
		send,
		get connected() {
			return ready;
		},
		close() {
			closed = true;
			clearTimeout(retryTimer);
			clearInterval(pingTimer);
			socket?.close();
		}
	};
}
