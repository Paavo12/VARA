/**
 * The signed-in staff member and a typed API client that sends their token.
 *
 * The 8-hour staff token is kept in memory only, so a reload signs out (a new TOTP code is
 * needed anyway). Phase 5 moves the console to an HttpOnly cookie with CSRF protection.
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
export type IncidentMessage = components['schemas']['IncidentMessage'];

/** The API base URL. Set VITE_API_URL at build time. */
export const API_URL: string = import.meta.env.VITE_API_URL ?? 'http://127.0.0.1:8080';

let session = $state<StaffSession | null>(null);

export const staff = {
	get session() {
		return session;
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

export const api = createFetchClient<paths>({ baseUrl: API_URL });
api.use({
	onRequest({ request }) {
		if (session) request.headers.set('Authorization', `Bearer ${session.token}`);
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

export async function signOut(): Promise<void> {
	if (session) await api.POST('/v1/ops/auth/logout').catch(() => undefined);
	session = null;
}
