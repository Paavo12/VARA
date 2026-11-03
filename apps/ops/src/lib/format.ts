// Display helpers for the review screens.

/** The API's error message, safe to show. */
export function messageOf(
	error: unknown,
	fallback = 'Something went wrong. Please try again.'
): string {
	const e = error as { error?: { message?: unknown } } | undefined;
	return typeof e?.error?.message === 'string' ? e.error.message : fallback;
}

/** "identity" → "ID document", "police_clearance" → "Police clearance". */
export function documentName(kind: string): string {
	const names: Record<string, string> = {
		identity: 'ID document',
		selfie: 'Selfie',
		licence: "Driver's licence",
		insurance: 'Insurance',
		police_clearance: 'Police clearance',
		transport_permit: 'Transport permit'
	};
	return names[kind] ?? kind;
}

export function formatWhen(iso: string): string {
	return new Date(iso).toLocaleString('en-NA', {
		day: 'numeric',
		month: 'short',
		year: 'numeric',
		hour: '2-digit',
		minute: '2-digit'
	});
}
