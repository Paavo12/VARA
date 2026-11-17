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
export type Place = components['schemas']['Place'];
export type Quote = components['schemas']['Quote'];
export type QuoteOption = components['schemas']['QuoteOption'];
export type RiderRide = components['schemas']['RiderRide'];
export type DriverRide = components['schemas']['DriverRide'];
export type DriverStatus = components['schemas']['DriverStatus'];
export type OfferCard = components['schemas']['OfferCard'];
export type Fare = components['schemas']['Fare'];
export type ChatMessage = components['schemas']['ChatMessage'];
export type RideCheckView = components['schemas']['RideCheckView'];
export type IssueCategory = components['schemas']['IssueCategory'];

export {
	createSession,
	memoryTokenStore,
	type Api,
	type App,
	type Session,
	type SessionOptions,
	type TokenStore
} from './session';
export { connectLive, wsUrl, type Live, type LiveMessage, type LiveOptions } from './live';
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
