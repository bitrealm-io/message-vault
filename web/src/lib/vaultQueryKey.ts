/**
 * How a vault cache entry is named.
 *
 * Its own module, importing nothing, because both `vaultQuery` and `auth` need
 * it: `vaultQuery` reads the signed-in account from `auth`, so `auth` importing
 * `vaultQuery` back would be a cycle.
 */

/** Key parts, before the account is put in front of them. */
export type VaultQueryKey = readonly unknown[];

/**
 * Name of the account a query runs for, before sign-in.
 *
 * Queries on the sign-in screens have no account yet; giving them a name of
 * their own keeps their entries from ever being read by a signed-in account.
 */
export const ANONYMOUS_ACCOUNT = "anonymous";

/** Who a cache entry belongs to: an account id, or the sign-in screens. */
export type AccountScope = number | typeof ANONYMOUS_ACCOUNT;

/**
 * Put the account in front of a key.
 *
 * Every cache entry carries the account that filled it, so no account can be
 * served another's data. See
 * `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */
export function vaultQueryKey(account: AccountScope, key: VaultQueryKey): unknown[] {
  return ["vault", account, ...key];
}
