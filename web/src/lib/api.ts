export interface ManifestPhoto {
  hash: string;
  w: number;
  h: number;
  effective_date: string;
  date_source: 'exif' | 'mtime' | 'override';
  favorite: boolean;
  tags: string[];
  /** Curated rotation, degrees clockwise: what the photograph should show. */
  rotation: number;
  /** The rotation the derivatives on the server were made for. Media URLs use
   *  this; it catches up to `rotation` when the indexer has regenerated. */
  media_rotation: number;
}

export interface TagCount {
  name: string;
  count: number;
}

export interface Manifest {
  generation: number;
  indexing: boolean;
  photos: ManifestPhoto[];
  tags: TagCount[];
}

export interface Status {
  generation: number;
  indexing: boolean;
  photo_count: number;
  last_scan_at: string | null;
  scan_total: number;
  scan_done: number;
  derivative_queue: number;
  derivative_failures: number;
}

export interface RequestInfo {
  id: number;
  kind: 'delete' | 'export';
  target: string | null;
  state: 'pending' | 'running' | 'done' | 'failed';
  result: string | null;
}

export type Variant = 'display' | 'preview' | 'thumb' | 'blur';

/** The rotation is part of the address, so a rotated photograph gets a new URL
 *  and the old one stays immutable. */
export const mediaUrl = (hash: string, variant: Variant, mediaRotation = 0): string =>
  `/media/${hash}/${variant}${mediaRotation ? `?r=${mediaRotation}` : ''}`;

/** A management call needs credentials the browser does not have yet. */
export class AuthRequired extends Error {
  constructor() {
    super('authentication required');
  }
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

// ---- credentials -----------------------------------------------------------
// HTTP Basic for the management routes. These are the server's credentials, not
// client settings, and live only for the browser session.

const CRED_KEY = 'pf-admin';
let credentials: string | null = null;

try {
  credentials = sessionStorage.getItem(CRED_KEY);
} catch {
  /* storage unavailable: the user just logs in again */
}

export function setCredentials(user: string, password: string): void {
  credentials = btoa(unescape(encodeURIComponent(`${user}:${password}`)));
  try {
    sessionStorage.setItem(CRED_KEY, credentials);
  } catch {
    /* ignore */
  }
}

export function clearCredentials(): void {
  credentials = null;
  try {
    sessionStorage.removeItem(CRED_KEY);
  } catch {
    /* ignore */
  }
}

// ---- transport -------------------------------------------------------------

async function problem(res: Response): Promise<ApiError> {
  let detail = `${res.status} ${res.statusText}`;
  try {
    const body = await res.json();
    detail = body.detail ?? body.title ?? detail;
  } catch {
    /* not JSON */
  }
  return new ApiError(res.status, detail);
}

async function call(path: string, init: RequestInit = {}, auth = false): Promise<Response> {
  const headers = new Headers(init.headers);
  if (auth) {
    if (!credentials) throw new AuthRequired();
    headers.set('Authorization', `Basic ${credentials}`);
  }
  const res = await fetch(path, { ...init, headers });
  if (res.status === 401 && auth) {
    clearCredentials();
    throw new AuthRequired();
  }
  if (!res.ok) throw await problem(res);
  return res;
}

const json = (body: unknown): RequestInit => ({
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(body),
});

// ---- open endpoints (the frame is a kiosk with no keyboard) -----------------

export async function getStatus(): Promise<Status> {
  return (await call('/api/status')).json();
}

export async function getManifest(): Promise<Manifest> {
  return (await call('/api/manifest')).json();
}

export async function toggleFavorite(hash: string): Promise<boolean> {
  return (await (await call(`/api/photos/${hash}/favorite`, json({}))).json()).favorite;
}

export async function createTag(name: string): Promise<string> {
  return (await (await call('/api/tags', json({ name }))).json()).name;
}

export async function addTag(hash: string, name: string): Promise<void> {
  await call(`/api/photos/${hash}/tags`, json({ add: name }));
}

export async function removeTag(hash: string, name: string): Promise<void> {
  await call(`/api/photos/${hash}/tags`, json({ remove: name }));
}

// ---- management endpoints (HTTP Basic) ---------------------------------------

/** Side-effect-free credential check for the login form. */
export async function verifyCredentials(): Promise<boolean> {
  try {
    await call('/api/session', {}, true);
    return true;
  } catch (e) {
    if (e instanceof AuthRequired) return false;
    throw e;
  }
}

/** `null` clears the override. `local` is a wall-clock time, kept as written. */
export async function setDateOverride(hash: string, iso: string | null): Promise<void> {
  await call(
    `/api/photos/${hash}`,
    { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ date_override: iso }) },
    true,
  );
}

/** Turn a photograph by `delta` degrees clockwise (a multiple of 90). Relative,
 *  so repeated and batched requests compose. The server regenerates the
 *  derivatives; `media_rotation` catches up in the manifest afterwards. */
export async function rotatePhoto(hash: string, delta: number): Promise<void> {
  await call(
    `/api/photos/${hash}`,
    { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ rotate: delta }) },
    true,
  );
}

export async function deletePhoto(hash: string): Promise<void> {
  await call(`/api/photos/${hash}`, { method: 'DELETE' }, true);
}

export async function requestScan(): Promise<void> {
  await call('/api/scan', { method: 'POST' }, true);
}

export async function requestExport(): Promise<number> {
  return (await (await call('/api/export', { method: 'POST' }, true)).json()).request;
}

export async function getRequest(id: number): Promise<RequestInfo> {
  return (await call(`/api/requests/${id}`, {}, true)).json();
}

/** Fetch a finished export with credentials, then hand it to the browser. */
export async function downloadExport(name: string): Promise<Blob> {
  return (await call(`/api/exports/${encodeURIComponent(name)}`, {}, true)).blob();
}

/** Multipart upload with progress. Resolves with how many files were accepted. */
export function uploadFiles(files: File[], onProgress: (fraction: number) => void): Promise<number> {
  return new Promise((resolve, reject) => {
    if (!credentials) return reject(new AuthRequired());
    const form = new FormData();
    for (const f of files) form.append('file', f, f.name);
    const xhr = new XMLHttpRequest();
    xhr.open('POST', '/api/upload');
    xhr.setRequestHeader('Authorization', `Basic ${credentials}`);
    xhr.upload.onprogress = (e) => e.lengthComputable && onProgress(e.loaded / e.total);
    xhr.onerror = () => reject(new ApiError(0, 'network error'));
    xhr.onload = () => {
      if (xhr.status === 401) {
        clearCredentials();
        return reject(new AuthRequired());
      }
      let body: { received?: number; detail?: string; title?: string } = {};
      try {
        body = JSON.parse(xhr.responseText);
      } catch {
        /* not JSON */
      }
      if (xhr.status >= 200 && xhr.status < 300) resolve(body.received ?? files.length);
      else reject(new ApiError(xhr.status, body.detail ?? body.title ?? `upload failed (${xhr.status})`));
    };
    xhr.send(form);
  });
}
