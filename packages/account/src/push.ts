/**
 * Push notifications on the phone (App Flow §10): asks permission, gives the server this
 * device's Firebase token for the signed-in session, and reports taps so the app can show
 * the right screen. Does nothing in a browser.
 *
 * The plugin is registered here rather than imported from its package entry, which would
 * pull the Firebase web SDK into the bundle for a web fallback VARA never uses.
 */
import type {
	Channel,
	FirebaseMessagingPlugin,
	NotificationActionPerformedEvent
} from '@capacitor-firebase/messaging';
import { Capacitor, registerPlugin, type PluginListenerHandle } from '@capacitor/core';
import type { Session } from '@fuber/api-client';

const FirebaseMessaging = registerPlugin<FirebaseMessagingPlugin>('FirebaseMessaging');

/** What the server puts in a notice's data (`kind`, and `ride_id` for ride notices). */
export interface PushTap {
	kind: string;
	rideId?: string;
}

/**
 * Android channels, named as the server sends them: `safety` (pickup, RideCheck) may sound
 * and show at once; `rides` is everything else. Values follow the plugin's `Importance`.
 */
const CHANNELS: Channel[] = [
	{ id: 'safety', name: 'Pickup and safety', importance: 5, vibration: true },
	{ id: 'rides', name: 'Trips and account', importance: 3 }
];

export interface Push {
	/** Stops notices to this device (call before signing out). */
	stop(): Promise<void>;
}

/** Reads the tap's data; unknown shapes give `null`. */
export function readTap(
	event: Pick<NotificationActionPerformedEvent, 'notification'>
): PushTap | null {
	const data = event.notification.data as Record<string, unknown> | undefined;
	if (!data || typeof data.kind !== 'string') return null;
	return { kind: data.kind, rideId: typeof data.ride_id === 'string' ? data.ride_id : undefined };
}

/**
 * Starts push for the signed-in `session`. Safe to call more than once a session: the token
 * is simply registered again. Any failure (no Firebase config in a development build,
 * permission refused) leaves the app working without push.
 */
export async function startPush(session: Session, ontap: (tap: PushTap) => void): Promise<Push> {
	const none: Push = { stop: async () => {} };
	if (!Capacitor.isNativePlatform()) return none;

	const listeners: PluginListenerHandle[] = [];
	const register = async (token: string) => {
		await session.api
			.PUT('/v1/me/push-token', { body: { token, platform: 'fcm' } })
			.catch(() => undefined);
	};
	try {
		let { receive } = await FirebaseMessaging.checkPermissions();
		if (receive === 'prompt' || receive === 'prompt-with-rationale') {
			({ receive } = await FirebaseMessaging.requestPermissions());
		}
		if (receive !== 'granted') return none;
		if (Capacitor.getPlatform() === 'android') {
			for (const c of CHANNELS) await FirebaseMessaging.createChannel(c);
		}
		listeners.push(
			await FirebaseMessaging.addListener('tokenReceived', (e) => void register(e.token)),
			await FirebaseMessaging.addListener('notificationActionPerformed', (e) => {
				const tap = readTap(e);
				if (tap) ontap(tap);
			})
		);
		await register((await FirebaseMessaging.getToken()).token);
	} catch {
		// Firebase isn't set up in this build (no google-services.json / GoogleService-Info.plist).
	}
	return {
		async stop() {
			await session.api.DELETE('/v1/me/push-token').catch(() => undefined);
			for (const l of listeners) await l.remove();
			// A new token next time: whoever signs in on this phone never gets the old one's notices.
			await FirebaseMessaging.deleteToken().catch(() => undefined);
		}
	};
}
