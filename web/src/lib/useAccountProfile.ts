import { type UseMutationResult, useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import type { AccountProfile } from "./account";
import { useAuth } from "./auth";
import { getAccountProfile, updateAccountProfile } from "./vaultApi";
import { keys } from "./vaultKeys";
import { useVaultCache, useVaultQuery } from "./vaultQuery";
import { ANONYMOUS_ACCOUNT, vaultQueryKey } from "./vaultQueryKey";

/**
 * The signed-in account's profile.
 *
 * This used to be a module-level store with its own in-flight guard, its own
 * subscriber list, and a `clearAccountProfile` that `auth.tsx` had to remember
 * to call on both sign-in and sign-out. All of that is TanStack Query's now,
 * and the entry is named with the account, so nothing has to be cleared for
 * one account to stop seeing another's profile.
 */

export function useAccountProfile(): {
  profile: AccountProfile | null;
  loading: boolean;
  error: string;
} {
  const { data, isPending, error } = useVaultQuery(keys.accountProfile.all, (signal) =>
    getAccountProfile({ signal }),
  );
  return { profile: data ?? null, loading: isPending, error: error ? error.message : "" };
}

/** What a change to the profile can carry: a name, handles to add, handles to drop. */
export type AccountProfileChange = Parameters<typeof updateAccountProfile>[0];

/**
 * Change the account's own name or handles.
 *
 * The vault answers with the profile as it now stands, so that answer goes
 * into the entry every screen reads. Nothing is marked stale: there is nothing
 * left to refresh.
 */
export function useUpdateAccountProfile(): UseMutationResult<
  AccountProfile,
  Error,
  AccountProfileChange
> {
  const cache = useVaultCache();
  return useMutation<AccountProfile, Error, AccountProfileChange>({
    mutationFn: (body) => updateAccountProfile(body),
    onSuccess: (profile) => {
      cache.set(keys.accountProfile.all, profile);
    },
  });
}

/**
 * Read the profile outside a render — during sign-in, and before an import
 * decides whether the backup belongs to this person.
 *
 * `accountId` is passed explicitly because sign-in knows the account before the
 * auth state carries it, and the entry has to land under the key the hook above
 * will read.
 */
export function fetchAccountProfileFor(
  client: ReturnType<typeof useQueryClient>,
  accountId: number | null,
  force = false,
): Promise<AccountProfile | null> {
  const key = vaultQueryKey(accountId ?? ANONYMOUS_ACCOUNT, keys.accountProfile.all);
  if (force) client.removeQueries({ queryKey: key });
  return client.fetchQuery({ queryKey: key, queryFn: () => getAccountProfile() }).catch(() => null);
}

/** The same, for a caller that is already inside the signed-in tree. */
export function useFetchAccountProfile(): (force?: boolean) => Promise<AccountProfile | null> {
  const client = useQueryClient();
  const { accountId } = useAuth();
  return useCallback(
    (force = false) => fetchAccountProfileFor(client, accountId, force),
    [client, accountId],
  );
}
