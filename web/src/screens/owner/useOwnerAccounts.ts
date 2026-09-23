import { type UseMutationResult, useMutation } from "@tanstack/react-query";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import {
  deleteAccountById,
  deleteAccountMessages,
  listAccounts,
  updateAccount,
} from "../../lib/vaultApi";
import type { components } from "../../lib/vaultApi.types";
import { keys } from "../../lib/vaultKeys";
import { useVaultCache, useVaultQuery } from "../../lib/vaultQuery";

/** One account as the vault owner sees it: the same row the account itself reads. */
export type ManagedAccount = components["schemas"]["AccountResponse"];

/** The flags the vault owner can change on one account. */
export type ManagedAccountChanges = Partial<
  Pick<ManagedAccount, "disabled" | "can_import" | "can_export" | "can_delete">
>;

const fetchAccounts = (signal: AbortSignal) =>
  listAccounts({ signal }).then((res) => res.items ?? []);

/** Every write but a password change shows on the account list. */
function useOwnerWrite<V>(
  write: (vars: V) => Promise<unknown>,
): UseMutationResult<unknown, Error, V> {
  const cache = useVaultCache();
  return useMutation<unknown, Error, V>({
    mutationFn: write,
    onSettled: () => cache.invalidate(keys.ownerAccounts.all),
  });
}

/**
 * Change an account's status or permissions. The vault answers with the
 * account as it now stands, which goes straight into the entry its Settings
 * read, so a checkbox shows its new state without waiting for the list.
 */
export function useUpdateAccount(): UseMutationResult<
  ManagedAccount,
  Error,
  { id: number; changes: ManagedAccountChanges }
> {
  const cache = useVaultCache();
  return useMutation<ManagedAccount, Error, { id: number; changes: ManagedAccountChanges }>({
    mutationFn: ({ id, changes }) => updateAccount(id, changes),
    onSuccess: (account) => {
      cache.set(keys.ownerAccounts.member(account.account_id), account);
      void cache.invalidate(keys.ownerAccounts.all);
    },
  });
}

export function useDeleteAccount(): UseMutationResult<unknown, Error, number> {
  return useOwnerWrite((id: number) => deleteAccountById(id));
}

export function useDeleteAccountMessages(): UseMutationResult<unknown, Error, number> {
  return useOwnerWrite((id: number) => deleteAccountMessages(id));
}

/**
 * The vault owner's view of every account. The table changes nothing: a
 * password, status, permissions and the deletions are in the account's
 * Settings, which the account's gear opens, and a new account starts there too.
 */
export function useOwnerAccounts() {
  const {
    data,
    isPending: loading,
    error: loadError,
  } = useVaultQuery(keys.ownerAccounts.all, fetchAccounts);

  return {
    accounts: data ?? [],
    loading,
    loadError: loadError ? apiErrorMessage(loadError, "Could not load accounts.") : "",
  };
}
