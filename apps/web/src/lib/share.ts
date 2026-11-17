// The Share trip page (PRD S-2): words for each state, and the link to the car's position.

/** The API base URL. Set VITE_API_URL at build time (the CSP allows only this origin). */
export const API_URL: string = import.meta.env.VITE_API_URL ?? 'http://127.0.0.1:8080';

/** How often the page asks for the car's position. */
export const POLL_MS = 10_000;

export type ShareStatus =
	| 'driver_on_the_way'
	| 'driver_arrived'
	| 'in_trip'
	| 'finding_driver'
	| 'with_safety_team'
	| 'ended';

export interface SharedTrip {
	status: ShareStatus;
	shared_by: string;
	shared_by_party: string;
	driver_first_name?: string | null;
	car?: { make: string; model: string; colour: string; plate: string } | null;
	position?: { lat: number; lon: number; at: string } | null;
	destination_area: string;
	eta?: string | null;
	expires_at?: string | null;
	police_phone: string;
}

/** What a fetch of the link came back as. */
export type Loaded =
	| { kind: 'trip'; trip: SharedTrip }
	| { kind: 'expired' }
	| { kind: 'not_found' }
	| { kind: 'retry' };

export async function loadTrip(token: string, fetchImpl: typeof fetch = fetch): Promise<Loaded> {
	let res: Response;
	try {
		res = await fetchImpl(`${API_URL}/v1/trip-shares/${encodeURIComponent(token)}`, {
			cache: 'no-store',
			referrerPolicy: 'no-referrer'
		});
	} catch {
		return { kind: 'retry' };
	}
	if (res.status === 404) return { kind: 'not_found' };
	if (res.status === 410) return { kind: 'expired' };
	if (!res.ok) return { kind: 'retry' };
	return { kind: 'trip', trip: (await res.json()) as SharedTrip };
}

/** The headline for each state. */
export function headline(t: SharedTrip): string {
	const who = t.shared_by;
	switch (t.status) {
		case 'driver_on_the_way':
			return t.shared_by_party === 'driver'
				? `${who} is on her way to a pickup`
				: `${who}'s driver is on the way`;
		case 'driver_arrived':
			return t.shared_by_party === 'driver'
				? `${who} is at the pickup`
				: `${who}'s driver has arrived`;
		case 'in_trip':
			return `${who} is on her way to ${t.destination_area}`;
		case 'finding_driver':
			return `VARA is finding ${who} a new driver`;
		case 'with_safety_team':
			return `VARA's safety team is looking after ${who}'s trip`;
		case 'ended':
			return `${who}'s trip has ended`;
	}
}

/** "around 14:32 (in 8 min)", or "any moment now". */
export function etaText(eta: string, now: Date): string {
	const at = new Date(eta);
	const min = Math.round((at.getTime() - now.getTime()) / 60_000);
	if (min <= 1) return 'any moment now';
	const clock = at.toLocaleTimeString('en-NA', {
		hour: '2-digit',
		minute: '2-digit',
		hour12: false
	});
	return `around ${clock} (in ${min} min)`;
}

/** "just now", "40 s ago", "3 min ago". */
export function ageText(at: string, now: Date): string {
	const s = Math.max(0, Math.round((now.getTime() - new Date(at).getTime()) / 1000));
	if (s < 15) return 'just now';
	if (s < 60) return `${s} s ago`;
	return `${Math.round(s / 60)} min ago`;
}

/** The car's position on OpenStreetMap (no map is embedded yet). */
export function mapLink(p: { lat: number; lon: number }): string {
	const lat = p.lat.toFixed(5);
	const lon = p.lon.toFixed(5);
	return `https://www.openstreetmap.org/?mlat=${lat}&mlon=${lon}#map=16/${lat}/${lon}`;
}

/** Whether the page should keep polling. */
export function live(l: Loaded): boolean {
	return l.kind === 'retry' || (l.kind === 'trip' && l.trip.status !== 'ended');
}
