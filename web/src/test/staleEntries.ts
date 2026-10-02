/**
 * Which cache entries a write left marked stale.
 *
 * A test of a write seeds the entries the write must mark stale, runs the
 * write, and asks which of them are still fresh. Asking the cache rather than
 * spying on `invalidateQueries` checks the outcome a screen depends on, and
 * the answer names each entry the write missed.
 */

import type { QueryClient } from "@tanstack/react-query";
import { type AccountScope, type RouteQueryKey, routeQueryKey } from "../lib/routeQueryKey";

/**
 * Put a value under each key for `account`.
 *
 * The entries are held for the whole test, whatever `gcTime` the client was
 * built with, because nothing on screen reads them and an entry dropped
 * before the assertion would read as one the write never marked.
 */
export function seedEntries(
  client: QueryClient,
  account: AccountScope,
  keys: readonly RouteQueryKey[],
): void {
  client.setQueryDefaults(["server", account], { gcTime: Number.POSITIVE_INFINITY });
  for (const key of keys) client.setQueryData(routeQueryKey(account, key), { seeded: true });
}

/** The keys among `keys` whose entry is not marked stale: empty once a write marked every one. */
export function freshEntries(
  client: QueryClient,
  account: AccountScope,
  keys: readonly RouteQueryKey[],
): RouteQueryKey[] {
  return keys.filter((key) => !client.getQueryState(routeQueryKey(account, key))?.isInvalidated);
}
