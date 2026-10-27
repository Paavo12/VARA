import type { components } from './schema';
import type { Api } from './session';

export type DocumentKind = components['schemas']['DocumentKind'];
export type UploadStatus = components['schemas']['UploadStatus'];

const ACCEPTED = ['image/jpeg', 'image/png', 'application/pdf'] as const;
type Accepted = (typeof ACCEPTED)[number];
export const MAX_UPLOAD_BYTES = 10 * 1024 * 1024;

export class UploadError extends Error {}

function contentType(file: Blob): Accepted {
	const t = file.type as Accepted;
	if (!ACCEPTED.includes(t)) {
		throw new UploadError('Please choose a JPEG or PNG photo (or a PDF for documents).');
	}
	return t;
}

/**
 * Uploads a file for `purpose` (TRD §7.2): asks the API for a signed URL, sends the bytes
 * straight to storage, then asks the API to check it. Resolves with the upload ID once the
 * server has queued the checks; use {@link waitForUpload} to wait for the verdict.
 */
export async function uploadFile(
	api: Api,
	purpose: DocumentKind,
	file: Blob,
	fetchImpl: typeof fetch = fetch
): Promise<string> {
	if (file.size === 0) throw new UploadError('This file is empty.');
	if (file.size > MAX_UPLOAD_BYTES) throw new UploadError('This file is larger than 10 MB.');
	const type = contentType(file);
	const { data: target, error } = await api.POST('/v1/uploads', {
		body: { purpose, content_type: type, size_bytes: file.size }
	});
	if (!target) throw new UploadError(error?.error.message ?? 'Upload failed. Please try again.');
	const put = await fetchImpl(target.url, {
		method: target.method,
		headers: target.headers,
		body: file
	});
	if (!put.ok) throw new UploadError('Upload failed. Please check your connection and try again.');
	const done = await api.POST('/v1/uploads/{id}/complete', {
		params: { path: { id: target.upload_id } }
	});
	if (!done.data)
		throw new UploadError(done.error?.error.message ?? 'Upload failed. Please try again.');
	return target.upload_id;
}

/** Polls until the server has checked the upload (usually a few seconds). */
export async function waitForUpload(
	api: Api,
	id: string,
	{ intervalMs = 1500, timeoutMs = 60_000 } = {}
): Promise<UploadStatus> {
	const until = Date.now() + timeoutMs;
	for (;;) {
		const { data } = await api.GET('/v1/uploads/{id}', {
			params: { path: { id } }
		});
		if (data && (data.status === 'clean' || data.status === 'rejected')) return data;
		if (Date.now() > until) throw new UploadError('Checking the file is taking longer than usual.');
		await new Promise((r) => setTimeout(r, intervalMs));
	}
}
