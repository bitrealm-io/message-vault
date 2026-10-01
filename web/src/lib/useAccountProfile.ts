import { type UseMutationResult, useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import type { AccountProfile } from "./account";
import { useAuth } from "./auth";
import { keys } from "./queryKeys";
import { useRouteCache, useRouteQuery } from "./routeQuery";
import { ANONYMOUS_ACCOUNT, routeQueryKey } from "./routeQueryKey";
import { getAccountProfile, updateAccountProfile } from "./serverApi";

/**
 * The logged-in account's profile.
 *
 * This used to be a module-level store with its own in-flight guard, its own
 * subscriber list, and a `clearAccountProfile` that `auth.tsx` had to remember
 * to call on both login and logout. All of that is TanStack Query's now,
 * and the entry is named with the account, so nothing has to be cleared for
 * one account to stop seeing another's profile.
 */

export function useAccountProfile(): {
  profile: AccountProfile | null;
  loading: boolean;
  error: string;
} {
  const { data, isPending, error } = useRouteQuery(keys.accountProfile.all, (signal) =>
    getAccountProfile({ signal }),
  );
  return { profile: data ?? null, loading: isPending, error: error ? error.message : "" };
}

/** What a change to the profile can carry: a name, identities to add, identities to drop. */
export type AccountProfileChange = Parameters<typeof updateAccountProfile>[0];

/**
 * Change the account’s own name or identities.
 *
 * The server answers with the profile as it now stands, so that answer goes
 * into the entry every screen reads. Nothing is marked stale: there is nothing
 * left to refresh.
 */
export function useUpdateAccountProfile(): UseMutationResult<
  AccountProfile,
  Error,
  AccountProfileChange
> {
  const cache = useRouteCache();
  return useMutation<AccountProfile, Error, AccountProfileChange>({
    mutationFn: (body) => updateAccountProfile(body),
    onSuccess: (profile) => {
      cache.set(keys.accountProfile.all, profile);
      // An identity added or removed changes the identities list and its counts.
      void cache.invalidate(keys.accountProfile.identities);
    },
  });
}

/**
 * Read the profile outside a render — during login, and before an import
 * decides whether the backup belongs to this person.
 *
 * `accountId` is passed explicitly because login knows the account before the
 * auth state carries it, and the entry has to land under the key the hook above
 * will read.
 */
export function fetchAccountProfileFor(
  client: ReturnType<typeof useQueryClient>,
  accountId: number | null,
  force = false,
): Promise<AccountProfile | null> {
  const key = routeQueryKey(accountId ?? ANONYMOUS_ACCOUNT, keys.accountProfile.all);
  if (force) client.removeQueries({ queryKey: key });
  return client.fetchQuery({ queryKey: key, queryFn: () => getAccountProfile() }).catch(() => null);
}

/** The same, for a caller that is already inside the logged-in tree. */
export function useFetchAccountProfile(): (force?: boolean) => Promise<AccountProfile | null> {
  const client = useQueryClient();
  const { accountId } = useAuth();
  return useCallback(
    (force = false) => fetchAccountProfileFor(client, accountId, force),
    [client, accountId],
  );
}
