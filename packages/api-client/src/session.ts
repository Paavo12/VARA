import createFetchClient, { type Middleware } from 'openapi-fetch';
import type { components, paths } from './schema';

export type App = components['schemas']['App'];
export type TokenPair = components['schemas']['TokenPair'];

/**
 * Where the refresh token lives between app launches. On phones this must be the
 * Keychain / Keystore (TRD §7.4); never `localStorage`.
 */
export interface TokenStore {
	get(): Promise<string | null>;
	set(refreshToken: string | null): Promise<void>;
}

/** Keeps the token in memory only: signing in again after every reload. For web dev builds. */
export function memoryTokenStore(): TokenStore {
	let value: string | null = null;
	return {
		get: async () => value,
		set: async (v) => {
			value = v;
		}
	};
}

export interface SessionOptions {
	baseUrl: string;
	app: App;
	store: TokenStore;
	/** Sent as `X-App-Version` with every request. */
	appVersion?: string;
	/** Called when the session ends (signed out, refresh rejected, account deleted). */
	onSignedOut?: () => void;
	fetch?: typeof fetch;
}

/** Access tokens are refreshed this long before they expire. */
const REFRESH_MARGIN_MS = 60_000;

/**
 * A signed-in API client. Adds the bearer token to every request, refreshes it shortly before
 * it expires (one refresh at a time: refresh tokens are single use, and presenting a spent one
 * signs the user out everywhere), and retries once after a 401.
 */
export function createSession(opts: SessionOptions) {
	const doFetch = opts.fetch ?? ((input: Request) => fetch(input));
	let access: string | null = null;
	let accessExpiresAt = 0;
	let refreshing: Promise<boolean> | null = null;

	const headers = (): Record<string, string> =>
		opts.appVersion ? { 'x-app-version': opts.appVersion } : {};

	// A bare client for the auth endpoints, so they never recurse through the middleware.
	const raw = createFetchClient<paths>({
		baseUrl: opts.baseUrl,
		fetch: doFetch,
		headers: headers()
	});

	async function adopt(pair: TokenPair): Promise<void> {
		access = pair.access_token;
		accessExpiresAt = Date.now() + pair.expires_in * 1000;
		await opts.store.set(pair.refresh_token);
	}

	async function end(): Promise<void> {
		access = null;
		accessExpiresAt = 0;
		await opts.store.set(null);
		opts.onSignedOut?.();
	}

	function refresh(): Promise<boolean> {
		refreshing ??= (async () => {
			try {
				const token = await opts.store.get();
				if (!token) return false;
				const { data, response } = await raw.POST('/v1/auth/refresh', {
					body: { refresh_token: token }
				});
				if (data) {
					await adopt(data);
					return true;
				}
				// Only a definite rejection ends the session; network errors keep the token.
				if (response.status === 401 || response.status === 403) await end();
				return false;
			} catch {
				return false;
			} finally {
				refreshing = null;
			}
		})();
		return refreshing;
	}

	async function token(): Promise<string | null> {
		if (access && Date.now() < accessExpiresAt - REFRESH_MARGIN_MS) return access;
		return (await refresh()) ? access : null;
	}

	const auth: Middleware = {
		async onRequest({ request }) {
			const t = await token();
			if (t) request.headers.set('authorization', `Bearer ${t}`);
			return request;
		}
	};

	const api = createFetchClient<paths>({
		baseUrl: opts.baseUrl,
		headers: headers(),
		// Retry once with a fresh token if the server says the old one is no longer valid.
		fetch: async (request: Request) => {
			const retry = request.clone();
			const res = await doFetch(request);
			if (res.status !== 401 || !(await refresh()) || !access) return res;
			retry.headers.set('authorization', `Bearer ${access}`);
			return doFetch(retry);
		}
	});
	api.use(auth);

	return {
		/** Typed client with authentication. */
		api,
		app: opts.app,
		/** True if a stored refresh token could be exchanged for a session. */
		async restore(): Promise<boolean> {
			return refresh();
		},
		async requestCode(phone: string) {
			return raw.POST('/v1/auth/otp/request', { body: { phone } });
		},
		async verifyCode(challengeId: string, phone: string, code: string, deviceName?: string) {
			const res = await raw.POST('/v1/auth/otp/verify', {
				body: {
					challenge_id: challengeId,
					phone,
					code,
					app: opts.app,
					device_name: deviceName ?? null
				}
			});
			if (res.data) await adopt(res.data);
			return res;
		},
		async signOut(): Promise<void> {
			if (access) await api.POST('/v1/auth/logout').catch(() => undefined);
			await end();
		},
		/** Forget local tokens without calling the server (after account deletion). */
		forget: end
	};
}

export type Session = ReturnType<typeof createSession>;
export type Api = Session['api'];
