import type { AccountProfile } from "./account";
import { getAccount, getAccountProfile } from "./vaultApi";
import { keys } from "./vaultKeys";
import { useVaultQuery } from "./vaultQuery";

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
