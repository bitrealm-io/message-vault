import { type UseMutationResult, useMutation } from "@tanstack/react-query";
import type { AccountProfile } from "./account";
import { type AccountProfileChange, useUpdateAccountProfile } from "./useAccountProfile";
import { getAccount, getAccountProfile, updateAccount } from "./vaultApi";
import { keys } from "./vaultKeys";
import { useVaultCache, useVaultQuery } from "./vaultQuery";

/**
 * The account a Settings screen is about.
 *
 * With no id it is the logged-in account, from the entry every other screen
 * reads. With an id it is an account the vault owner has opened from User
 * Accounts, read from the same `/v1/accounts/{id}` row under the owner's list
 * entry, so a change to the list refreshes it.
 */
export function useSettingsAccount(managedAccountId?: number): {
  profile: AccountProfile | null;
  loading: boolean;
  error: string;
} {
  const { data, isPending, error } = useVaultQuery(
    managedAccountId === undefined
      ? keys.accountProfile.all
      : keys.ownerAccounts.member(managedAccountId),
    (signal) =>
      managedAccountId === undefined
        ? getAccountProfile({ signal })
        : getAccount(managedAccountId, { signal }),
  );
  return { profile: data ?? null, loading: isPending, error: error ? error.message : "" };
}

/**
 * Change the name, time zone or identities of the account a Settings screen
 * is about: the logged-in one, or one the vault owner opened.
 *
 * The vault answers with the account as it now stands, which goes into the
 * entry the screen reads. The owner's list shows an account's name, so a
 * managed change refreshes it.
 */
export function useUpdateSettingsProfile(
  managedAccountId?: number,
): UseMutationResult<AccountProfile, Error, AccountProfileChange> {
  const cache = useVaultCache();
  const own = useUpdateAccountProfile();
  const managed = useMutation<AccountProfile, Error, AccountProfileChange>({
    mutationFn: (body) => updateAccount(managedAccountId ?? 0, body),
    onSuccess: (profile) => {
      cache.set(keys.ownerAccounts.member(profile.account_id), profile);
      void cache.invalidate(keys.ownerAccounts.all);
    },
  });
  return managedAccountId === undefined ? own : managed;
}
