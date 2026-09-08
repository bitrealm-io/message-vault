let baseUrl = "";
let authToken: string | null = null;

/** Set the vault server URL. An empty string means "same host as this page". */
export function setBaseUrl(url: string) {
  baseUrl = url.replace(/\/+$/, "");
}

/** Store the session token used on later API calls. Pass null to log out. */
export function setToken(token: string | null) {
  authToken = token;
}

/** Current session token on the API client, or null when signed out. */
export function getToken(): string | null {
  return authToken;
}

export function getBaseUrl(): string {
  return baseUrl;
}

/**
 * The body of every failure the vault answers: an RFC 7807 problem document
 * (ADR-0010). `type` is the URL of the page describing the kind of failure,
 * or `about:blank` for an internal error.
 */
export type Problem = {
  type: string;
  title: string;
  status: number;
  detail?: string;
  errors?: string[];
  request_id?: string;
  word?: string;
  did_you_mean?: string;
  retry_after?: number;
};

/**
 * A raw-text fallback longer than this is someone else's page, not a message
 * — clamped so it cannot overrun the fixed-height auth card, which never
 * scrolls.
 */
const RAW_BODY_FALLBACK_LIMIT = 200;

/**
 * A failed response from the vault: the HTTP status, the problem's slug so a
 * screen can branch on what went wrong, and `message`, the sentence a person
 * reads — the problem's `detail`, or its `errors` joined for a validation
 * failure.
 */
export class VaultApiError extends Error {
  readonly status: number;
  /** The last segment of the problem's `type` URL; null when the body was not a problem. */
  readonly type: string | null;
  readonly title: string | null;
  readonly detail: string | null;
  readonly errors: string[];
  readonly requestId: string | null;

  constructor(status: number, message: string, problem: Problem | null = null) {
    super(message);
    this.name = "VaultApiError";
    this.status = status;
    this.type = problem ? problemSlug(problem.type) : null;
    this.title = problem?.title ?? null;
    this.detail = problem?.detail ?? null;
    this.errors = problem?.errors ?? [];
    this.requestId = problem?.request_id ?? null;
  }
}

function problemSlug(type: string): string | null {
  if (type === "about:blank") return null;
  const slug = type.slice(type.lastIndexOf("/") + 1);
  return slug ? slug : null;
}

function parseProblem(text: string): Problem | null {
  try {
    const parsed: unknown = JSON.parse(text);
    if (
      parsed &&
      typeof parsed === "object" &&
      "type" in parsed &&
      "title" in parsed &&
      typeof (parsed as { type: unknown }).type === "string" &&
      typeof (parsed as { title: unknown }).title === "string"
    ) {
      return parsed as Problem;
    }
  } catch {
    // Not JSON — the raw text is the best available message.
  }
  return null;
}

/**
 * The error to throw for a failed response.
 *
 * The vault answers a problem document, and its `detail` (or, for a
 * validation failure, every one of its `errors`) is what a user should read —
 * not the status code and not the envelope around it. Anything else (a
 * proxy's HTML error page, an empty body) falls back to the raw text — clamped
 * to `RAW_BODY_FALLBACK_LIMIT` characters, since a reverse proxy or non-vault
 * host can answer with a whole HTML page — then to a generic sentence.
 */
export function problemFromBody(status: number, text: string): VaultApiError {
  const trimmed = text.trim();
  if (!trimmed) return new VaultApiError(status, `Request failed (${status})`);

  const problem = parseProblem(trimmed);
  if (problem) {
    const detail = problem.detail?.trim();
    const errors = (problem.errors ?? []).map((e) => e.trim()).filter(Boolean);
    const message =
      detail || errors.join("; ") || problem.title.trim() || `Request failed (${status})`;
    return new VaultApiError(status, message, problem);
  }
  if (trimmed.length > RAW_BODY_FALLBACK_LIMIT) {
    return new VaultApiError(status, `${trimmed.slice(0, RAW_BODY_FALLBACK_LIMIT)}…`);
  }
  return new VaultApiError(status, trimmed);
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (authToken) {
    headers.Authorization = `Bearer ${authToken}`;
  }

  const res = await fetch(`${baseUrl}${path}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
    signal,
  });

  if (!res.ok) {
    const text = await res.text();
    throw problemFromBody(res.status, text);
  }

  // A 204 has no body by definition; asking for JSON would throw.
  if (res.status === 204) {
    return undefined as T;
  }

  return res.json() as Promise<T>;
}

export type ApiRequestOptions = {
  signal?: AbortSignal;
};

export const apiClient = {
  get<T>(path: string, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("GET", path, undefined, opts?.signal);
  },
  post<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("POST", path, body, opts?.signal);
  },
  put<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("PUT", path, body, opts?.signal);
  },
  patch<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("PATCH", path, body, opts?.signal);
  },
  delete<T>(path: string, body?: unknown, opts?: ApiRequestOptions): Promise<T> {
    return request<T>("DELETE", path, body, opts?.signal);
  },
};
