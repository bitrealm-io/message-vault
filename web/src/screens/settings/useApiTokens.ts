import { type UseMutationResult, useMutation } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { useRevealApiToken } from "../../components/apiTokenRevealState";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { keys } from "../../lib/queryKeys";
import { useRouteCache, useRouteQuery } from "../../lib/routeQuery";
import { createApiToken, deleteApiToken, listApiTokens, renameApiToken } from "../../lib/serverApi";
import type { ApiTokenItem } from "./apiTokensUtils";

const fetchTokens = (signal: AbortSignal) => listApiTokens({ signal });

type NewToken = Parameters<typeof createApiToken>[0];
type CreatedToken = Awaited<ReturnType<typeof createApiToken>>;

/** Every token write marks the account's cache stale, and the list refetches itself. */
function useApiTokenWrite<T, V>(
  write: (vars: V) => Promise<T>,
  onSuccess?: (res: T) => void,
): UseMutationResult<T, Error, V> {
  const cache = useRouteCache();
  return useMutation<T, Error, V>({
    mutationFn: write,
    onSuccess,
    onSettled: () => cache.invalidateAccount(),
  });
}

/**
 * The secret is revealed from the mutation's own `onSuccess`, which runs even
 * when the screen that called `mutate` has unmounted. The `onSuccess` passed
 * to `mutate` does not, so a secret revealed from there was lost whenever the
 * person left Settings before the server answered.
 */
export function useCreateApiToken(): UseMutationResult<CreatedToken, Error, NewToken> {
  const reveal = useRevealApiToken();
  return useApiTokenWrite(
    (body: NewToken) => createApiToken(body),
    (res) => reveal({ label: res.label, token: res.token }),
  );
}

export function useRenameApiToken(): UseMutationResult<
  Awaited<ReturnType<typeof renameApiToken>>,
  Error,
  { id: number; label: string }
> {
  return useApiTokenWrite(({ id, label }: { id: number; label: string }) =>
    renameApiToken(id, { label }),
  );
}

export function useRevokeApiToken(): UseMutationResult<
  Awaited<ReturnType<typeof deleteApiToken>>,
  Error,
  number
> {
  return useApiTokenWrite((id: number) => deleteApiToken(id));
}

/**
 * The list goes through `useRouteQuery` and each write through one of the
 * mutations above — the busy flag and error string are the union of the
 * three mutations' own state rather than a separate piece of state.
 */
export function useApiTokens() {
  const [composing, setComposing] = useState(false);
  const [label, setLabel] = useState("");
  const [canImport, setCanImport] = useState(true);
  const [canExport, setCanExport] = useState(true);
  const [revokeTarget, setRevokeTarget] = useState<ApiTokenItem | null>(null);
  const [renameTarget, setRenameTarget] = useState<ApiTokenItem | null>(null);
  const [renameLabel, setRenameLabel] = useState("");

  const {
    data,
    isPending: loading,
    error: loadError,
  } = useRouteQuery(keys.apiTokens.all, fetchTokens);
  const createToken = useCreateApiToken();
  const renameToken = useRenameApiToken();
  const revokeToken = useRevokeApiToken();

  const busy = createToken.isPending || renameToken.isPending || revokeToken.isPending;

  // Each mutate call resets that mutation's own error and stamps a fresh
  // `submittedAt`, so whichever of the three last started is also whichever
  // last settled; its error (or lack of one) is `actionError`. A fixed
  // create-then-rename-then-revoke order would instead let an old create
  // failure outlive a later, successful rename.
  const latest = [createToken, renameToken, revokeToken].reduce((newest, next) =>
    next.submittedAt > newest.submittedAt ? next : newest,
  );
  const actionError = latest.error ? latest.error.message : "";

  const resetCreate = createToken.reset;
  const resetRename = renameToken.reset;
  const resetRevoke = revokeToken.reset;
  const clearError = useCallback(() => {
    resetCreate();
    resetRename();
    resetRevoke();
  }, [resetCreate, resetRename, resetRevoke]);

  const cancelCompose = useCallback(() => {
    setComposing(false);
    setLabel("");
    setCanImport(true);
    setCanExport(true);
    clearError();
  }, [clearError]);

  const openRename = useCallback(
    (item: ApiTokenItem) => {
      setRenameTarget(item);
      setRenameLabel(item.label);
      clearError();
    },
    [clearError],
  );

  const closeRename = useCallback(() => {
    if (busy) return;
    setRenameTarget(null);
    setRenameLabel("");
  }, [busy]);

  const create = () => {
    const trimmed = label.trim();
    if (!trimmed) return;
    createToken.mutate(
      {
        label: trimmed,
        can_import: canImport,
        can_export: canExport,
      },
      {
        // Only the form is reset here; `useCreateApiToken` reveals the secret.
        onSuccess: () => {
          setLabel("");
          setCanImport(true);
          setCanExport(true);
          setComposing(false);
        },
      },
    );
  };

  const rename = () => {
    if (!renameTarget) return;
    const trimmed = renameLabel.trim();
    if (!trimmed) return;
    renameToken.mutate(
      { id: renameTarget.id, label: trimmed },
      {
        onSuccess: () => {
          setRenameTarget(null);
          setRenameLabel("");
        },
      },
    );
  };

  /** The dialog closes whether or not the server agreed; the refusal shows in `actionError`. */
  const revoke = (item: ApiTokenItem) => {
    revokeToken.mutate(item.id, { onSettled: () => setRevokeTarget(null) });
  };

  return {
    items: data ?? [],
    loading,
    loadError: loadError ? apiErrorMessage(loadError, "Could not load API Tokens.") : "",
    busy,
    composing,
    setComposing,
    label,
    setLabel,
    canImport,
    setCanImport,
    canExport,
    setCanExport,
    actionError,
    revokeTarget,
    setRevokeTarget,
    renameTarget,
    renameLabel,
    setRenameLabel,
    cancelCompose,
    openRename,
    closeRename,
    create,
    rename,
    revoke,
  };
}

/**
 * Another account's API tokens, as the owner sees them: each token's label,
 * permissions and use, never its masked secret. The owner revokes a token
 * here, to end one that has leaked, and makes and renames none.
 */
export function useManagedApiTokens(accountId: number) {
  const [revokeTarget, setRevokeTarget] = useState<ApiTokenItem | null>(null);
  const {
    data,
    isPending: loading,
    error: loadError,
  } = useRouteQuery(keys.ownerAccounts.apiTokens(accountId), (signal) =>
    listApiTokens({ signal }, accountId),
  );
  const revokeToken = useApiTokenWrite((id: number) => deleteApiToken(id, accountId));

  /** The dialog closes whether or not the server agreed; the refusal shows in `actionError`. */
  const revoke = (item: ApiTokenItem) => {
    revokeToken.mutate(item.id, { onSettled: () => setRevokeTarget(null) });
  };

  return {
    items: data ?? [],
    loading,
    loadError: loadError ? apiErrorMessage(loadError, "Could not load API Tokens.") : "",
    busy: revokeToken.isPending,
    actionError: revokeToken.error ? revokeToken.error.message : "",
    revokeTarget,
    setRevokeTarget,
    revoke,
  };
}
