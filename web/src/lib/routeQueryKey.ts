/**
 * How a vault cache entry is named.
 *
 * Its own module, importing nothing, because both `routeQuery` and `auth` need
 * it: `routeQuery` reads the logged-in account from `auth`, so `auth` importing
 * `routeQuery` back would be a cycle.
 */

/** Key parts, before the account is put in front of them. */
export type RouteQueryKey = readonly unknown[];

/**
 * Name of the account a query runs for, before login.
 *
 * Queries on the login screens have no account yet; giving them a name of
 * their own keeps their entries from ever being read by a logged-in account.
 */
export const ANONYMOUS_ACCOUNT = "anonymous";

/** Who a cache entry belongs to: an account id, or the login screens. */
export type AccountScope = number | typeof ANONYMOUS_ACCOUNT;

/**
 * Put the account in front of a key.
 *
 * Every cache entry carries the account that filled it, so no account can be
 * served another's data. See
 * `docs/adr/0002-one-way-to-fetch-data-in-the-web-app.md`.
 */
export function routeQueryKey(account: AccountScope, key: RouteQueryKey): unknown[] {
  return ["vault", account, ...key];
}
