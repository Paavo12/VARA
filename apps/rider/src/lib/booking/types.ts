/** A pickup or destination as the booking screens hold it. */
export interface Stop {
	address: string;
	lat: number;
	lon: number;
}

/** Windhoek and about 20 km around it: the API refuses points outside (`LatLng`). */
export function inServiceArea(lat: number, lon: number): boolean {
	return lat >= -22.8 && lat <= -22.35 && lon >= 16.85 && lon <= 17.35;
}

/**
 * Local development only: where "current location" is when the computer running the app is
 * not in Windhoek (Post Street Mall, as in the demo). Production builds use the real GPS fix
 * or ask the rider to search for the pickup.
 */
export const DEV_LOCATION: Stop = { address: 'Post Street Mall', lat: -22.56665, lon: 17.08126 };
