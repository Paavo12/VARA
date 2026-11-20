/**
 * The signed-in staff member and a typed API client.
 *
 * The 8-hour session is an HttpOnly cookie the API sets at sign-in: this code never sees the
 * token, and a reload stays signed in ({@link restoreSession}). Requests that change something
 * send the session's CSRF token in X-CSRF-Token. The console must be served from the same site
 * as the API, or the browser won't send the cookie.
 */
import createFetchClient from 'openapi-fetch';
import type { components, paths } from '@fuber/api-client';

export type StaffSession = components['schemas']['StaffSession'];
export type DriverQueueItem = components['schemas']['DriverQueueItem'];
export type DriverReview = components['schemas']['DriverReview'];
export type ReviewDocument = components['schemas']['ReviewDocument'];
export type VerificationQueueItem = components['schemas']['QueueItem'];
export type VerificationDetail = components['schemas']['VerificationDetail'];
export type Tier = components['schemas']['Tier'];
export type IncidentItem = components['schemas']['IncidentItem'];
export type SettlementItem = components['schemas']['SettlementItem'];
export type UserItem = components['schemas']['UserItem'];
export type UserDetail = components['schemas']['UserDetail'];
export type SupportReport = components['schemas']['SupportReport'];
export type Setting = components['schemas']['Setting'];
export type Metrics = components['schemas']['Metrics'];
export type TierRates = components['schemas']['TierRates'];
export type SurgeZone = components['schemas']['SurgeZone'];
export type AuditEntry = components['schemas']['AuditEntry'];
export type IncidentMessage = components['schemas']['IncidentMessage'];

/** The API base URL. Set VITE_API_URL at build time. */
export const API_URL: string = import.meta.env.VITE_API_URL ?? 'http://127.0.0.1:8080';

let session = $state<StaffSession | null>(null);
/** False until the first session check after a page load has answered. */
let checked = $state(false);

export const staff = {
	get session() {
		return session;
	},
	get checked() {
		return checked;
	},
	get canReview() {
		return !!session?.roles.some((r) => r === 'ops_reviewer' || r === 'ops_admin');
	},
	get canHandleIncidents() {
		return !!session?.roles.some((r) => r === 'ops_safety' || r === 'ops_admin');
	},
	/** Users, suspensions, priority points and reported issues. */
	get canSupport() {
		return !!session?.roles.some((r) => r === 'ops_support' || r === 'ops_admin');
	},
	/** Weekly settlements: money, admins only. */
	get canSettle() {
		return !!session?.roles.includes('ops_admin');
	}
};

export const api = createFetchClient<paths>({ baseUrl: API_URL, credentials: 'include' });
api.use({
	onRequest({ request }) {
		if (session && request.method !== 'GET')
			request.headers.set('X-CSRF-Token', session.csrf_token);
		return request;
	},
	onResponse({ response }) {
		// Expired or revoked: back to the sign-in form.
		if (response.status === 401 && session) session = null;
		return response;
	}
});

export function signedIn(s: StaffSession): void {
	session = s;
}

/** After a page load: picks up the session cookie if it is still valid. */
export async function restoreSession(): Promise<void> {
	const { data } = await api.GET('/v1/ops/auth/session').catch(() => ({ data: undefined }));
	session = data ?? null;
	checked = true;
}

export async function signOut(): Promise<void> {
	if (session) await api.POST('/v1/ops/auth/logout').catch(() => undefined);
	session = null;
}
