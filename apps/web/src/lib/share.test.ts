import { describe, expect, it, vi } from 'vitest';
import { ageText, etaText, headline, live, loadTrip, mapLink, type SharedTrip } from './share';

const trip: SharedTrip = {
	status: 'in_trip',
	shared_by: 'Ndeshi',
	shared_by_party: 'rider',
	driver_first_name: 'Hilma',
	car: { make: 'Toyota', model: 'Corolla', colour: 'White', plate: 'N 6152 W' },
	position: { lat: -22.5662, lon: 17.0813, at: '2026-11-17T10:00:00Z' },
	destination_area: 'Klein Windhoek area',
	eta: '2026-11-17T10:12:00Z',
	expires_at: null,
	police_phone: '10111'
};

describe('share page', () => {
	it('says where the trip is for each side', () => {
		expect(headline(trip)).toBe('Ndeshi is on her way to Klein Windhoek area');
		expect(headline({ ...trip, status: 'driver_on_the_way' })).toBe(
			"Ndeshi's driver is on the way"
		);
		expect(headline({ ...trip, status: 'driver_arrived', shared_by_party: 'driver' })).toBe(
			'Ndeshi is at the pickup'
		);
		expect(headline({ ...trip, status: 'ended' })).toBe("Ndeshi's trip has ended");
	});

	it('words the ETA and the age of the position', () => {
		const now = new Date('2026-11-17T10:04:00Z');
		expect(etaText('2026-11-17T10:12:00Z', now)).toMatch(/\(in 8 min\)$/);
		expect(etaText('2026-11-17T10:04:30Z', now)).toBe('any moment now');
		expect(ageText('2026-11-17T10:03:55Z', now)).toBe('just now');
		expect(ageText('2026-11-17T10:03:20Z', now)).toBe('40 s ago');
		expect(ageText('2026-11-17T10:01:00Z', now)).toBe('3 min ago');
	});

	it('links the position with five decimals', () => {
		expect(mapLink({ lat: -22.566234, lon: 17.081311 })).toBe(
			'https://www.openstreetmap.org/?mlat=-22.56623&mlon=17.08131#map=16/-22.56623/17.08131'
		);
	});

	it('maps API answers and stops polling once the trip ends', async () => {
		const answer = (status: number, body: unknown = {}) =>
			vi.fn().mockResolvedValue(new Response(JSON.stringify(body), { status }));
		expect(await loadTrip('t', answer(404))).toEqual({ kind: 'not_found' });
		expect(await loadTrip('t', answer(410))).toEqual({ kind: 'expired' });
		expect(await loadTrip('t', answer(429))).toEqual({ kind: 'retry' });
		expect(await loadTrip('t', vi.fn().mockRejectedValue(new Error('offline')))).toEqual({
			kind: 'retry'
		});
		const ok = await loadTrip('t', answer(200, trip));
		expect(ok).toEqual({ kind: 'trip', trip });
		expect(live(ok)).toBe(true);
		expect(live({ kind: 'trip', trip: { ...trip, status: 'ended' } })).toBe(false);
		expect(live({ kind: 'expired' })).toBe(false);
	});
});
