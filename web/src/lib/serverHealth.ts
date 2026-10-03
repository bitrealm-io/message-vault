/** How long to wait after failure attempt `failureIndex` (0-based) before the next probe. */
export const HEALTH_BACKOFF_CAP_MS = 30_000;
export const HEALTH_SUCCESS_RECHECK_MS = 30_000;
export const HEALTH_URL_DEBOUNCE_MS = 400;
/** Give up on a single /health request so a black-holed host cannot leave the light grey. */
export const HEALTH_PROBE_TIMEOUT_MS = 8_000;

/** Progressive backoff: 1s → 2s → 4s → … capped at 30s. */
export function healthBackoffMs(failureIndex: number): number {
  const n = Math.max(0, Math.floor(failureIndex));
  const delay = 1000 * 2 ** n;
  return Math.min(delay, HEALTH_BACKOFF_CAP_MS);
}

export type ServerHealthStatus = "unknown" | "checking" | "ok" | "fail";

/**
 * URL to GET for server liveness.
 * A blank value means this origin (same as the API client empty base URL).
 * Returns null when the value is not empty and not an absolute http(s) URL.
 */
export function healthProbeUrl(baseUrl: string): string | null {
  const trimmed = baseUrl.trim();
  if (!trimmed) return "/health";
  try {
    const parsed = new URL(trimmed);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return null;
    }
  } catch {
    return null;
  }
  return `${trimmed.replace(/\/+$/, "")}/health`;
}

/**
 * Probe server liveness via GET /health (plain text, not JSON).
 * Returns true only when the response is OK.
 *
 * Calls `fetch` itself rather than going through `serverApi.ts`: the address
 * may be one the person is still typing on the login screen, not the server
 * `apiClient` talks to. One of the two named exceptions in
 * `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */
export async function checkServerHealth(baseUrl: string, signal?: AbortSignal): Promise<boolean> {
  const url = healthProbeUrl(baseUrl);
  if (!url) return false;
  if (signal?.aborted) return false;

  const timeoutController = new AbortController();
  const timer = setTimeout(() => timeoutController.abort(), HEALTH_PROBE_TIMEOUT_MS);
  const onParentAbort = () => timeoutController.abort();
  signal?.addEventListener("abort", onParentAbort, { once: true });

  try {
    const res = await fetch(url, {
      method: "GET",
      signal: timeoutController.signal,
      cache: "no-store",
    });
    return res.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", onParentAbort);
  }
}

export function healthStatusLabel(status: ServerHealthStatus): string {
  switch (status) {
    case "ok":
      return "Connected";
    case "fail":
      return "Disconnected";
    // "No answer yet" and "still trying" are the same thing to a reader, so
    // both grey states say the same word.
    case "checking":
    case "unknown":
      return "Connecting…";
    default: {
      const _exhaustive: never = status;
      return _exhaustive;
    }
  }
}
