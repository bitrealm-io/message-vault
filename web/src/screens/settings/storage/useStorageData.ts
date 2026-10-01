import { keepPreviousData } from "@tanstack/react-query";
import { useCallback, useEffect, useState } from "react";
import { apiErrorMessage } from "../../../lib/apiErrorMessage";
import { keys } from "../../../lib/queryKeys";
import { useRouteQuery } from "../../../lib/routeQuery";
import {
  getAccountImport,
  getAccountStorage,
  listAccountExports,
  listAccountImports,
} from "../../../lib/serverApi";
import { RUN_PAGE_SIZE, type TopAttachment } from "./storageUtils";

type StorageOverview = {
  totalBytes: number;
  attachmentCount: number;
  conversationCount: number;
  contactCount: number;
  topAttachments: TopAttachment[];
};

async function fetchOverview(signal: AbortSignal, accountId?: number): Promise<StorageOverview> {
  const usageRes = await getAccountStorage({ signal }, accountId);
  return {
    totalBytes: usageRes.total_bytes ?? 0,
    attachmentCount: usageRes.attachment_count ?? 0,
    conversationCount: usageRes.conversation_count ?? 0,
    contactCount: usageRes.contact_count ?? 0,
    topAttachments: usageRes.top_attachments ?? [],
  };
}

/** `limit` and `offset` for one page of a history table. */
function runPage(page: number) {
  return { limit: RUN_PAGE_SIZE, offset: page * RUN_PAGE_SIZE };
}

/**
 * Every request runs through `useRouteQuery`, which already owns the
 * abort-on-unmount and aborted-guard handling these effects were repeating —
 * and the overview request, written by hand, had no AbortController at all.
 *
 * The two history tables each read one page of runs from the server, so an
 * account with more runs than a page holds can reach all of them. Largest
 * attachments pages over the rows the overview already brought.
 */
export function useStorageData(managedAccountId?: number) {
  const [page, setPage] = useState(0);
  const [importPage, setImportPage] = useState(0);
  const [exportPage, setExportPage] = useState(0);
  const [selectedImportId, setSelectedImportId] = useState<number | null>(null);

  const {
    data: overview,
    isPending: overviewLoading,
    error: overviewError,
  } = useRouteQuery(
    managedAccountId === undefined
      ? keys.storage.overview
      : keys.ownerAccounts.storage(managedAccountId),
    (signal) => fetchOverview(signal, managedAccountId),
  );

  // The page on screen stays up while the next one loads, so the table does
  // not blank between pages.
  const {
    data: importsPage,
    isPending: importsLoading,
    error: importsError,
  } = useRouteQuery(
    managedAccountId === undefined
      ? keys.storage.imports(importPage)
      : keys.ownerAccounts.imports(managedAccountId, importPage),
    (signal) => listAccountImports(runPage(importPage), { signal }, managedAccountId),
    { placeholderData: keepPreviousData },
  );

  const {
    data: exportsPage,
    isPending: exportsLoading,
    error: exportsError,
  } = useRouteQuery(
    managedAccountId === undefined
      ? keys.storage.exports(exportPage)
      : keys.ownerAccounts.exports(managedAccountId, exportPage),
    (signal) => listAccountExports(runPage(exportPage), { signal }, managedAccountId),
    { placeholderData: keepPreviousData },
  );

  const loading = overviewLoading || importsLoading || exportsLoading;
  const error = overviewError ?? importsError ?? exportsError;

  // A fresh overview invalidates whatever page of attachments the user was on.
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
  } = useRouteQuery(
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
    imports: importsPage?.items ?? [],
    importTotal: importsPage?.total ?? 0,
    importPage,
    setImportPage,
    exports: exportsPage?.items ?? [],
    exportTotal: exportsPage?.total ?? 0,
    exportPage,
    setExportPage,
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
