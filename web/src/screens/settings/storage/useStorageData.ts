import { useCallback, useEffect, useState } from "react";
import { apiErrorMessage } from "../../../lib/apiErrorMessage";
import { getAccountStorage, getImport, listImports } from "../../../lib/vaultApi";
import { keys } from "../../../lib/vaultKeys";
import { useVaultQuery } from "../../../lib/vaultQuery";
import type { ImportRow, TopAttachment } from "./storageUtils";

type StorageOverview = {
  imports: ImportRow[];
  totalBytes: number;
  attachmentCount: number;
  topAttachments: TopAttachment[];
};

async function fetchOverview(signal: AbortSignal): Promise<StorageOverview> {
  const [importsRes, usageRes] = await Promise.all([
    listImports({}, { signal }),
    getAccountStorage({ signal }),
  ]);
  return {
    imports: importsRes.items,
    totalBytes: usageRes.total_bytes ?? 0,
    attachmentCount: usageRes.attachment_count ?? 0,
    topAttachments: usageRes.top_attachments ?? [],
  };
}

/**
 * Both requests run through `useVaultQuery`, which already owns the
 * abort-on-unmount and aborted-guard handling these effects were repeating —
 * and the overview request, written by hand, had no AbortController at all.
 */
export function useStorageData() {
  const [page, setPage] = useState(0);
  const [selectedImportId, setSelectedImportId] = useState<number | null>(null);

  const {
    data: overview,
    isPending: loading,
    error,
  } = useVaultQuery(keys.storage.overview, fetchOverview);

  // A fresh overview invalidates whatever page the user was on.
  useEffect(() => {
    if (overview) setPage(0);
  }, [overview]);

  const fetchDetail = useCallback(
    (signal: AbortSignal) =>
      selectedImportId === null ? Promise.resolve(null) : getImport(selectedImportId, { signal }),
    [selectedImportId],
  );

  const {
    data: selectedImport,
    isPending: selectedImportLoading,
    error: selectedImportError,
  } = useVaultQuery(keys.storage.importDetail(selectedImportId), fetchDetail, {
    enabled: selectedImportId !== null,
  });

  const closeImportDetail = useCallback(() => {
    setSelectedImportId(null);
  }, []);

  const toggleImportDetail = useCallback((importId: number) => {
    setSelectedImportId((current) => (current === importId ? null : importId));
  }, []);

  return {
    imports: overview?.imports ?? [],
    totalBytes: overview?.totalBytes ?? 0,
    attachmentCount: overview?.attachmentCount ?? 0,
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
