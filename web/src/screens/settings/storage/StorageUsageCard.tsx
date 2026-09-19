import { formatBytes, sectionHint, sectionTitle } from "./storageUtils";

export default function StorageUsageCard({
  totalBytes,
  attachmentCount,
  messageCount,
}: {
  totalBytes: number;
  attachmentCount: number;
  messageCount: number;
}) {
  return (
    <section>
      <h3 className={sectionTitle}>Usage</h3>
      <p className={sectionHint}>Attachment storage for this account (original file sizes).</p>
      <div className="mt-3 rounded-lg border border-border bg-elevated p-3 px-4">
        <div className="text-[1.375rem] font-semibold text-text">{formatBytes(totalBytes)}</div>
        <div className="mt-1 text-[0.813rem] text-muted">
          {messageCount.toLocaleString()} message{messageCount === 1 ? "" : "s"},{" "}
          {attachmentCount.toLocaleString()} attachment{attachmentCount === 1 ? "" : "s"}
        </div>
      </div>
    </section>
  );
}
