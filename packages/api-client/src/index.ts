import createFetchClient from 'openapi-fetch';
import type { components, paths } from './schema';

export type { components, paths };
export type ApiError = components['schemas']['ErrorBody'];
export type FareEstimate = components['schemas']['EstimateResponse'];

/**
 * Creates a typed client. Requests and responses are checked against the backend's OpenAPI
 * document at compile time, e.g. `api.POST('/v1/fares/estimate', { body })`.
 */
export function createApiClient(baseUrl: string, fetchImpl?: typeof fetch) {
	return createFetchClient<paths>({ baseUrl, fetch: fetchImpl });
}
