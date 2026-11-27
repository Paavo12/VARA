import { describe, expect, it } from 'vitest';
import { createSession, memoryTokenStore } from './session';

const json = (body: unknown, status = 200) =>
	new Response(JSON.stringify(body), {
		status,
		headers: { 'content-type': 'application/json' }
	});

const pair = (n: number) => ({
	access_token: `access-${n}`,
	refresh_token: `refresh-${n}`.padEnd(43, 'x'),
	token_type: 'Bearer',
	expires_in: 900
});

/** A fake API: counts refreshes, accepts only the latest access token. */
function fakeApi() {
	const state = { refreshes: 0, current: 'access-0', seen: [] as string[] };
	const fetchImpl: typeof fetch = async (info, init) => {
		// openapi-fetch passes a Request; anything else `fetch` accepts works too.
		const input = info instanceof Request ? info : new Request(info, init);
		const url = new URL(input.url);
		if (url.pathname === '/v1/auth/refresh') {
			state.refreshes += 1;
			await new Promise((r) => setTimeout(r, 10));
			state.current = `access-${state.refreshes}`;
			return json(pair(state.refreshes));
		}
		if (url.pathname === '/v1/auth/otp/verify')
			return json({ ...pair(0), user_id: 'u', new_user: true });
		const auth = input.headers.get('authorization') ?? '';
		state.seen.push(auth);
		return auth === `Bearer ${state.current}`
			? json({ ok: true })
			: json({ error: { code: 'unauthorized', message: '' } }, 401);
	};
	return { state, fetchImpl };
}

describe('createSession', () => {
	it('signs in, sends the token, and stores the refresh token', async () => {
		const { state, fetchImpl } = fakeApi();
		const store = memoryTokenStore();
		const s = createSession({
			baseUrl: 'https://api.test',
			app: 'rider',
			store,
			fetch: fetchImpl
		});
		await s.verifyCode('c', '+264811234567', '123456');
		expect(await store.get()).toBe(pair(0).refresh_token);
		const { response } = await s.api.GET('/v1/me');
		expect(response.status).toBe(200);
		expect(state.seen).toEqual(['Bearer access-0']);
	});

	it('refreshes once for many concurrent requests (refresh tokens are single use)', async () => {
		const { state, fetchImpl } = fakeApi();
		const store = memoryTokenStore();
		await store.set('stored'.padEnd(43, 'x'));
		const s = createSession({
			baseUrl: 'https://api.test',
			app: 'driver',
			store,
			fetch: fetchImpl
		});
		const results = await Promise.all([1, 2, 3, 4].map(() => s.api.GET('/v1/me')));
		expect(results.every((r) => r.response.status === 200)).toBe(true);
		expect(state.refreshes).toBe(1);
		expect(await store.get()).toBe(pair(1).refresh_token);
	});

	it('retries once with a new token after a 401', async () => {
		const { state, fetchImpl } = fakeApi();
		const s = createSession({
			baseUrl: 'https://api.test',
			app: 'rider',
			store: memoryTokenStore(),
			fetch: fetchImpl
		});
		await s.verifyCode('c', '+264811234567', '123456');
		state.current = 'access-1'; // the server revoked access-0, e.g. after rotation elsewhere
		state.refreshes = 0;
		const { response } = await s.api.GET('/v1/me');
		expect(response.status).toBe(200);
		expect(state.refreshes).toBe(1);
	});

	it('ends the session when the refresh token is rejected', async () => {
		let signedOut = false;
		const store = memoryTokenStore();
		await store.set('spent'.padEnd(43, 'x'));
		const fetchImpl = async () => json({ error: { code: 'unauthorized', message: '' } }, 401);
		const s = createSession({
			baseUrl: 'https://api.test',
			app: 'rider',
			store,
			fetch: fetchImpl,
			onSignedOut: () => (signedOut = true)
		});
		expect(await s.restore()).toBe(false);
		expect(signedOut).toBe(true);
		expect(await store.get()).toBeNull();
	});

	it('tells the app when new terms must be accepted', async () => {
		let asked = 0;
		const fetchImpl = async (req: Request) =>
			req.url.endsWith('/v1/rides/active')
				? json({ error: { code: 'forbidden', message: '' } }, 403)
				: json({ error: { code: 'consent_required', message: '' } }, 403);
		const s = createSession({
			baseUrl: 'https://api.test',
			app: 'rider',
			store: memoryTokenStore(),
			fetch: fetchImpl,
			onConsentRequired: () => asked++
		});
		const { response } = await s.api.POST('/v1/uploads', {
			body: { purpose: 'selfie', content_type: 'image/png', size_bytes: 10 }
		});
		expect(response.status).toBe(403);
		expect(asked).toBe(1);
		await s.api.GET('/v1/rides/active');
		expect(asked).toBe(1);
	});

	it('keeps the stored token when the network is down', async () => {
		const store = memoryTokenStore();
		await store.set('keep'.padEnd(43, 'x'));
		const s = createSession({
			baseUrl: 'https://api.test',
			app: 'rider',
			store,
			fetch: async () => {
				throw new TypeError('offline');
			}
		});
		expect(await s.restore()).toBe(false);
		expect(await store.get()).toBe('keep'.padEnd(43, 'x'));
	});
});
