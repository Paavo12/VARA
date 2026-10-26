import { describe, expect, it } from 'vitest';
import { formatNad, formatPhone } from './format';

describe('formatNad', () => {
	it('formats cents as Namibian dollars', () => {
		expect(formatNad(11650)).toBe('N$ 116.50');
		expect(formatNad(5)).toBe('N$ 0.05');
		expect(formatNad(123456789)).toBe('N$ 1,234,567.89');
		expect(formatNad(-250)).toBe('-N$ 2.50');
	});

	it('rejects non-integers so floats never reach the UI', () => {
		expect(() => formatNad(1.5)).toThrow(TypeError);
	});
});

describe('formatPhone', () => {
	it('groups Namibian numbers', () => {
		expect(formatPhone('+264811234567')).toBe('+264 81 123 4567');
	});

	it('leaves anything else untouched', () => {
		expect(formatPhone('+27821234567')).toBe('+27821234567');
	});
});
