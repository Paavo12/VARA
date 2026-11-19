import { describe, expect, it } from 'vitest';
import { formatDetail, formatWhen } from './format.ts';

describe('formatDetail', () => {
	it('shows Unix-second times as dates', () => {
		const at = 1_795_000_000;
		const text = formatDetail({ tier: 'comfort', effective_from: at, service_fee_bp: 1650 });
		expect(text).toContain(formatWhen(new Date(at * 1000).toISOString()));
		expect(text).not.toContain(String(at));
		expect(text).toContain('"service_fee_bp":1650');
	});

	it('leaves other numbers and missing times alone', () => {
		expect(formatDetail({ from: 3000, to: 4500, before: null })).toBe(
			'{"from":3000,"to":4500,"before":null}'
		);
		expect(formatDetail(null)).toBe('');
	});
});
