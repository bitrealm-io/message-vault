import ExportHistoryTable from "./storage/ExportHistoryTable";
import ImportHistoryTable from "./storage/ImportHistoryTable";
import StorageUsageCard from "./storage/StorageUsageCard";
import { toImportSummaryView } from "./storage/storageUtils";
import TopAttachmentsTable from "./storage/TopAttachmentsTable";
import { useStorageData } from "./storage/useStorageData";

export function StorageSection() {
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
  } = useStorageData();

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

      <StorageUsageCard totalBytes={totalBytes} attachmentCount={attachmentCount} />

      <ImportHistoryTable
        imports={imports}
        selectedImportId={selectedImportId}
        selectedImport={selectedImport}
        selectedImportSummary={selectedImportSummary}
        selectedImportLoading={selectedImportLoading}
        selectedImportError={selectedImportError}
        onToggle={toggleImportDetail}
        onCloseDetail={closeImportDetail}
      />

      <ExportHistoryTable exports={exports} />

      <TopAttachmentsTable topAttachments={topAttachments} page={page} onPageChange={setPage} />
    </div>
  );
}
