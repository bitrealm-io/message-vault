import { type UseMutationResult, useMutation } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import {
  createAccount as createVaultAccount,
  deleteAccountById,
  deleteAccountMessages,
  listAccounts,
  setAccountPassword as setVaultAccountPassword,
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
  refreshesTheList = true,
): UseMutationResult<unknown, Error, V> {
  const cache = useVaultCache();
  return useMutation<unknown, Error, V>({
    mutationFn: write,
    onSettled: refreshesTheList ? () => cache.invalidate(keys.ownerAccounts.all) : undefined,
  });
}

export function useCreateAccount(): UseMutationResult<
  unknown,
  Error,
  { username: string; password: string }
> {
  return useOwnerWrite((body) => createVaultAccount(body));
}

export function useUpdateAccount(): UseMutationResult<
  unknown,
  Error,
  { id: number; changes: ManagedAccountChanges }
> {
  return useOwnerWrite(({ id, changes }) => updateAccount(id, changes));
}

export function useDeleteAccount(): UseMutationResult<unknown, Error, number> {
  return useOwnerWrite((id: number) => deleteAccountById(id));
}

export function useDeleteAccountMessages(): UseMutationResult<unknown, Error, number> {
  return useOwnerWrite((id: number) => deleteAccountMessages(id));
}

/** Setting a password changes nothing the list shows, so it does not refresh it. */
export function useSetAccountPassword(): UseMutationResult<
  unknown,
  Error,
  { id: number; password: string }
> {
  return useOwnerWrite(({ id, password }) => setVaultAccountPassword(id, { password }), false);
}

/**
 * The vault owner's view of every account, with the one thing the table
 * itself does: add an account. A password, status, permissions and the
 * deletions are in the account's Settings, which the account's name opens.
 */
export function useOwnerAccounts() {
  const {
    data,
    isPending: loading,
    error: loadError,
  } = useVaultQuery(keys.ownerAccounts.all, fetchAccounts);
  const createAccount = useCreateAccount();

  const busy = createAccount.isPending;
  const actionError = createAccount.error ? createAccount.error.message : "";
  const clearError = createAccount.reset;

  const [composing, setComposing] = useState(false);
  const [newUsername, setNewUsername] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [newPasswordConfirm, setNewPasswordConfirm] = useState("");

  const cancelCompose = useCallback(() => {
    setComposing(false);
    setNewUsername("");
    setNewPassword("");
    setNewPasswordConfirm("");
    clearError();
  }, [clearError]);

  // The form takes the password twice and saves only when the two agree; the
  // panel disables Save until then, and this is the same rule on the way out.
  const createOne = useCallback(() => {
    const username = newUsername.trim();
    const password = newPassword;
    if (!username || !password || password !== newPasswordConfirm) return;
    createAccount.mutate(
      { username, password },
      {
        onSuccess: () => {
          setNewUsername("");
          setNewPassword("");
          setNewPasswordConfirm("");
          setComposing(false);
        },
      },
    );
  }, [newUsername, newPassword, newPasswordConfirm, createAccount.mutate]);

  return {
    accounts: data ?? [],
    loading,
    loadError: loadError ? apiErrorMessage(loadError, "Could not load accounts.") : "",
    busy,
    actionError,
    clearError,
    composing,
    setComposing,
    newUsername,
    setNewUsername,
    newPassword,
    setNewPassword,
    newPasswordConfirm,
    setNewPasswordConfirm,
    cancelCompose,
    createAccount: createOne,
  };
}
