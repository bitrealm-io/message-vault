import { countOf, formatBytes, sectionHint, sectionTitle } from "./storageUtils";

/**
 * What the account holds: attachment bytes over the message, attachment,
 * conversation and contact counts. Counts and never names, so the owner
 * reads the same card (`docs/adr/0008-the-vault-owner-holds-no-messages.md`).
 */
export default function StorageUsageCard({
  totalBytes,
  attachmentCount,
  conversationCount,
  contactCount,
  messageCount,
}: {
  totalBytes: number;
  attachmentCount: number;
  conversationCount: number;
  contactCount: number;
  messageCount: number;
}) {
  return (
    <section>
      <h3 className={sectionTitle}>Usage</h3>
      <p className={sectionHint}>Attachment storage for this account (original file sizes).</p>
      <div className="mt-3 rounded-lg border border-border bg-elevated p-3 px-4">
        <div className="text-[1.375rem] font-semibold text-text">{formatBytes(totalBytes)}</div>
        <div className="mt-1 text-[0.813rem] text-muted">
          {countOf(messageCount, "message")}, {countOf(attachmentCount, "attachment")}
        </div>
        <div className="mt-0.5 text-[0.813rem] text-muted">
          {countOf(conversationCount, "conversation")}, {countOf(contactCount, "contact")}
        </div>
      </div>
    </section>
  );
}
