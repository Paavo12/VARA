import { describe, expect, it } from 'vitest';
import { formatDistance, formatMinutes } from './duration';

describe('formatMinutes', () => {
	it('rounds up and never shows zero', () => {
		expect(formatMinutes(0)).toBe('1 min');
		expect(formatMinutes(61)).toBe('2 min');
		expect(formatMinutes(3900)).toBe('1 h 5 min');
		expect(formatMinutes(7200)).toBe('2 h');
	});
});

describe('formatDistance', () => {
	it('uses metres below a kilometre', () => {
		expect(formatDistance(647)).toBe('650 m');
		expect(formatDistance(1450)).toBe('1.4 km');
	});
});
