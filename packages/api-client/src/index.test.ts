import { describe, expect, it } from 'vitest';
import { createApiClient } from './index';

describe('createApiClient', () => {
	it('sends typed requests to the API base URL', async () => {
		let seen: Request | undefined;
		const fake: typeof fetch = async (input) => {
			seen = input as Request;
			return new Response(JSON.stringify({ status: 'ok' }), {
				headers: { 'content-type': 'application/json' }
			});
		};
		const api = createApiClient('https://api.example.test', fake);
		const { response } = await api.GET('/v1/health');
		expect(response.status).toBe(200);
		expect(seen?.url).toBe('https://api.example.test/v1/health');
	});
});
