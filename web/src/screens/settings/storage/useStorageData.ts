import { useCallback, useEffect, useState } from "react";
import { apiErrorMessage } from "../../../lib/apiErrorMessage";
import {
  getAccountImport,
  getAccountStorage,
  listAccountExports,
  listAccountImports,
} from "../../../lib/vaultApi";
import { keys } from "../../../lib/vaultKeys";
import { useVaultQuery } from "../../../lib/vaultQuery";
import type { ExportRow, ImportRow, TopAttachment } from "./storageUtils";

type StorageOverview = {
  imports: ImportRow[];
  exports: ExportRow[];
  totalBytes: number;
  attachmentCount: number;
  conversationCount: number;
  contactCount: number;
  topAttachments: TopAttachment[];
};

async function fetchOverview(signal: AbortSignal, accountId?: number): Promise<StorageOverview> {
  const [importsRes, exportsRes, usageRes] = await Promise.all([
    listAccountImports({ signal }, accountId),
    listAccountExports({ signal }, accountId),
    getAccountStorage({ signal }, accountId),
  ]);
  return {
    imports: importsRes.items,
    exports: exportsRes.items,
    totalBytes: usageRes.total_bytes ?? 0,
    attachmentCount: usageRes.attachment_count ?? 0,
    conversationCount: usageRes.conversation_count ?? 0,
    contactCount: usageRes.contact_count ?? 0,
    topAttachments: usageRes.top_attachments ?? [],
  };
}

/**
 * Both requests run through `useVaultQuery`, which already owns the
 * abort-on-unmount and aborted-guard handling these effects were repeating —
 * and the overview request, written by hand, had no AbortController at all.
 */
export function useStorageData(managedAccountId?: number) {
  const [page, setPage] = useState(0);
  const [selectedImportId, setSelectedImportId] = useState<number | null>(null);

  const {
    data: overview,
    isPending: loading,
    error,
  } = useVaultQuery(
    managedAccountId === undefined
      ? keys.storage.overview
      : keys.ownerAccounts.storage(managedAccountId),
    (signal) => fetchOverview(signal, managedAccountId),
  );

  // A fresh overview invalidates whatever page the user was on.
  useEffect(() => {
    if (overview) setPage(0);
  }, [overview]);

  const fetchDetail = useCallback(
    (signal: AbortSignal) =>
      selectedImportId === null
        ? Promise.resolve(null)
        : getAccountImport(selectedImportId, { signal }, managedAccountId),
    [selectedImportId, managedAccountId],
  );

  const {
    data: selectedImport,
    isPending: selectedImportLoading,
    error: selectedImportError,
  } = useVaultQuery(
    managedAccountId === undefined
      ? keys.storage.importDetail(selectedImportId)
      : keys.ownerAccounts.importDetail(managedAccountId, selectedImportId),
    fetchDetail,
    {
      enabled: selectedImportId !== null,
    },
  );

  const closeImportDetail = useCallback(() => {
    setSelectedImportId(null);
  }, []);

  const toggleImportDetail = useCallback((importId: number) => {
    setSelectedImportId((current) => (current === importId ? null : importId));
  }, []);

  return {
    imports: overview?.imports ?? [],
    exports: overview?.exports ?? [],
    totalBytes: overview?.totalBytes ?? 0,
    attachmentCount: overview?.attachmentCount ?? 0,
    conversationCount: overview?.conversationCount ?? 0,
    contactCount: overview?.contactCount ?? 0,
    topAttachments: overview?.topAttachments ?? [],
    page,
    setPage,
    loading,
    error: error ? apiErrorMessage(error, "Could not load storage.") : "",
    selectedImportId,
    selectedImport: selectedImport ?? null,
    selectedImportLoading,
    selectedImportError: selectedImportError
      ? apiErrorMessage(selectedImportError, "Could not load this import.")
      : "",
    closeImportDetail,
    toggleImportDetail,
  };
}
