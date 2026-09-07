/** What every vault sign-in route returns: a session token and the account it belongs to. */
export interface SessionResponse {
  token: string;
  account_id: string;
}

/** Desktop login default. IPv4 loopback, because `localhost` often resolves to IPv6 and Docker Compose publishes 8080 on IPv4 only. */
export const DEFAULT_TAURI_VAULT_URL = "http://127.0.0.1:8080";

/**
 * First value for the login server URL field.
 * Replaces the old `http://localhost:8080` default so a saved session still reaches a local Docker vault.
 */
export function initialLoginServerUrl(savedUrl: string | undefined, inTauri: boolean): string {
  if (typeof savedUrl === "string" && savedUrl.length > 0) {
    const normalized = savedUrl.trim().replace(/\/+$/, "");
    if (normalized === "http://localhost:8080") {
      return DEFAULT_TAURI_VAULT_URL;
    }
    return savedUrl;
  }
  return inTauri ? DEFAULT_TAURI_VAULT_URL : "";
}

export interface ParsedPersistedAuth {
  serverUrl: string;
  token: string;
  accountId: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Read a saved login session from JSON.
 * Returns null when the text is not valid JSON or required fields are missing.
 */
export function parsePersistedAuth(raw: string): ParsedPersistedAuth | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }

  if (!isRecord(parsed)) return null;

  if (typeof parsed.serverUrl !== "string") return null;
  if (typeof parsed.token !== "string" || !parsed.token) return null;
  if (typeof parsed.accountId !== "string" || !parsed.accountId) return null;

  return {
    serverUrl: parsed.serverUrl,
    token: parsed.token,
    accountId: parsed.accountId,
  };
}
