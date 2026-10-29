/** "4 min", "1 h 5 min": durations on cards (rounded up so an ETA never under-promises). */
export function formatMinutes(seconds: number): string {
	const m = Math.max(1, Math.ceil(seconds / 60));
	if (m < 60) return `${m} min`;
	const h = Math.floor(m / 60);
	return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}

/** "1.4 km" or "650 m". */
export function formatDistance(metres: number): string {
	return metres < 1000 ? `${Math.round(metres / 10) * 10} m` : `${(metres / 1000).toFixed(1)} km`;
}
