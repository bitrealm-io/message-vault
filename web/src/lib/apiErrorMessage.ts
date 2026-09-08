/**
 * Pull the human-readable part out of an API error.
 *
 * `apiClient` now rejects with a `VaultApiError` whose `message` is already
 * the vault's own sentence (`problemFromBody` in `api.ts` reads the problem
 * document once, at the client). There is no `"<status>: <body>"`
 * shape left to unwrap here — this just falls back to `fallback` when the
 * rejection is not an `Error`, or has no message, at all.
 */
export function apiErrorMessage(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}
