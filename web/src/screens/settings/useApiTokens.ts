import { type UseMutationResult, useMutation } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { apiErrorMessage } from "../../lib/apiErrorMessage";
import { createApiToken, deleteApiToken, listApiTokens, renameApiToken } from "../../lib/vaultApi";
import { keys } from "../../lib/vaultKeys";
import { useVaultCache, useVaultQuery } from "../../lib/vaultQuery";
import type { ApiTokenItem } from "./apiTokensUtils";

const fetchTokens = (signal: AbortSignal) =>
  listApiTokens({ signal }).then((res) => res.items ?? []);

type NewToken = Parameters<typeof createApiToken>[0];
type CreatedToken = Awaited<ReturnType<typeof createApiToken>>;

/** Every token write marks the list stale, and the list refetches itself. */
function useApiTokenWrite<T, V>(write: (vars: V) => Promise<T>): UseMutationResult<T, Error, V> {
  const cache = useVaultCache();
  return useMutation<T, Error, V>({
    mutationFn: write,
    onSettled: () => cache.invalidate(keys.apiTokens.all),
  });
}

export function useCreateApiToken(): UseMutationResult<CreatedToken, Error, NewToken> {
  return useApiTokenWrite((body: NewToken) => createApiToken(body));
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
 * The list goes through `useVaultQuery` and each write through one of the
 * mutations above — the busy flag and error string are the union of the
 * three mutations' own state rather than a separate piece of state.
 */
export function useApiTokens() {
  const [composing, setComposing] = useState(false);
  const [label, setLabel] = useState("");
  const [canImport, setCanImport] = useState(true);
  const [canExport, setCanExport] = useState(true);
  const [canDelete, setCanDelete] = useState(false);
  const [reveal, setReveal] = useState<{ label: string; token: string } | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<ApiTokenItem | null>(null);
  const [renameTarget, setRenameTarget] = useState<ApiTokenItem | null>(null);
  const [renameLabel, setRenameLabel] = useState("");

  const {
    data,
    isPending: loading,
    error: loadError,
  } = useVaultQuery(keys.apiTokens.all, fetchTokens);
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
    setCanDelete(false);
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
        can_delete: canDelete,
      },
      {
        onSuccess: (res) => {
          setLabel("");
          setCanImport(true);
          setCanExport(true);
          setCanDelete(false);
          setComposing(false);
          setReveal({ label: res.label, token: res.token });
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

  /** The dialog closes whether or not the vault agreed; the refusal shows in `actionError`. */
  const revoke = (item: ApiTokenItem) => {
    revokeToken.mutate(item.id, { onSettled: () => setRevokeTarget(null) });
  };

  return {
    items: data ?? [],
    loading,
    loadError: loadError ? apiErrorMessage(loadError, "Could not load API keys.") : "",
    busy,
    composing,
    setComposing,
    label,
    setLabel,
    canImport,
    setCanImport,
    canExport,
    setCanExport,
    canDelete,
    setCanDelete,
    actionError,
    reveal,
    setReveal,
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
