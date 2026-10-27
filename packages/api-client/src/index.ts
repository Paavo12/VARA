import createFetchClient from 'openapi-fetch';
import type { components, paths } from './schema';

export type { components, paths };
export type ApiError = components['schemas']['ErrorBody'];
export type FareEstimate = components['schemas']['EstimateResponse'];
export type Me = components['schemas']['Me'];
export type LegalDocument = components['schemas']['LegalDocument'];
export type DriverApplication = components['schemas']['Application'];
export type DriverDocument = components['schemas']['DriverDocument'];
export type Tier = components['schemas']['Tier'];

export {
	createSession,
	memoryTokenStore,
	type Api,
	type App,
	type Session,
	type SessionOptions,
	type TokenStore
} from './session';
export {
	MAX_UPLOAD_BYTES,
	UploadError,
	uploadFile,
	waitForUpload,
	type DocumentKind,
	type UploadStatus
} from './upload';

/**
 * Creates a typed client. Requests and responses are checked against the backend's OpenAPI
 * document at compile time, e.g. `api.POST('/v1/fares/estimate', { body })`.
 */
export function createApiClient(baseUrl: string, fetchImpl?: typeof fetch) {
	return createFetchClient<paths>({ baseUrl, fetch: fetchImpl });
}
