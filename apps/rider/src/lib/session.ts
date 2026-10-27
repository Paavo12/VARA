import { createSession } from '@fuber/api-client';
import { deviceTokenStore } from '@fuber/account';

/** The API base URL. Set VITE_API_URL at build time; phones need the PC's Wi-Fi address. */
export const API_URL: string = import.meta.env.VITE_API_URL ?? 'http://127.0.0.1:8080';

export function riderSession(onSignedOut: () => void) {
	return createSession({
		baseUrl: API_URL,
		app: 'rider',
		store: deviceTokenStore('rider'),
		appVersion: import.meta.env.VITE_APP_VERSION,
		onSignedOut
	});
}
