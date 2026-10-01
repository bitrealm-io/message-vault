import { countOf, formatBytes } from "../../settings/storage/storageUtils";
import { DashboardSection } from "./DashboardSection";
import type { ServerStorage } from "./types";

/**
 * What the whole database holds: attachment bytes over the four counts. No
 * message, contact or conversation is named here; the per-account breakdown
 * is each account's Storage tab under User Accounts
 * (`docs/adr/0008-the-owner-holds-no-messages.md`).
 */
export function ContentsSection({ storage }: { storage: ServerStorage }) {
  return (
    <DashboardSection title="Contents">
      <div className="rounded-xl border border-border bg-elevated p-4">
        <div className="text-[1.375rem] font-semibold text-text">
          {formatBytes(storage.total_bytes)}
        </div>
        <div className="mt-1 text-[0.813rem] text-muted">
          {countOf(storage.message_count, "message")},{" "}
          {countOf(storage.attachment_count, "attachment")}
        </div>
        <div className="mt-0.5 text-[0.813rem] text-muted">
          {countOf(storage.conversation_count, "conversation")},{" "}
          {countOf(storage.contact_count, "contact")}
        </div>
      </div>
    </DashboardSection>
  );
}
