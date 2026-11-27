import { describe, expect, it, vi } from 'vitest';
import type { Session } from '@fuber/api-client';
import { readTap, startPush } from './push';

describe('readTap', () => {
	it('reads the notice kind and ride', () => {
		const tap = readTap({ notification: { id: '1', data: { kind: 'ridecheck', ride_id: 'r-1' } } });
		expect(tap).toEqual({ kind: 'ridecheck', rideId: 'r-1' });
		expect(readTap({ notification: { id: '2', data: { kind: 'rider_verified' } } })).toEqual({
			kind: 'rider_verified',
			rideId: undefined
		});
	});

	it('ignores notifications that are not VARA notices', () => {
		expect(readTap({ notification: { id: '3' } })).toBeNull();
		expect(readTap({ notification: { id: '4', data: { kind: 7 } } })).toBeNull();
	});
});

describe('startPush', () => {
	it('does nothing in a browser', async () => {
		const put = vi.fn();
		const session = { api: { PUT: put, DELETE: vi.fn() } } as unknown as Session;
		const push = await startPush(session, () => {});
		await push.stop();
		expect(put).not.toHaveBeenCalled();
	});
});
