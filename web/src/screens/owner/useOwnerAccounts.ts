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
import { keys } from "../../lib/vaultKeys";
import { useVaultCache, useVaultQuery } from "../../lib/vaultQuery";

/** One account as the vault owner sees it — mirrors `ManagedAccount` in `owner_api.rs`. */
export type ManagedAccount = {
  account_id: number;
  username: string;
  disabled: boolean;
  must_change_password: boolean;
  can_import: boolean;
  can_export: boolean;
  can_delete: boolean;
  message_count: number;
  storage_bytes: number;
};

/** The flags the vault owner can change on one account. */
export type ManagedAccountChanges = Partial<
  Pick<ManagedAccount, "disabled" | "can_import" | "can_export" | "can_delete">
>;

const fetchAccounts = (signal: AbortSignal) =>
  listAccounts({ signal }).then((res) => (res.items ?? []) as ManagedAccount[]);

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

/**
 * Setting a password does show on the list — it sets `must_change_password` —
 * so unlike its predecessor this one refreshes too.
 */
export function useSetAccountPassword(): UseMutationResult<
  unknown,
  Error,
  { id: number; password: string }
> {
  return useOwnerWrite(({ id, password }) => setVaultAccountPassword(id, { password }));
}

/** The vault owner's view of every account, plus the actions on one. */
export function useOwnerAccounts() {
  const {
    data,
    isPending: loading,
    error: loadError,
  } = useVaultQuery(keys.ownerAccounts.all, fetchAccounts);
  const createAccount = useCreateAccount();
  const changeAccount = useUpdateAccount();
  const removeAccount = useDeleteAccount();
  const removeMessages = useDeleteAccountMessages();
  const changePassword = useSetAccountPassword();

  const busy =
    createAccount.isPending ||
    changeAccount.isPending ||
    removeAccount.isPending ||
    removeMessages.isPending ||
    changePassword.isPending;

  // Each mutate call resets that mutation's own error and stamps a fresh
  // `submittedAt`, so whichever of the five last started is also whichever
  // last settled; its error (or lack of one) is `actionError`. A fixed
  // create-then-update-then-… order would instead let an old failure outlive
  // a later, successful write.
  const latest = [
    createAccount,
    changeAccount,
    removeAccount,
    removeMessages,
    changePassword,
  ].reduce((newest, next) => (next.submittedAt > newest.submittedAt ? next : newest));
  const actionError = latest.error ? latest.error.message : "";

  const resetCreate = createAccount.reset;
  const resetChange = changeAccount.reset;
  const resetRemove = removeAccount.reset;
  const resetRemoveMessages = removeMessages.reset;
  const resetChangePassword = changePassword.reset;
  const clearError = useCallback(() => {
    resetCreate();
    resetChange();
    resetRemove();
    resetRemoveMessages();
    resetChangePassword();
  }, [resetCreate, resetChange, resetRemove, resetRemoveMessages, resetChangePassword]);

  const [composing, setComposing] = useState(false);
  const [newUsername, setNewUsername] = useState("");
  const [newPassword, setNewPassword] = useState("");

  const [passwordTarget, setPasswordTarget] = useState<ManagedAccount | null>(null);
  const [resetPasswordValue, setResetPasswordValue] = useState("");

  const cancelCompose = useCallback(() => {
    setComposing(false);
    setNewUsername("");
    setNewPassword("");
    clearError();
  }, [clearError]);

  const createOne = useCallback(() => {
    const username = newUsername.trim();
    const password = newPassword;
    if (!username || !password) return;
    createAccount.mutate(
      { username, password },
      {
        onSuccess: () => {
          setNewUsername("");
          setNewPassword("");
          setComposing(false);
        },
      },
    );
  }, [newUsername, newPassword, createAccount.mutate]);

  const openPasswordReset = useCallback(
    (account: ManagedAccount) => {
      clearError();
      setPasswordTarget(account);
      setResetPasswordValue("");
    },
    [clearError],
  );

  const closePasswordReset = useCallback(() => {
    if (busy) return;
    setPasswordTarget(null);
    setResetPasswordValue("");
  }, [busy]);

  const setAccountPassword = useCallback(() => {
    if (!passwordTarget || !resetPasswordValue) return;
    changePassword.mutate(
      { id: passwordTarget.account_id, password: resetPasswordValue },
      {
        onSuccess: () => {
          setPasswordTarget(null);
          setResetPasswordValue("");
        },
      },
    );
  }, [passwordTarget, resetPasswordValue, changePassword.mutate]);

  const patch = useCallback(
    (id: number, changes: ManagedAccountChanges) =>
      changeAccount.mutateAsync({ id, changes }).then(
        () => undefined,
        () => undefined,
      ),
    [changeAccount.mutateAsync],
  );

  // These two answer whether the vault agreed, so the confirmation dialog can
  // stay open and show the refusal instead of closing as though it had worked.
  const deleteMessages = useCallback(
    (id: number) =>
      removeMessages.mutateAsync(id).then(
        () => true,
        () => false,
      ),
    [removeMessages.mutateAsync],
  );

  const deleteOne = useCallback(
    (id: number) =>
      removeAccount.mutateAsync(id).then(
        () => true,
        () => false,
      ),
    [removeAccount.mutateAsync],
  );

  return {
    accounts: data ?? [],
    loading,
    loadError: loadError ? apiErrorMessage(loadError, "Could not load accounts.") : "",
    busy,
    actionError,
    clearError,
    patch,
    deleteMessages,
    deleteAccount: deleteOne,
    composing,
    setComposing,
    newUsername,
    setNewUsername,
    newPassword,
    setNewPassword,
    cancelCompose,
    createAccount: createOne,
    passwordTarget,
    resetPassword: resetPasswordValue,
    setResetPassword: setResetPasswordValue,
    openPasswordReset,
    closePasswordReset,
    setAccountPassword,
  };
}
