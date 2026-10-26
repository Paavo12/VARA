// Display formats from the design brief §8: N$ 123.40 · +264 81 123 4567.

/** Formats integer cents of NAD, as the API sends them, e.g. `11650` → `N$ 116.50`. */
export function formatNad(cents: number): string {
	if (!Number.isSafeInteger(cents)) throw new TypeError('cents must be an integer');
	const sign = cents < 0 ? '-' : '';
	const abs = Math.abs(cents);
	const dollars = Math.floor(abs / 100).toLocaleString('en-NA');
	return `${sign}N$ ${dollars}.${String(abs % 100).padStart(2, '0')}`;
}

/** Formats an E.164 Namibian number for display: `+264811234567` → `+264 81 123 4567`. */
export function formatPhone(e164: string): string {
	const m = /^\+264(\d{2})(\d{3})(\d{3,4})$/.exec(e164);
	return m ? `+264 ${m[1]} ${m[2]} ${m[3]}` : e164;
}
