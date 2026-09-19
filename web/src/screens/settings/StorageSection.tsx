import { useSettingsAccount } from "../../lib/useSettingsAccount";
import ExportHistoryTable from "./storage/ExportHistoryTable";
import ImportHistoryTable from "./storage/ImportHistoryTable";
import StorageUsageCard from "./storage/StorageUsageCard";
import { toImportSummaryView } from "./storage/storageUtils";
import TopAttachmentsTable from "./storage/TopAttachmentsTable";
import { useStorageData } from "./storage/useStorageData";

/**
 * The Storage tab: what an account holds, and its import and export history.
 *
 * Given `managedAccountId`, the account is one the vault owner opened from
 * User Accounts. All of this describes the account's data without being it,
 * so the owner reads the same screen
 * (`docs/adr/0008-the-vault-owner-holds-no-messages.md`). The one part held
 * back is which contacts an import created: the owner reads how many.
 */
export function StorageSection({ managedAccountId }: { managedAccountId?: number }) {
  const { profile } = useSettingsAccount(managedAccountId);
  const {
    imports,
    exports,
    totalBytes,
    attachmentCount,
    topAttachments,
    page,
    setPage,
    loading,
    error,
    selectedImportId,
    selectedImport,
    selectedImportLoading,
    selectedImportError,
    closeImportDetail,
    toggleImportDetail,
  } = useStorageData(managedAccountId);

  if (loading) {
    return <div className="text-[0.875rem] text-muted">Loading storage…</div>;
  }

  const selectedImportSummary = selectedImport ? toImportSummaryView(selectedImport) : null;

  return (
    <div className="flex flex-col gap-8">
      {error && (
        <div className="rounded-md border border-danger-soft-border bg-danger-soft-bg p-2 px-3 text-[0.813rem] text-danger">
          {error}
        </div>
      )}

      <StorageUsageCard
        totalBytes={totalBytes}
        attachmentCount={attachmentCount}
        messageCount={profile?.message_count ?? 0}
      />

      <ImportHistoryTable
        imports={imports}
        selectedImportId={selectedImportId}
        selectedImport={selectedImport}
        selectedImportSummary={selectedImportSummary}
        selectedImportLoading={selectedImportLoading}
        selectedImportError={selectedImportError}
        listContacts={managedAccountId === undefined}
        onToggle={toggleImportDetail}
        onCloseDetail={closeImportDetail}
      />

      <ExportHistoryTable exports={exports} />

      <TopAttachmentsTable topAttachments={topAttachments} page={page} onPageChange={setPage} />
    </div>
  );
}
